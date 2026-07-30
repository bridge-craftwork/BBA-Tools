# Claude Code Instructions for BBA-Tools

## Architecture

BBA-Tools is a pure Rust project using Edward Piwowar's native EPBot libraries (NativeAOT-compiled .NET → native shared libraries). No .NET runtime needed at runtime.

### Components

| Directory | Purpose |
|-----------|---------|
| `epbot-core/` | Shared Rust crate: FFI bindings to native EPBot, auction orchestration, convention loading |
| `bba-cli/` | CLI binary (`bba-cli`): batch PBN processing |
| `bba-server/` | Axum web server (`bba-server`): REST API for browser extensions |
| `epbot-libs/` | Native EPBot libraries per platform (checked into repo) |
| `legacy/` | Retired C# code (`bba-server-cs`, `bba-cli-cs`, `epbot-wrapper`) and old Windows tooling, kept as reference. Not built by CI. |
| `history/` | Archived documentation from the Windows-hosted era |

### EPBot Native Libraries

From Edward Piwowar's NativeAOT build. Located in `epbot-libs/`:
- `linux/x64/libEPBot.so`, `linux/arm64/libEPBot.so`
- `macos/arm64/libEPBot.dylib`
- `windows/x64/EPBot.dll`, `windows/arm64/EPBot.dll` (untested — proper AOT builds first shipped in v2.2.4)

Current build: EPBot 8740, Edward's patched build, shipped in BBA-Tools v2.2.4 (committed 2026-05-04). The "8740" label and file dates are NOT reliable identifiers — an earlier 2026-05-03 build carries the same label and leaked into installs. Identify the patched build by sha256 (fingerprints under "EPBot 25-day uptime crash" below). See that section for what the patch fixes.

### EPBot 25-day uptime crash

**Status as of 2026-07-01: RESOLVED — the v2.2.4 patch works; the recurrence was a stale install, not a patch failure.** David's Mac (and Rick's, found the same day) was still running a pre-patch **2026-05-03** macOS dylib (`e82e4471…`) in `/Applications/Bridge Utilities/`; it was never bumped to the patched **2026-05-04** build (`ded470bf…`). Past ~25 days uptime that old dylib overflows on every `epbot_create()`. David's Mac was at 29 days uptime; clock changes never helped because the overflow keys off *uptime*, not wall-clock. The Linux droplet, by contrast, was correctly updated to the patched `.so` on 2026-05-04 and has run clean.

**Proof the patch works.** The droplet's loaded `libEPBot.so` is a sha256-exact match to the repo's patched build (`e0e48200…`, 3,929,144 B). It sat at 57 days OS uptime through its 24.855-day danger window (~2026-05-29) and the entire negative-tick window (late May–~2026-06-23) with **zero** overflow rows in the auction audit logs, and it bids live today. That is exactly the confirmation the old "watch 2026-05-29" plan was waiting for.

**Authoritative fingerprints — identify a build by sha256, NOT by the "8740" label or file date (all report 8740):**
- macOS arm64 patched `libEPBot.dylib`: `ded470bf10e1f65f2d775c8b6860cde4c6ebf76b20610d3074971278173d8ca5` (3,741,088 B)
- Linux x64 patched `libEPBot.so`: `e0e482000de4c65cda1415a18475aaa1a31037e27d4c0a0ebe2aa642f9abd39f` (3,929,144 B)
- **Known-bad** stale macOS build (2026-05-03, the one found in installs): `e82e44715ac5b9c259bf1d1b0f33048ca5d7758d1437815b1b5662628a674301` (3,726,288 B)

**Fix for an affected machine:** replace the install's `libEPBot.dylib` with the patched build (sha `ded470bf…`) — no reboot needed, the patched build handles the overflow at any uptime. Rebooting is only a stopgap for a still-unpatched dylib.

**Root cause.** EPBot's NativeAOT C# code uses `GetTickCount()` (Win32 DWORD, `uint`, ms since system boot) to track lead-bid timing, but stores it in a private field `m_Lead_Tick_Count` typed as `int` and accesses it via unchecked casts. After **~24.855 days of system uptime** (`Int32.MaxValue` ms), the cast yields a large negative number. Subsequent `Math.Abs(num3 - m_Lead_Tick_Count)` can hit `Math.Abs(Int32.MinValue)` — which has no positive `Int32` representation and throws `OverflowException`. The exception bubbles out of `epbot_create()` as null, surfaces in our Rust as `EPBotError::CreateFailed("...Arithmetic operation resulted in an overflow.")`.

The proper fix (Edward's patch) is to type the field as `uint`, drop the casts, and use unsigned subtraction (which wraps cleanly across the `uint` boundary, giving correct elapsed-ms deltas for intervals < ~49.7 days).

**Confirmed incidents:**
- **Droplet, 2026-04-09** — first observed crash. Initially attributed to a coincident `libssl3` update; we now think the package update was incidental and the trigger was simply uptime crossing the threshold. Reboot resolved.
- **David's Mac, 2026-05-03** — same overflow on his pipeline. Mac had been up >25 days. Reboot resolved; ran fine after.
- **Droplet, 2026-05-04 22:17 UTC** — recurred after exactly 24.872 days uptime (boot was 2026-04-10 01:46 UTC; threshold is 24.855 days). Detected by Rick when bridge-classroom started erroring; first user-reported error came in within ~25 minutes of the threshold being crossed. Reboot at ~22:46 restored service. New uptime clock started.
- **David's Mac, 2026-07-01** — bba-cli failed at `epbot_create()` on every call (0 auctions, empty bba committed). Mac was at 29 days uptime. **Root cause was a stale install, not a v2.2.4 defect:** his `/Applications/Bridge Utilities/libEPBot.dylib` was the pre-patch 2026-05-03 build (`e82e4471…`) — same as Rick's install, found stale the same day. Clock changes (May 5 / June 1 / July 1, all identical) didn't help because the overflow is uptime-based, not wall-clock; there is no license code. Fixed by swapping in the patched `ded470bf…` dylib — no reboot. Also surfaced bba-cli issue #2: the stale May 3 bba-cli exited 0 on 0 auctions, so the pipeline silently committed empty output (now fixed by the `auctions_generated == 0` guard).

**How it was verified.** Edward's patched library has the same version label (8740) as the previous build but materially different bytes across all platforms (macOS +36 KB, Linux +40 KB, Windows ~+2.7 MB — the Windows jump also reflects switching from a legacy COM wrapper to a real AOT build). We couldn't test the timing fix locally without a machine >25 days up (time-shifting the wall clock does nothing — the bug is uptime-based). The droplet supplied the proof: its patched Linux `.so` is sha-verified and survived the full danger + negative-tick window with zero overflows (see Status above). The macOS patched dylib (`ded470bf…`) is the same-batch sibling; David's 29-day Mac provides the past-threshold macOS confirmation once he swaps to it.

**Diagnostic clue if it recurs.** The failure was originally observed as *partial*: on the droplet near the threshold, a single `epbot_create()` call (e.g., `bba-cli`'s startup version probe at [main.rs:99](bba-cli/src/main.rs#L99)) often succeeded — only subsequent calls into the lead-tick code path crashed. David's 2026-07-01 case (stale unpatched dylib, 29 days uptime) is the opposite: the probe itself fails, i.e. *every* create overflows. Both are consistent with the same uptime overflow — "first call succeeds" is a near-threshold timing coincidence, whereas well past the threshold the tick delta is negative on every call. Bottom line: a normal-looking `BBA-CLI vX (EPBot 8740)` startup line **does NOT** mean EPBot is healthy; you have to test an actual auction. After deploys, always run a real `POST /api/auction/generate` against the droplet, not just the health endpoint. Note also that bba-cli now exits non-zero when 0 auctions are generated ([main.rs](bba-cli/src/main.rs), `auctions_generated == 0` guard), so a dead engine surfaces as a failed pipeline step rather than an empty bba.

**The patch is NOT bidding-neutral.** Established 2026-07-30 while refreshing the test fixtures. The pre-patch build (`b434aa7a…`, the original macOS dylib) and the patched build (`ded470bf…`) bid differently on a measurable fraction of deals:

| Fixture | Boards | Differ | Different contract |
|---------|--------|--------|--------------------|
| `tests/fixtures/slow/1N.pbn` | 501 | 40 (7%) | 16 |
| `tests/fixtures/slow/Fourth_Suit_Forcing.pbn` | 501 | 66 (13%) | 35 |

Proven by extracting the pre-patch dylib from git (`git show dbf721f^:epbot-libs/macos/arm64/libEPBot.dylib`) and re-running the fixtures: the old goldens reproduce **byte-identically** under `b434aa7a…` and diverge under `ded470bf…`. So Edward's 8740 rebuild changed more than the tick arithmetic — treat it as a bidding-behavior change too, not just an overflow fix. Both builds report version 8740, so the label cannot distinguish them; use sha256. The goldens in `tests/fixtures/expected/` now pin the **patched** behavior, which is what has shipped since v2.2.4 and runs in production.

**Don't be fooled by the bbsa context.** The error message often appears alongside convention card filenames in surrounding log lines, which makes it look like a bbsa parsing issue. It isn't — `epbot_create()` runs *before* any convention is loaded ([lib.rs:420-433](epbot-core/src/lib.rs#L420-L433)).

**Install hygiene (the 2026-07-01 lesson).** The droplet got the patched `.so` on 2026-05-04, but the macOS `/Applications/Bridge Utilities/` install was never bumped from the 2026-05-03 build — bridge-wrangler and pbn-to-pdf there were refreshed 2026-06-19, yet the `bba-cli` + `libEPBot.dylib` pair was skipped and sat stale for ~2 months. When shipping an EPBot update, update **every** install target (droplet + each Mac running the pipeline) and verify each dylib/so by **sha256** against the fingerprints above — not by version label or file date, since both builds report "8740". The install pipeline references `/Applications/Bridge Utilities/bba-cli` (see `Practice-Bidding-Scenarios/build-scripts-mac/config.py`); the binary loads the dylib sitting next to it via `@executable_path` rpath, so the two must be updated together. A healthy-looking `BBA-CLI vX (EPBot 8740)` startup line does **not** prove the dylib is patched.

**Workaround order if a machine is still on an unpatched dylib:**
1. Best fix: drop in the patched dylib (sha `ded470bf…` on macOS). Works at any uptime, no reboot.
2. Reboot the affected machine. Resets `GetTickCount()` to 0; gives ~25 fresh days. Only a stopgap.
3. Schedule monthly reboots if a machine somehow can't be updated (cron-driven, low-traffic window).
4. Patch the AOT source ourselves if Edward stops responding — fix is small and well-understood.

## BBA Server (Production)

The Rust bba-server runs on a DigitalOcean droplet, behind Caddy reverse proxy.

### Server Details

| Item | Value |
|------|-------|
| Droplet IP | `146.190.135.172` |
| SSH | `ssh root@146.190.135.172` (Mac id_ed25519 key) |
| Public URL | `https://bba.harmonicsystems.com` |
| Install path | `/opt/bba-server/` |
| Systemd service | `bba-server` |
| Reverse proxy | Caddy at `/opt/livekit/Caddyfile` |
| DNS | Cloudflare A record → droplet IP (DNS only, Caddy handles TLS) |
| Also on droplet | LiveKit at `/opt/livekit/` (docker-compose) |

### Key Endpoints

- `GET /health` - Health check
- `POST /api/auction/generate` - Generate auction for a deal
- `GET /api/scenarios` - List available scenarios
- `POST /api/scenario/select` - Record scenario selection (analytics)

### Admin Dashboard

- `GET /admin/dashboard?key=<admin_key>` - Usage stats, charts, request history
- `GET /admin/whoami` - Debug endpoint showing detected IP and access status

Admin access via `?key=` query parameter. Admin users (for filtering): `Valerie_Perez`, `Travis_Scott`, `Tom_Martinez`, `Carol_Jordan`, `Joe_Evans`, `Rebecca_Coleman`, `Timothy_Carter`

The dashboard HTML is served from disk at `/opt/bba-server/wwwroot/dashboard.html` — editable without rebuilding the binary.

### Server Management

**Check status:**
```bash
ssh root@146.190.135.172 'systemctl status bba-server --no-pager'
```

**View logs:**
```bash
ssh root@146.190.135.172 'journalctl -u bba-server -n 50 --no-pager'
```

**Deploy new version** (after CI builds a release):
```bash
ssh root@146.190.135.172 'bash -s' << 'REMOTE'
systemctl stop bba-server
cd /opt/bba-server
curl -sL https://github.com/bridge-craftwork/BBA-Tools/releases/download/TAG/bba-TAG-linux-x64.tar.gz | tar xz
systemctl start bba-server
REMOTE
```

**Update dashboard only** (no rebuild needed):
```bash
scp bba-server/wwwroot/dashboard.html root@146.190.135.172:/opt/bba-server/wwwroot/
```

**Restart Caddy** (if Caddyfile changes):
```bash
ssh root@146.190.135.172 'cd /opt/livekit && docker compose restart caddy'
```

### Maintenance & Updates

Automatic reboots are disabled (`/etc/apt/apt.conf.d/51no-auto-reboot`). Unattended security upgrades still install but won't reboot.

**Important:** Until v2.2.4's EPBot patch is verified (see "EPBot 25-day uptime crash" above), the droplet WILL crash again ~25 days after each boot. Track uptime: `ssh root@146.190.135.172 'uptime -s; uptime'`. Next predicted failure window: 2026-05-29 ~21:18 UTC if the v2.2.4 patch didn't take.

**Before applying OS updates:**
1. Check for pending updates: `ssh root@146.190.135.172 'apt list --upgradable'`
2. Plan a maintenance window (low-traffic period)
3. Apply updates: `ssh root@146.190.135.172 'apt upgrade -y'`
4. Restart bba-server: `ssh root@146.190.135.172 'systemctl restart bba-server'`
5. Verify with a real auction request, not just `/health`: `curl -X POST https://bba.harmonicsystems.com/api/auction/generate -H "Content-Type: application/json" -d '{"deal":{"pbn":"N:.63.AKQ987.A9732 A8654.KQ5.T.QJT4 KQT9.J98742.J.K8 J732.AT.65432.65","dealer":"N","vulnerability":"None"}}'`
6. If EPBot fails, reboot: `ssh root@146.190.135.172 'reboot'`

**Check for pending reboot:** `ssh root@146.190.135.172 'cat /var/run/reboot-required 2>/dev/null || echo "no reboot required"'`

### Configuration

Environment file: `/opt/bba-server/.env`

```
HOST=0.0.0.0
PORT=5000
LOG_PATH=/opt/bba-server/logs
MAX_CONCURRENCY=4
DEFAULT_NS_CARD=21GF-DEFAULT
DEFAULT_EW_CARD=21GF-GIB
GITHUB_RAW_BASE_URL=https://raw.githubusercontent.com/ADavidBailey/Practice-Bidding-Scenarios/main
ADMIN_USERS=Valerie_Perez,Travis_Scott,Tom_Martinez,Carol_Jordan,Joe_Evans,Rebecca_Coleman,Timothy_Carter
ADMIN_KEY=goosebumps
```

Convention cards (.bbsa) and scenario files (.pbs) are fetched from GitHub at runtime.

### Logs

Logs are in `/opt/bba-server/logs/`:
- `audit-auction-YYYY-MM.csv` - Auction request audit log
- `audit-scenario-YYYY-MM.csv` - Scenario selection audit log

CSV columns (current format):
- Auction: `Timestamp,RequestIP,ClientVersion,Extension,Browser,OS,DurationMs,Version,EPBotVersion,Dealer,Vulnerability,Scoring,NSConvention,EWConvention,Scenario,PBN,Success,Auction,Alerts,Error`
- Scenario: `Timestamp,RequestIP,ClientVersion,Extension,Browser,OS,Version,Scenario`

### Client Info Header

Browser extensions send `X-Client-Info: ext=BBOAlert|PBSforBBO; browser=Chrome|Firefox|Safari|Edge; os=Windows|macOS|Linux` for environment tracking.

## Building

GitHub Actions (`.github/workflows/build.yml`) builds all platforms on push to main. Tagged releases (`v*`) create GitHub Releases.

### Local builds — use `./dev-build.sh`, not bare cargo

**Use `./dev-build.sh` for local development builds, not bare cargo.** `bba-cli` depends on the sibling `Bridge-Parsers`, which pulls `bridge-types` and `bridge-encodings` as git dependencies, with gitignored `[patch]` overrides in `.cargo/config.toml` redirecting them to the local checkouts in `../`. Cargo never lets a `[patch]` override an existing `Cargo.lock` pin, so bare `cargo build` silently compiles the GitHub revisions of those crates instead of your local edits — and if the patches do take effect, they rewrite `Cargo.lock` with local-path entries that must never be committed (CI has no sibling checkouts). The script keeps a separate local lock per crate (`.cargo/dev-<crate>.lock`), swaps it in around the cargo call, verifies each patched crate resolved to a local checkout, and leaves the committed `Cargo.lock` untouched. It also exports `DYLD_LIBRARY_PATH`/`LD_LIBRARY_PATH` for `epbot-libs`, without which `cargo test` and `cargo run` abort at load time with `Library not loaded: @rpath/libEPBot.dylib`.

This repo is not a single cargo workspace — each crate carries its own lock — so the script takes the crate as its first argument, or infers it from the current directory:

```bash
./dev-build.sh bba-cli build --release     # CLI
./dev-build.sh bba-server build --release  # server
./dev-build.sh epbot-core test             # any cargo subcommand + args
cd bba-cli && ../dev-build.sh test         # crate inferred from $PWD
./dev-build.sh bba-server run              # run server locally
cargo fmt --check                          # no dependency resolution; bare cargo is fine
```

For CI-parity builds (pre-commit checks, release verification) use `./dev-build.sh --ci <crate> test` — it temporarily disables the local patches and builds with the committed lock's git pins. **Avoid bare cargo for anything that resolves dependencies** (build/test/check/run): with the patches present, a same-version patch is applied immediately and silently rewrites `Cargo.lock` to local-path entries, while a version mismatch makes the patches silently ignored — both wrong. For `bba-server` and `epbot-core` the patches are inert, and bare cargo instead appends `[[patch.unused]]` entries to their locks. The committed `Cargo.lock` must always pin `git+https://` sources for the internal crates; never commit a lock where those entries have lost their `source =` lines. (`epbot-core/Cargo.lock` is gitignored — the binaries' locks govern.)

### Dependencies

- `epbot-core` depends on native EPBot library at link time
- `bba-cli` depends on `epbot-core` and `bridge-parsers` (sibling repo at `../../Bridge-Parsers`)
- `bba-server` depends on `epbot-core`

## Windows VM Access via SSH

The Windows VM is still used for testing Windows-specific EPBot functionality and the legacy C# components.

### SSH Runner

```python
import os, sys
os.environ['WINDOWS_HOST'] = '10.211.55.5'
os.environ['WINDOWS_USER'] = 'Rick'
sys.path.insert(0, '/Users/rick/Development/GitHub/Practice-Bidding-Scenarios/build-scripts-mac')
from ssh_runner import run_windows_command
```

### Drive Mappings

| Windows Drive | Mac Path |
|--------------|----------|
| `G:` | `/Users/rick/Development/GitHub` |
| `P:` | `/Users/rick/Development/GitHub/Practice-Bidding-Scenarios` |

### Convention Files

- Mac: `/Users/rick/Development/GitHub/Practice-Bidding-Scenarios/bbsa/`
- Windows: `P:\bbsa\`
- Default convention: `21GF-DEFAULT.bbsa`
