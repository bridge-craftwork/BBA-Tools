# EPBot Wrapper

A .NET wrapper for the EPBot bridge bidding DLL, providing a command-line interface for generating bridge auctions.

## Architecture Overview

EPBot uses a **4-instance model** where each player at the table has their own EPBot instance. All instances must be kept in sync by broadcasting each bid to all players.

```
┌─────────┐     ┌─────────┐
│ North   │     │  East   │
│ EPBot   │     │  EPBot  │
└────┬────┘     └────┬────┘
     │               │
     └───────┬───────┘
             │ Bids broadcast to all
     ┌───────┴───────┐
     │               │
┌────┴────┐     ┌────┴────┐
│  West   │     │ South   │
│  EPBot  │     │  EPBot  │
└─────────┘     └─────────┘
```

## High-Level Pseudocode

```
# Initialize 4 EPBot instances (one per seat)
for position in [North, East, South, West]:
    players[position] = new EPBot()

    # Load conventions for BOTH partnerships into each bot
    players[position].set_conventions(NS_SIDE, ns_convention_file)
    players[position].set_conventions(EW_SIDE, ew_convention_file)

    # Initialize the hand for this position
    # NOTE: EPBot expects suits in C.D.H.S order, not PBN's S.H.D.C order
    hand = reverse(pbn_hand)  # Convert S.H.D.C -> C.D.H.S
    players[position].new_hand(position, hand, dealer, vulnerability, false, false)

# Run the auction
current_position = dealer
pass_count = 0
has_bid = false

while auction_not_ended:
    # Get bid from current player
    bid_code = players[current_position].get_bid()

    # Broadcast this bid to ALL players
    for each player in players:
        player.set_bid(current_position, bid_code, alert_string)

    # Track passes for auction termination
    if bid_code == PASS:
        pass_count++
    else:
        pass_count = 0
        has_bid = true

    # Auction ends: 3 passes after a bid, or 4 initial passes
    if (has_bid and pass_count >= 3) or (not has_bid and pass_count >= 4):
        break

    current_position = next_position(current_position)
```

## Key EPBot API Methods

| Method | Description |
|--------|-------------|
| `new_hand(position, hand[], dealer, vul, ?, ?)` | Initialize a player with their hand |
| `get_bid()` | Get the bid for the current player |
| `set_bid(position, bid_code, alert)` | Broadcast a bid to this EPBot instance |
| `get_hand(position)` | Get the hand at a position (for debugging) |
| `get_str_bidding()` | Get the auction so far as a string (for debugging) |
| `set_conventions(side, key, value)` | Set a convention for a side (0=NS, 1=EW) |
| `get_conventions(side, key)` | Get a convention value |
| `set_system_type(side, type)` | Set the bidding system type |

## Bid Code Encoding

EPBot uses integer codes for bids:

| Code | Bid |
|------|-----|
| 0 | Pass |
| 1 | Double (X) |
| 2 | Redouble (XX) |
| 5-9 | 1C, 1D, 1H, 1S, 1NT |
| 10-14 | 2C, 2D, 2H, 2S, 2NT |
| 15-19 | 3C, 3D, 3H, 3S, 3NT |
| ... | ... |
| 35-39 | 7C, 7D, 7H, 7S, 7NT |

Formula: `code = 5 + (level - 1) * 5 + suit_index` where suit_index is C=0, D=1, H=2, S=3, NT=4

## Suit Order Conversion

PBN format uses **S.H.D.C** order (Spades first), but EPBot expects **C.D.H.S** order (Clubs first).

```
PBN:   "AK32.QJ5.T98.764"  →  Spades=AK32, Hearts=QJ5, Diamonds=T98, Clubs=764
EPBot: ["764", "T98", "QJ5", "AK32"]  →  Clubs, Diamonds, Hearts, Spades
```

The conversion is simply reversing the suit array.

## Position Encoding

| Position | Code |
|----------|------|
| North | 0 |
| East | 1 |
| South | 2 |
| West | 3 |

## Vulnerability Encoding

Per [EPBot docs](https://sites.google.com/view/bbaenglish/for-programmers):

| Vulnerability | Code |
|---------------|------|
| None (both before) | 0 |
| EW vulnerable | 1 |
| NS vulnerable | 2 |
| Both vulnerable | 3 |

## Convention Files (.bbsa)

Convention files are key-value pairs:

```
System type = 3
1NT opening range = 15-17
Transfers if RHO passes = true
Jacoby 2NT = true
```

Each convention is loaded with `set_conventions(side, key, value)`.

## Usage

### Batch Mode (JSON input/output)

```bash
cat input.json | epbot-wrapper.exe > output.json
```

Input format:
```json
{
  "ns_conventions": "path/to/ns.bbsa",
  "ew_conventions": "path/to/ew.bbsa",
  "deals": [
    {
      "pbn": "N:AK32.QJ5.T98.764 ...",
      "dealer": "S",
      "vulnerability": "NS"
    }
  ]
}
```

Output format:
```json
{
  "results": [
    {
      "deal": "N:AK32.QJ5.T98.764 ...",
      "auction": ["1NT", "Pass", "2C", "Pass", "2H", "Pass", "Pass", "Pass"],
      "success": true,
      "error": null
    }
  ]
}
```

### Interactive Mode

```bash
epbot-wrapper.exe --interactive
```

### Test Mode

```bash
epbot-wrapper.exe --test
```

## Requirements

- .NET Framework 4.8 (required for EPBot DLL compatibility)
- EPBot64.dll or EPBotARM64.dll in the same directory
- Windows (EPBot is Windows-only)

## Running from Mac via SSH

Since EPBot is Windows-only, the wrapper runs on a Windows VM (Parallels) and is invoked from Mac via SSH.

### Direct SSH

```bash
ssh rick@10.211.55.5 "net use G: \\\\Mac\\Home\\Development\\GitHub >nul 2>&1 & G:\\BBA-Tools\\epbot-wrapper\\bin\\Debug\\net48\\epbot-wrapper.exe --test"
```

### Using ssh_runner.py

The `ssh_runner.py` script in Practice-Bidding-Scenarios provides a cleaner Python API:

```python
import os
os.environ['WINDOWS_HOST'] = '10.211.55.5'
os.environ['WINDOWS_USER'] = 'Rick'

import sys
sys.path.insert(0, '/Volumes/Express2T/Development/GitHub/Practice-Bidding-Scenarios/build-scripts-mac')
from ssh_runner import run_windows_command, mac_to_windows_path

# Run epbot-wrapper
returncode, stdout, stderr = run_windows_command(
    r'G:\BBA-Tools\epbot-wrapper\bin\Debug\net48\epbot-wrapper.exe --test',
    check=False
)
print(stdout)
```

Benefits of `ssh_runner.py`:
- Automatic drive mapping before each command (`net use G:`, `net use P:`, etc.)
- Path conversion helpers (`mac_to_windows_path`, `windows_to_mac_path`)
- Cleaner API for scripting and integration into build pipelines

For ad-hoc testing, direct SSH works fine. For integrating into a Python pipeline (like the Practice-Bidding-Scenarios build scripts), using `ssh_runner.py` is cleaner.
