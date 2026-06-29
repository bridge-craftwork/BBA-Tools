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

Current build: EPBot 8740, patched build dated 2026-05-03 (Edward), shipped in BBA-Tools v2.2.4. See "EPBot 25-day uptime crash" below for context on what that patch is.

### EPBot 25-day uptime crash

**Status as of 2026-05-04: fix shipped in v2.2.4, verification window 2026-05-29 ~21:18 UTC.** Until that date passes without recurrence, treat the bug as live.

**Root cause.** EPBot's NativeAOT C# code uses `GetTickCount()` (Win32 DWORD, `uint`, ms since system boot) to track lead-bid timing, but stores it in a private field `m_Lead_Tick_Count` typed as `int` and accesses it via unchecked casts. After **~24.855 days of system uptime** (`Int32.MaxValue` ms), the cast yields a large negative number. Subsequent `Math.Abs(num3 - m_Lead_Tick_Count)` can hit `Math.Abs(Int32.MinValue)` — which has no positive `Int32` representation and throws `OverflowException`. The exception bubbles out of `epbot_create()` as null, surfaces in our Rust as `EPBotError::CreateFailed("...Arithmetic operation resulted in an overflow.")`.

The proper fix (Edward's patch) is to type the field as `uint`, drop the casts, and use unsigned subtraction (which wraps cleanly across the `uint` boundary, giving correct elapsed-ms deltas for intervals < ~49.7 days).

**Confirmed incidents:**
- **Droplet, 2026-04-09** — first observed crash. Initially attributed to a coincident `libssl3` update; we now think the package update was incidental and the trigger was simply uptime crossing the threshold. Reboot resolved.
- **David's Mac, 2026-05-03** — same overflow on his pipeline. Mac had been up >25 days. Reboot resolved; ran fine after.
- **Droplet, 2026-05-04 22:17 UTC** — recurred after exactly 24.872 days uptime (boot was 2026-04-10 01:46 UTC; threshold is 24.855 days). Detected by Rick when bridge-classroom started erroring; first user-reported error came in within ~25 minutes of the threshold being crossed. Reboot at ~22:46 restored service. New uptime clock started.

**Verification plan for v2.2.4.** Edward's patched library has the same version label (8740) as the previous build but materially different bytes across all platforms (macOS +36 KB, Linux +40 KB, Windows ~+2.7 MB — the Windows jump also reflects switching from a legacy COM wrapper to a real AOT build). We can't test the timing fix locally without waiting 25 days or time-shifting a machine. **Watch the droplet on 2026-05-29 ~21:18 UTC.** If it survives that window without overflow, the patch landed. If it crashes, Edward needs another round.

**Diagnostic clue if it recurs.** The failure is *partial*: a single `epbot_create()` call (e.g., `bba-cli`'s startup version probe at [main.rs:99](bba-cli/src/main.rs#L99)) often succeeds — only subsequent calls into the lead-tick code path crash. So a normal-looking `BBA-CLI vX (EPBot 8740)` startup line **does NOT** mean EPBot is healthy; you have to test an actual auction. After deploys, always run a real `POST /api/auction/generate` against the droplet, not just the health endpoint.

**Don't be fooled by the bbsa context.** The error message often appears alongside convention card filenames in surrounding log lines, which makes it look like a bbsa parsing issue. It isn't — `epbot_create()` runs *before* any convention is loaded ([lib.rs:420-433](epbot-core/src/lib.rs#L420-L433)).

**Workaround order if v2.2.4 doesn't fix it:**
1. Reboot the affected machine. Resets `GetTickCount()` to 0; gives ~25 fresh days.
2. Schedule monthly reboots if the bug stays unfixed upstream (cron-driven, in a low-traffic window).
3. Patch the AOT source ourselves if Edward stops responding — fix is small and well-understood.

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

### Local macOS build

```bash
# CLI
cd bba-cli && cargo build --release

# Server
cd bba-server && cargo build --release

# Run server locally
DYLD_LIBRARY_PATH=../epbot-libs/macos/arm64 cargo run
```

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
