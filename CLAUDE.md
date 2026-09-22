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

Current build: EPBot 8740, Edward's patched build, shipped in BBA-Tools v2.2.4 (committed 2026-05-04). The "8740" label and file dates are NOT reliable identifiers — an earlier 2026-05-03 build carries the same label and leaked into installs, and Edward's repo now holds a *newer* 2026-05-07 rebuild that also reports 8740 and bids differently from ours. Identify the patched build by sha256 (fingerprints under "EPBot 25-day uptime crash" below). See that section for what the patch fixes.

### EPBot 25-day uptime crash

**Status as of 2026-07-01: RESOLVED — the v2.2.4 patch works; the recurrence was a stale install, not a patch failure.** David's Mac (and Rick's, found the same day) was still running a pre-patch **2026-05-03** macOS dylib (`e82e4471…`) in `/Applications/Bridge Utilities/`; it was never bumped to the patched **2026-05-04** build (`ded470bf…`). Past ~25 days uptime that old dylib overflows on every `epbot_create()`. David's Mac was at 29 days uptime; clock changes never helped because the overflow keys off *uptime*, not wall-clock. The Linux droplet, by contrast, was correctly updated to the patched `.so` on 2026-05-04 and has run clean.

**Proof the patch works.** The droplet's loaded `libEPBot.so` is a sha256-exact match to the repo's patched build (`e0e48200…`, 3,929,144 B). It sat at 57 days OS uptime through its 24.855-day danger window (~2026-05-29) and the entire negative-tick window (late May–~2026-06-23) with **zero** overflow rows in the auction audit logs, and it bids live today. That is exactly the confirmation the old "watch 2026-05-29" plan was waiting for.

**Upstream tracking — [EdwardPiwowar/BBA#137](https://github.com/EdwardPiwowar/BBA/issues/137), "New version of BBA released".** Edward's long-running release thread (open since 2023-12-27, 329 comments as of 2026-08-26) and the de-facto channel for everything EPBot: build announcements, bug reports, and library drops that never reach the repo. **Check it before trusting `Native-libraries/` in his repo** — builds get posted here as zip attachments and are sometimes never committed (see 8741 below). Regulars: EdwardPiwowar (owner), ThorvaldAagaard (BEN), ADavidBailey, Rick-Wilson. Rick's comments run 2026-03-08→2026-05-02 and are the origin of the native-library builds — Edward's C ABI *is* Rick's `EPBotFFI.cs` from [Rick-Wilson/bba-native-libraries](https://github.com/Rick-Wilson/bba-native-libraries). Thread has been silent since 2026-05-10.

Note Edward considers native/wasm library builds Rick's side of the fence — *"Maybe Rick will create libraries based on EPBotNet.dll"* (2026-05-09). `bba-native-libraries` already builds them via GitHub Actions (matrix: `osx-arm64`, `linux-x64`, `win-x64`, from `dll/EPBot8739.dll`), so a new target belongs there rather than in a request to him.

**Authoritative fingerprints — identify a build by sha256, NOT by the "8740" label or file date (all report 8740).**

*Repo/source builds (as committed here — adhoc/linker-signed; stable across time):*
- macOS arm64 patched `libEPBot.dylib`: `ded470bf10e1f65f2d775c8b6860cde4c6ebf76b20610d3074971278173d8ca5` (3,741,088 B)
- Linux x64 patched `libEPBot.so`: `e0e482000de4c65cda1415a18475aaa1a31037e27d4c0a0ebe2aa642f9abd39f` (3,929,144 B)
- Linux arm64 patched `libEPBot.so`: `481027fea96bcf0e1d0a5f54d03bf413c14218e5dcef02ff28f5716c273feadf` (3,884,008 B)
- Windows x64 patched `EPBot.dll`: `587ca3b2055f8b86ea53b37e361b245286aee950a5de98ecbb6e4ba332b93bf0` (3,840,512 B)
- Windows arm64 patched `EPBot.dll`: `fbdfd2d68ec6547ab9e0fe033deb488bdd4ee10b3c8251bef68b8028a4000b1c` (3,785,216 B)

**Upstream now ships a THIRD distinct 8740 build — newer than ours, and NOT bidding-equivalent (established 2026-08-26).** Edward's repo (`github.com/EdwardPiwowar/BBA`, `Native-libraries/`) committed a rebuild on **2026-05-07**, three days after the 2026-05-04 hand-off we ship. Every platform differs from ours by sha256 while being **byte-for-byte the same size** and still reporting version **8740** — so size, label, and date *all* fail to distinguish them. Only sha256 does. Upstream shas:

| Platform | Ours (repo, 2026-05-04) | Upstream (2026-05-07) |
|----------|-------------------------|------------------------|
| macos/arm64 | `ded470bf…` | `c3d7146ced0542597dbe16d008acd82ea916568c6cbb10ace5d685f22431d1c3` |
| linux/x64 | `e0e48200…` | `1a1b5f4df4d4d7d1cee212430a53d79359dc6174de222161e9de35e622d5315d` |
| linux/arm64 | `481027fe…` | `1de565314d3c9ee2d81c7a4a609d317e59c04b047978c96e4a86fdf171493e5a` |
| windows/x64 | `587ca3b2…` | `c3c351aa6b15ac1f3a5dcb6eeadd59066b2e9bcfb0372948fd047bf692da55aa` |
| windows/arm64 | `fbdfd2d6…` | `6066832679acf0b614953e4843d1a98eecb9e1879bef38f27ee9d55cb0aa4087` |

A/B of the upstream macOS dylib against ours through `bba-cli` on the slow fixtures (21GF-DEFAULT / 21GF-GIB):

| Fixture | Boards | Differ | Different contract |
|---------|--------|--------|--------------------|
| `tests/fixtures/slow/1N.pbn` | 500 | 2 (0.4%) | 1 |
| `tests/fixtures/slow/Fourth_Suit_Forcing.pbn` | 500 | 50 (10%) | 36 |

The upstream build looks *better* on the FSF fixture — it finds real fourth-suit-forcing sequences (`1C-1H-1S-2D` with a "Fourth suit game force" alert) where ours jumps straight to 2H — but that is an inference from one inspected board, not a graded comparison across all 50. **Do not upgrade casually:** it moves production auctions and requires a goldens refresh in `tests/fixtures/expected/`.

**A FOURTH build exists — "8741", the one we should actually upgrade to (found 2026-08-26).** Posted by Edward on 2026-05-10 as a **zip attachment in [BBA#137](https://github.com/EdwardPiwowar/BBA/issues/137) only** — [EPBot-libraries-8741.zip](https://github.com/user-attachments/files/27563301/EPBot-libraries-8741.zip). It was **never committed to his repo**, so the repo's 2026-05-07 files are missing its fix. It still self-reports version **8740**.

It fixes a real crash ThorvaldAagaard reported (2026-05-08→10): pointer-returning `get_info_*` calls segfaulting at `position=13`. **That crash almost certainly cannot reach us** — we pass seat positions 0–3, and Rick's `EPBotFFI.cs` ABI fills a caller-supplied buffer and returns a status code rather than returning a pointer to dereference (see [epbot-core/src/lib.rs:742](epbot-core/src/lib.rs#L742)). The reported failure was a Python ctypes binding dereferencing returned pointers.

8741 fingerprints (sha256 / bytes):
- macOS arm64 `libEPBot.dylib`: `c1ca27b5eede4a55c92220c8816fc1c9b2b41038caa9b4be0f4ba625f18f1fbb` (3,741,088 B)
- Linux x64 `libEPBot.so`: `63e83a9503b87fcbf0a6637075cb323497b5b0a4c40f2c9ab98eaf411056af28` (3,929,144 B)
- Linux arm64 `libEPBot.so`: `2ac0e466cb795c3b7b0e4b9d8b38f319a35d8ef0ece0334fe820c9a3c83b6277` (3,884,008 B)
- Windows x64 `EPBot.dll`: `2df86fdcff077dbcd5366ccefe94587d7f478a13c99d21b0520e8bbc300a023c` (3,841,024 B)
- Windows arm64 `EPBot.dll`: `8e0612207b99a21e74e78da8f5c794488e0d6e66babdbcf93103e63a7d83abd8` (3,785,216 B)

**8741 is bidding-identical to the 2026-05-07 repo build** — verified on both slow fixtures, 0 boards differing. So the bidding change happened between our 2026-05-04 hand-off and 2026-05-07; 8741 adds only the pointer fix on top. **If we upgrade, upgrade to 8741, not to the repo files.**

**Open actions (as of 2026-08-26):** (1) ask Edward what changed in the 2026-05-07 rebuild that moved the bidding, and ask him to **commit 8741** rather than leaving it as a thread attachment; (2) ask him to bump the version number (or expose a build id) whenever the binaries change — ThorvaldAagaard asked for the same thing on 2026-05-09, so the request has a second voice behind it; (3) decide on the 8741 upgrade, which needs a goldens refresh either way.

**Runtime-independent reference engine.** Edward also publishes the engine as plain IL: `EPBotNET.dll` (957,440 B, assembly version 0.0.0.8740) at his repo root and under `Native-libraries/wasm/`. It loads on stock .NET 10 (verified on macOS arm64), needs **no** native EPBot and no shim — the `kernel32!GetTickCount` P/Invoke that breaks the older 8736 IL is never reached in 8740 — and it reproduced a production `bba-server` auction exactly. Useful as a cross-check when you need to know what "the engine" says without a platform binary. Note it matches *our* 2026-05-04 behavior on the deal tested, not necessarily upstream's 2026-05-07 rebuild.

**IMPORTANT — installed macOS copies do NOT match the repo sha.** The release workflow re-signs the macOS dylib with Developer ID **plus a per-build trusted timestamp**, so a dylib installed from a `.dmg` has a *different* sha than `ded470bf…` — and it even differs release-to-release for byte-identical code. So:
- Verify a **source/repo** dylib against `ded470bf…`.
- Verify an **installed** dylib against **that specific release's** DMG asset, or against the bba-cli version string, or by behavior (does it bid past 25 days uptime). Do NOT expect an install to equal `ded470bf…`. Per-release shipped dylib shas:
  - v2.2.5: `aab58732ea7bd3e971080a727e558fd85c1bfc525be7a8759f54647d1a468ce0`
  - v2.3.2: `455ffd7921eb337ae5b18de8b384f7efa7ef035a4ef2c0ef59d467bddff49266` (shipped bba-cli `9a5e3a177cc25feec8899374829e727280d76b04203d9135bd94f818602bbc4a`) — installed to `/Applications/Bridge Utilities/` on Rick's Mac 2026-07-30, replacing bba-cli 0.2.3
  - v2.4.0: `2174c35c99f5a793996be163784c148f3fdc728752af1deb68bc1c1293c532f6` (shipped bba-cli `f51bf66ee31ea195c90f7a2bd4330872a22647fb40f7d40b9803223176cc9778`, bba-server `09baca7f0e6aa1443e1bb37a1206727e0f7cd63d59bebc25e318cf6e74a12d4d`; DMG `6a13f05e5e2e1b40b52891c947c1af5dce2409886af9676defe14afd8913621b`) — notarized. Installed to `/Applications/Bridge Utilities/` on Rick's Mac 2026-09-22, replacing the v2.3.2 pair (kept as `bba-cli.bak-20260922` / `libEPBot.dylib.bak-20260922`); verified by sha against the DMG and by bidding a real file. **David's Mac is still on v2.3.2.** bba-cli 0.3.0 is the first release whose PBN output preserves the input's tags (#26), so the first pipeline run after each install rewrites `bba/` wholesale.
- The Linux `.so` is *not* re-signed, so installs there do match `e0e48200…` (the droplet is a sha-exact match).

*Known-bad unpatched macOS builds seen in the wild (each overflows past ~25 days; all came from direct hand-offs, never a signed release — the repo only ever held two macOS dylibs, the original `b434aa7a…` and the patched `ded470bf…`):*
- `e82e44715ac5b9c259bf1d1b0f33048ca5d7758d1437815b1b5662628a674301` (3,726,288 B, 2026-05-03) — Rick's stale install
- `39269d8dc86a87246ee7b038e2bef92a46eea3b0251924b98b61689027ef95d1` — David's stale install (found 2026-07-01; a fourth distinct binary, not in the repo)

**Fix for an affected machine:** install the notarized `.dmg` from the matching release (drag bba-cli + bba-server + libEPBot.dylib into `/Applications/Bridge Utilities/`), or drop in the patched source dylib (`ded470bf…`). No reboot needed — the patched build handles the overflow at any uptime. Rebooting is only a stopgap for a still-unpatched dylib. Verify the install per the note above (release-DMG sha / version / behavior), not against `ded470bf…`.

**Root cause.** EPBot's NativeAOT C# code uses `GetTickCount()` (Win32 DWORD, `uint`, ms since system boot) to track lead-bid timing, but stores it in a private field `m_Lead_Tick_Count` typed as `int` and accesses it via unchecked casts. After **~24.855 days of system uptime** (`Int32.MaxValue` ms), the cast yields a large negative number. Subsequent `Math.Abs(num3 - m_Lead_Tick_Count)` can hit `Math.Abs(Int32.MinValue)` — which has no positive `Int32` representation and throws `OverflowException`. The exception bubbles out of `epbot_create()` as null, surfaces in our Rust as `EPBotError::CreateFailed("...Arithmetic operation resulted in an overflow.")`.

**What Edward actually changed (corrected 2026-08-26 from [BBA#137](https://github.com/EdwardPiwowar/BBA/issues/137)).** Earlier revisions of this file claimed the patch typed the field as `uint` and used unsigned subtraction. That is *not* what he did — he went the opposite way, and said so explicitly: *"I think a simple change from UInteger to Integer will suffice. I used UInteger at Claude's suggestion, but he now says it's incorrect :)"* (2026-05-02). The shipped patch is:

```vb
Public Function GetTickCount() As Integer
GetTickCount = Environment.TickCount
End Function
```

**Why that works is probably not the mechanism described above.** VB.NET narrowing conversions are checked by default, so the old `UInteger`-returning `GetTickCount` assigned into the `Integer` field would itself throw `OverflowException` once the tick count passed `Int32.MaxValue` — before any `Math.Abs` was reached. Returning `Integer` directly removes the conversion entirely. `Environment.TickCount` is still an `Int32` that goes negative at ~24.9 days, but consecutive readings sit close together, so the delta stays small and the `Math.Abs` gate is safe in practice. Treat the checked-narrowing explanation as **inference** — it reconciles his one-line fix with the droplet's 57-day clean run, but the thread does not state it and we have not decompiled the patched build to confirm.

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
| Reverse proxy | Caddy at `/opt/edge/` (moved out of `/opt/livekit/` per platform ADR 0004) |
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

**Caddyfile changes** live in the **bridge-craftwork-platform** repo
(`edge/Caddyfile`), not here — that one Caddy fronts every hostname on the
droplet. Edit there, merge, then from the Mac:
```bash
./mac/scripts/sync.sh                             # in bridge-craftwork-platform
ssh root@146.190.135.172 '/opt/edge/scripts/reload.sh'
```
Note bba-server emits its **own** CORS headers (its Caddy stanza is a bare
`reverse_proxy`), so a CORS change for this service belongs in
`bba-server/src/main.rs`, not the Caddyfile.

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
