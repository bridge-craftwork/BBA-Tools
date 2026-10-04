# BBA Server

A web API server that wraps the EPBot bridge bidding engine to generate auctions for bridge deals. The server is designed to be called by BBOAlert browser extension to compare actual auctions with "correct" bidding.

## Public Endpoint

**URL**: `https://bba.harmonicsystems.com`

The server is exposed to the internet via Cloudflare Tunnel, which provides:
- HTTPS encryption (automatic)
- DDoS protection
- No open ports on the host network

## Architecture

```
Client (BBOAlert/curl)              BBA Server (Windows VM)
┌─────────────────────┐             ┌────────────────────────────────┐
│ HTTPS request       │             │ ASP.NET Core Minimal API       │
│ via Cloudflare      │ ──────────► │ - /health                      │
│ Tunnel              │             │ - /api/auction/generate        │
│                     │ ◄────────── │ - /api/scenarios               │
└─────────────────────┘             │                                │
                                    │ Wraps EPBot64.dll (COM)        │
                                    │ Reads .dlr/.bbsa from PBS      │
                                    └────────────────────────────────┘
```

## API Endpoints

### Health Check

```bash
curl https://bba.harmonicsystems.com/health
```

Response:
```json
{"status":"healthy","timestamp":"2026-01-16T20:32:04.7196884Z"}
```

### Generate Auction

```bash
curl -X POST \
  -H "Content-Type: application/json" \
  -d '{
    "deal": {
      "pbn": "N:A653.Q97.K64.954 KQ4.AT8432.A72.A J8.K.QJ98.KJT762 T972.J65.T53.Q83",
      "dealer": "S",
      "vulnerability": "NS",
      "scoring": "MP"
    },
    "scenario": "Smolen"
  }' \
  https://bba.harmonicsystems.com/api/auction/generate
```

Response:
```json
{
  "success": true,
  "auction": ["1C", "Pass", "1S", "2H", "Pass", "Pass", "Pass"],
  "auctionEncoded": "1C--1S2H------",
  "conventionsUsed": {
    "ns": "21GF-DEFAULT",
    "ew": "21GF-GIB"
  },
  "meanings": [...],
  "error": null
}
```

**Request Fields:**
| Field | Description |
|-------|-------------|
| `deal.pbn` | PBN format deal string (required) |
| `deal.dealer` | Dealer position: N, E, S, or W (required) |
| `deal.vulnerability` | Vulnerability: None, NS, EW, or Both (required) |
| `deal.scoring` | Scoring: MP (matchpoints) or IMP (default: MP) |
| `scenario` | Scenario name for convention card lookup (optional) |
| `conventions.ns` / `conventions.ew` | Explicit convention cards (optional) |

### List Scenarios

```bash
curl https://bba.harmonicsystems.com/api/scenarios
```

Response:
```json
{
  "scenarios": ["Smolen", "Jacoby_2N", "Lebensohl", ...]
}
```

### Record Scenario Selection

```bash
curl -X POST \
  -H "Content-Type: application/json" \
  -d '{"scenario": "Smolen"}' \
  https://bba.harmonicsystems.com/api/scenario/select
```

Records when a user selects a practice scenario (for analytics).

## Admin Dashboard

The admin dashboard provides log viewing and usage statistics. Access is restricted by IP whitelist.

### Admin URLs

| Endpoint | Description |
|----------|-------------|
| `/admin/dashboard` | Main dashboard with stats, charts, and request history |
| `/admin/whoami` | Debug endpoint showing your detected IP and access status |
| `/admin/api/stats` | JSON statistics data |
| `/admin/api/logs` | List of available log files |
| `/admin/api/logs/{filename}` | Get specific log file content |

**Dashboard URL**: https://bba.harmonicsystems.com/admin/dashboard

### Access Control

Admin access is controlled by:
1. **Localhost** - Always allowed (127.0.0.1, ::1)
2. **IP Whitelist** - Anonymized IP names that are allowed:
   - `Valerie_Perez` - David (external)
   - `Travis_Scott` - Rick (external)
   - `Tom_Martinez` - Rick (local via Parallels)

To check your access status, visit `/admin/whoami`:
```bash
curl https://bba.harmonicsystems.com/admin/whoami
```

Response:
```json
{
  "rawIP": "1.2.3.4",
  "anonymizedIP": "Travis_Scott",
  "isAllowed": true,
  "whitelistedNames": ["Valerie_Perez", "Travis_Scott", "Tom_Martinez"]
}
```

### Dashboard Features

- **Stats Cards** - Total requests, success/failure counts, average/max duration, unique users
- **Daily Chart** - Bar chart of requests per day (last 14 days)
- **Scenario Table** - Request counts by scenario
- **User Table** - Request counts by anonymized user
- **Recent Requests** - Last 100 auction requests with details
- **Errors** - Recent error messages
- **Hide Admin Users** - Checkbox (top right) to filter out admin traffic from all views

## Server Management

The server runs on a Windows VM (Parallels on Mac) and is managed via SSH.

### Management Scripts

All scripts are in `/Users/rick/Development/GitHub/BBA-Tools/bba-server/`:

| Script | Purpose |
|--------|---------|
| `restart-server.py` | Full restart: stop, start, health check, test call |
| `start-server.py` | Start/stop/status/build commands |
| `test-scenario.py` | Test auctions against reference PBN files |
| `setup-cloudflare.py` | Install and configure Cloudflare Tunnel |

### Common Operations

**Restart the server:**
```bash
python3 restart-server.py
```

**Check server status:**
```bash
python3 start-server.py status
```

**Build the server:**
```bash
python3 start-server.py build
```

**Test a scenario:**
```bash
python3 test-scenario.py Smolen all
python3 test-scenario.py Smolen 1-5
python3 test-scenario.py Smolen 1,3,7 --verbose
```

## Windows VM Setup

### Prerequisites

- Windows 11 ARM (Parallels VM)
- .NET 8 SDK
- OpenSSH Server enabled
- EPBot DLLs in `G:\BBA-Tools\epbot-wrapper\`
- PBS project mapped to `P:\` drive

### Directory Structure on Windows

```
G:\BBA-Tools\
├── bba-server\
│   ├── bin\Debug\net8.0-windows\
│   │   ├── bba-server.exe
│   │   ├── start-hidden.vbs      # Runs server without window
│   │   └── logs\                 # Log files
│   ├── Services\
│   ├── Models\
│   └── Program.cs
└── epbot-wrapper\
    ├── EPBotARM64.dll
    └── *.bbsa files

P:\                               # Mapped to PBS project
├── dlr\                          # Scenario definitions
└── bbsa\                         # Convention card files

C:\Users\rick\.cloudflared\
├── cert.pem                      # Cloudflare auth certificate
├── config.yml                    # Tunnel configuration
├── start-tunnel.vbs              # Runs tunnel without window
└── {tunnel-id}.json              # Tunnel credentials
```

### Auto-Start Configuration

Both services start automatically when the Windows user logs in via VBS scripts in the Startup folder:

```
C:\Users\rick\AppData\Roaming\Microsoft\Windows\Start Menu\Programs\Startup\
├── start-bba-server.vbs          # Starts bba-server.exe hidden
└── start-tunnel.vbs              # Starts cloudflared tunnel hidden
```

### Cloudflare Tunnel Details

- **Tunnel Name**: `bba-server`
- **Tunnel ID**: `7d3ab3db-0a42-4422-8017-2542f18a15a2`
- **Hostname**: `bba.harmonicsystems.com`
- **Routes to**: `http://localhost:5000`

**Config file** (`C:\Users\rick\.cloudflared\config.yml`):
```yaml
tunnel: 7d3ab3db-0a42-4422-8017-2542f18a15a2
credentials-file: C:\Users\rick\.cloudflared\7d3ab3db-0a42-4422-8017-2542f18a15a2.json

ingress:
  - hostname: bba.harmonicsystems.com
    service: http://localhost:5000
  - service: http_status:404
```

### Manual Service Control

**On Windows (PowerShell):**

```powershell
# Start cloudflared tunnel
wscript.exe "C:\Users\rick\.cloudflared\start-tunnel.vbs"

# Start BBA server
wscript.exe "G:\BBA-Tools\bba-server\bin\Debug\net8.0-windows\start-hidden.vbs"

# Stop services
taskkill /f /im cloudflared.exe
taskkill /f /im bba-server.exe

# Check if running
tasklist | findstr "cloudflared bba-server"
```

**From Mac (via SSH):**
```bash
# Using the management scripts (recommended)
cd /Users/rick/Development/GitHub/BBA-Tools/bba-server
python3 restart-server.py

# Or direct SSH
ssh rick@10.211.55.5 "tasklist | findstr cloudflared"
```

**Quick Restart via ssh_runner (Python):**
```python
import os, sys
os.environ['WINDOWS_HOST'] = '10.211.55.5'
os.environ['WINDOWS_USER'] = 'Rick'
sys.path.insert(0, '/Volumes/Express2T/Development/GitHub/Practice-Bidding-Scenarios/build-scripts-mac')
from ssh_runner import run_windows_command

# Stop, build, and restart
run_windows_command('taskkill /IM bba-server.exe /F', check=False)
run_windows_command(r'cd /d G:\BBA-Tools\bba-server && dotnet build -c Release -o G:\BBA-Tools\dist\bba-server')
run_windows_command(r'schtasks /Create /TN "StartBBAServer" /TR "G:\BBA-Tools\dist\bba-server\bba-server.exe" /SC ONCE /ST 00:00 /F && schtasks /Run /TN "StartBBAServer"')
```

## Logging

### Application Logs

Log files are in `G:\BBA-Tools\dist\bba-server\logs\`:
- `bba-server-YYYY-MM-DD.log` - Daily log files (30-day retention)

### Audit Logs (CSV)

Audit logs track all API requests (30-day retention):

**Auction Requests** (`audit-auction-YYYY-MM.csv`):
```
Timestamp, RequestIP, ClientVersion, DurationMs, Version, EPBotVersion,
Dealer, Vulnerability, Scoring, NSConvention, EWConvention, Scenario,
PBN, Success, Auction, Alerts, Error
```

**Scenario Selections** (`audit-scenario-YYYY-MM.csv`):
```
Timestamp, RequestIP, ClientVersion, Version, Scenario
```

### IP Anonymization

For privacy, all client IPs are anonymized to friendly names like "Alice_Baker" using a deterministic hash. The same IP always maps to the same name, but the original IP cannot be recovered. This allows tracking usage patterns without storing personal data.

## Convention Card Data Flow

The server reads convention cards from the PBS project:

1. If `scenario` is provided in the request:
   - Read `# convention-card: {name}` from `/dlr/{scenario}.dlr`
   - If not specified, use default: `21GF-DEFAULT`
   - EW always uses: `21GF-GIB`

2. If `conventions` is provided explicitly, use those values

3. Load `.bbsa` files from the configured `BbsaPath`

## Development

### Building

```bash
# From Mac
python3 start-server.py build

# On Windows
cd G:\BBA-Tools\bba-server
dotnet build
```

### Configuration

**appsettings.json:**
```json
{
  "Pbs": {
    "DlrPath": "P:\\dlr",
    "BbsaPath": "G:\\BBA-Tools\\epbot-wrapper"
  },
  "Version": "1.0.0"
}
```

### Project Structure

```
bba-server/
├── Program.cs              # Main entry, endpoints, middleware, admin dashboard HTML
├── Services/
│   ├── EPBotService.cs     # EPBot COM wrapper
│   ├── ConventionService.cs # Convention card lookup
│   ├── AuditLogService.cs  # CSV audit logging
│   ├── AdminService.cs     # Admin dashboard logic and IP whitelist
│   └── IpAnonymizer.cs     # Privacy-preserving IP hashing
├── Models/
│   └── AuctionModels.cs    # Request/response DTOs
├── Logging/
│   └── FileLoggerProvider.cs # File logging
└── bba-server.csproj
```

### Thread Safety

EPBot COM objects are not thread-safe. The server uses a `SemaphoreSlim` to limit concurrent EPBot instances.

### CORS

The server allows requests from:
- `https://www.bridgebase.com`
- `http://www.bridgebase.com`
- `https://bridgebase.com`
- `http://localhost:3000` (development)

## Troubleshooting

### Server not responding

1. Check if VM is running
2. Check SSH connection: `ssh rick@10.211.55.5 "echo ok"`
3. Check if server is running: `python3 start-server.py status`
4. Restart server: `python3 restart-server.py`

### Tunnel not working (error 1033)

1. Check if cloudflared is running on Windows: `tasklist | findstr cloudflared`
2. Start tunnel manually: `wscript.exe "C:\Users\rick\.cloudflared\start-tunnel.vbs"`
3. Check tunnel connections: `cloudflared tunnel list` (should show connections)

### Build fails (file locked)

The server executable is locked while running. Stop it first:
```bash
python3 start-server.py stop
python3 start-server.py build
python3 restart-server.py
```

### Check tunnel connections

```bash
# From Windows
& "C:\Program Files (x86)\cloudflared\cloudflared.exe" tunnel list

# Should show something like:
# ID                                   NAME       CREATED              CONNECTIONS
# 7d3ab3db-0a42-4422-8017-2542f18a15a2 bba-server 2026-01-16T20:31:18Z 2xsjc06, 2xsjc08
```

If CONNECTIONS is empty, the tunnel isn't connected. Restart cloudflared.

## Future Plans

- Migrate to Linux hosting when EPBot gets a Linux port
- This will reduce costs and simplify deployment
- The API interface will remain the same
