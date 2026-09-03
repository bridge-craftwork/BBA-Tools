#!/bin/bash
#
# dev-build.sh - run cargo against local sibling checkouts, reliably.
#
# Why this exists: bba-cli depends on ../../Bridge-Parsers, which pulls
# bridge-types and bridge-encodings as *git* dependencies. The gitignored
# [patch] overrides in .cargo/config.toml redirect those to local checkouts
# during development. That combination is a trap for bare cargo:
#
#   * When the local crate's version EQUALS the locked one, any resolving
#     cargo command (build/test/check/run) applies the patch immediately and
#     silently REWRITES Cargo.lock with local-path entries that must never
#     be committed (CI has no sibling checkouts).
#   * When the versions differ, the patch is silently IGNORED and you build
#     the GitHub revisions instead of your local edits.
#
# Either way bare cargo does the wrong thing, so always go through this
# script. It keeps two lockfiles per crate and swaps them around the cargo
# call:
#
#   <crate>/Cargo.lock        committed lock, pinned to git sources (CI truth)
#   .cargo/dev-<crate>.lock   local-only lock, resolved with the patches applied
#
# and verifies every patched crate in the dependency graph actually resolved
# to a local path, failing loudly if not. The committed Cargo.lock is never
# touched.
#
# Unlike bridge-solver's copy of this script, BBA-Tools is not a single cargo
# workspace: bba-cli, bba-server and epbot-core each carry their own
# Cargo.lock, so the script operates on one crate at a time. It also exports
# the EPBot library path, without which `cargo test` aborts at load time
# (dyld: Library not loaded: @rpath/libEPBot.dylib).
#
# Only bba-cli actually consumes the patched crates. For bba-server and
# epbot-core the patches are inert, and bare cargo appends [[patch.unused]]
# entries to their locks — noise this script also avoids.
#
# Config discovery: cargo merges .cargo/config.toml from every *ancestor* of
# the invocation directory, so the overrides that apply here are not
# necessarily next to this script. In a git worktree under
# .claude/worktrees/<name>/ there is no local .cargo/ at all, yet the main
# checkout's config still patches the build. Looking only beside the script
# made this script fall through to a bare invocation in exactly that case --
# the one place bare cargo silently corrupts a lockfile, and with --ci not
# even reaching the guard that would have said so. So we walk up the way
# cargo does and manage whichever config we find. Lockfiles stay per-worktree
# (Cargo.lock is), while the config, and therefore the --ci move-aside, may be
# shared: don't run two --ci builds against the same config concurrently.
#
# Usage:
#   ./dev-build.sh bba-cli                  # cargo build in bba-cli
#   ./dev-build.sh bba-cli test             # cargo test in bba-cli
#   ./dev-build.sh bba-server build --release
#   cd bba-cli && ../dev-build.sh test      # crate inferred from $PWD
#   ./dev-build.sh --ci bba-cli test        # CI-parity: patches disabled
#
set -euo pipefail

ROOT=$(cd "$(dirname "$0")" && pwd)

# Both spellings cargo accepts, newest first.
CONFIG_NAMES=(config.toml config)

# Nearest ancestor .cargo/ config, starting at $1, that actually carries
# [patch.] overrides. Filtering on [patch.] during the walk matters: a
# ~/.cargo/config.toml without one must not stop the search.
find_patch_config() {
    local dir=$1 name
    while :; do
        for name in "${CONFIG_NAMES[@]}"; do
            if [[ -f $dir/.cargo/$name ]] && grep -q '^\[patch\.' "$dir/.cargo/$name"; then
                printf '%s\n' "$dir/.cargo/$name"
                return 0
            fi
        done
        if [[ $dir == / ]]; then
            return 1
        fi
        dir=$(dirname "$dir")
    done
}

# Nearest ancestor marker left by an in-flight (or crashed) --ci run.
find_disabled_config() {
    local dir=$1 name
    while :; do
        for name in "${CONFIG_NAMES[@]}"; do
            if [[ -f $dir/.cargo/$name.ci-off ]]; then
                printf '%s\n' "$dir/.cargo/$name.ci-off"
                return 0
            fi
        done
        if [[ $dir == / ]]; then
            return 1
        fi
        dir=$(dirname "$dir")
    done
}

ci_mode=""
if [[ ${1:-} == --ci ]]; then
    ci_mode=1
    shift
fi

# Pick the crate: an explicit first argument naming a crate directory, else
# the current directory if it is one.
crate_dir=""
if [[ ${1:-} != "" && -f $ROOT/${1%/}/Cargo.toml ]]; then
    crate_dir=$ROOT/${1%/}
    shift
elif [[ -f $PWD/Cargo.toml ]]; then
    crate_dir=$PWD
else
    echo "dev-build: no crate selected. Run from inside a crate, or name one:" >&2
    for c in "$ROOT"/*/Cargo.toml; do
        [[ -f $c ]] && echo "  ./dev-build.sh $(basename "$(dirname "$c")") [cargo args]" >&2
    done
    exit 1
fi

crate=$(basename "$crate_dir")
DEV_LOCK=$ROOT/.cargo/dev-$crate.lock
CI_LOCK_STASH=$ROOT/.cargo/ci-$crate.lock.swap

[[ $# -eq 0 ]] && set -- build

cd "$crate_dir"

# EPBot is loaded via @rpath/LD_LIBRARY_PATH; without this, test binaries and
# `cargo run` abort before main().
case "$(uname -s)" in
    Darwin) lib_dir=$ROOT/epbot-libs/macos/$(uname -m | sed 's/^x86_64$/x64/;s/^aarch64$/arm64/')
            [[ -d $lib_dir ]] && export DYLD_LIBRARY_PATH="$lib_dir${DYLD_LIBRARY_PATH:+:$DYLD_LIBRARY_PATH}" ;;
    Linux)  lib_dir=$ROOT/epbot-libs/linux/$(uname -m | sed 's/^x86_64$/x64/;s/^aarch64$/arm64/')
            [[ -d $lib_dir ]] && export LD_LIBRARY_PATH="$lib_dir${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}" ;;
esac

CONFIG=$(find_patch_config "$crate_dir") || CONFIG=""

# No local patch overrides anywhere above us: behave exactly like a direct
# invocation.
if [[ -z $CONFIG ]]; then
    stray=$(find_disabled_config "$crate_dir") || stray=""
    if [[ -n $stray ]]; then
        echo "dev-build: ERROR: $stray exists." >&2
        echo "dev-build: another --ci run has the patch overrides moved aside, or one" >&2
        echo "dev-build: crashed before restoring them. Wait for it, or rename that file" >&2
        echo "dev-build: back to ${stray%.ci-off} if nothing else is running." >&2
        exit 1
    fi
    exec cargo "$@"
fi

CONFIG_DIR=$(dirname "$CONFIG")
CONFIG_OFF="$CONFIG.ci-off"

# We have to be able to move the config aside (--ci) and read the patch list
# (dev). Falling back to bare cargo here is not safe: cargo would still apply
# these overrides and rewrite the lockfile with local-path entries.
if [[ ! -w $CONFIG || ! -w $CONFIG_DIR ]]; then
    echo "dev-build: ERROR: $CONFIG carries [patch] overrides but is not writable" >&2
    echo "dev-build: (neither is $CONFIG_DIR), so this script cannot disable or" >&2
    echo "dev-build: inspect them. Refusing to run bare cargo, which would apply the" >&2
    echo "dev-build: patches and rewrite the lockfile with local-path entries." >&2
    exit 1
fi

if [[ $CONFIG_DIR != "$ROOT/.cargo" ]]; then
    echo "dev-build: patch overrides from $CONFIG" >&2
fi

# --- CI-parity mode: disable the patches, build with the committed lock ---
if [[ -n $ci_mode ]]; then
    # Only a *tracked* lock can be committed by mistake; epbot-core's is
    # gitignored (the binaries' locks govern), so churn there is harmless.
    lock_before=""
    if [[ -f Cargo.lock ]] && git ls-files --error-unmatch Cargo.lock >/dev/null 2>&1; then
        lock_before=$(cksum < Cargo.lock)
    fi
    mv "$CONFIG" "$CONFIG_OFF"
    restore_ci() { [[ -f $CONFIG_OFF ]] && mv "$CONFIG_OFF" "$CONFIG"; }
    trap restore_ci EXIT
    cargo "$@"
    if [[ -n $lock_before && $(cksum < Cargo.lock) != "$lock_before" ]]; then
        echo "dev-build: NOTE: Cargo.lock was re-resolved during this CI-parity run." >&2
        echo "dev-build: review 'git diff $crate/Cargo.lock' — internal crates must keep" >&2
        echo "dev-build: their source = \"git+https://...\" lines before committing." >&2
    fi
    exit 0
fi

# --- dev mode: swap in the dev lock, build against local checkouts ---

# Lockfiles are per-manifest, so they always live beside *this* checkout even
# when the config above is shared with the main worktree.
mkdir -p "$ROOT/.cargo"

# Crate names the config patches to local paths.
patched=$(sed -n 's/^\([A-Za-z0-9_-]*\) *= *{ *path *=.*/\1/p' "$CONFIG")

swapped=""
restore() {
    if [[ -n $swapped ]]; then
        [[ -f Cargo.lock ]] && mv Cargo.lock "$DEV_LOCK"
        [[ -f $CI_LOCK_STASH ]] && mv "$CI_LOCK_STASH" Cargo.lock
    fi
}
trap restore EXIT

# If the committed (CI) lock is tracked, set it aside and use the dev lock;
# cargo re-creates the dev lock from scratch if it doesn't exist yet, and a
# fresh resolve does honor the config patches.
if git ls-files --error-unmatch Cargo.lock >/dev/null 2>&1; then
    swapped=1
    mv Cargo.lock "$CI_LOCK_STASH"
    [[ -f $DEV_LOCK ]] && mv "$DEV_LOCK" Cargo.lock
fi

# How a crate resolved in the lock: "path", "remote", or "" if it is not a
# real dependency. Only [[package]] blocks count — cargo also emits
# [[patch.unused]] blocks (for crates this binary does not actually depend
# on) that carry a name but never a `source =`, and would otherwise read as
# a successful local resolve.
lock_status() {
    [[ -f Cargo.lock ]] || return 0
    awk -v want="$1" 'BEGIN { RS = "" }
        $0 ~ /^\[\[package\]\]/ && $0 ~ ("(^|\n)name = \"" want "\"(\n|$)") {
            print ($0 ~ /(^|\n)source = /) ? "remote" : "path"
            exit
        }' Cargo.lock
}

# True when every patched crate that is really depended on is path-resolved.
verify() {
    local ok=0 c
    for c in $patched; do
        if [[ $(lock_status "$c") == remote ]]; then
            echo "dev-build: $c still resolves to a remote source" >&2
            ok=1
        fi
    done
    return $ok
}

cargo "$@"

if [[ -f Cargo.lock ]] && ! verify; then
    # Stale dev lock from before the patches existed; it is disposable —
    # discard it and re-resolve fresh, which applies the patches.
    echo "dev-build: discarding stale dev lock and re-resolving..." >&2
    rm Cargo.lock
    cargo "$@"
    verify || {
        echo "dev-build: ERROR: patched crates still resolve to remote sources." >&2
        echo "dev-build: check that the sibling checkouts in $CONFIG exist." >&2
        exit 1
    }
fi

used=""
for c in $patched; do
    if [[ $(lock_status "$c") == path ]]; then
        echo "dev-build: ✓ $c → local checkout"
        used=1
    fi
done
[[ -z $used ]] && echo "dev-build: ✓ $crate does not depend on the patched crates"
exit 0
