#!/usr/bin/env python3
"""
Test script to compare BBA server auctions against reference PBN files.

Usage:
    python test-scenario.py Smolen 1-3,5
    python test-scenario.py Smolen 1-10
    python test-scenario.py Smolen all
"""
import sys
import os
import re
import json
import subprocess
import argparse
from dataclasses import dataclass
from typing import Optional

# Configuration
PBS_ROOT = os.path.expanduser("/Volumes/Express2T/Development/GitHub/Practice-Bidding-Scenarios")
BBA_PATH = os.path.join(PBS_ROOT, "bba")
SERVER_URL = "http://10.211.55.5:5000"


@dataclass
class Deal:
    """Parsed deal from PBN file."""
    board: int
    dealer: str
    vulnerability: str
    pbn: str
    scoring: str
    auction: list[str]
    notes: dict[str, str]


def parse_deal_list(spec: str, max_deals: int) -> list[int]:
    """
    Parse deal specification like "1-3,5,7-9" into a list of board numbers.
    "all" returns all boards.
    """
    if spec.lower() == "all":
        return list(range(1, max_deals + 1))

    result = []
    for part in spec.split(","):
        part = part.strip()
        if "-" in part:
            start, end = part.split("-", 1)
            result.extend(range(int(start), int(end) + 1))
        else:
            result.append(int(part))

    return sorted(set(result))


def parse_pbn_file(filepath: str) -> list[Deal]:
    """Parse a PBN file and extract all deals."""
    deals = []

    with open(filepath, "r") as f:
        content = f.read()

    # Split into deal blocks (each starts with [Event)
    blocks = re.split(r'\n(?=\[Event )', content)

    for block in blocks:
        if not block.strip() or block.startswith("%"):
            continue

        # Extract tags
        tags = {}
        for match in re.finditer(r'\[(\w+)\s+"([^"]*)"\]', block):
            tags[match.group(1)] = match.group(2)

        if "Board" not in tags or "Deal" not in tags:
            continue

        # Extract auction
        auction_match = re.search(r'\[Auction "([^"]+)"\]\n(.*?)(?:\n\[|\Z)', block, re.DOTALL)
        auction = []
        if auction_match:
            auction_text = auction_match.group(2)
            # Parse auction - remove notes like =1= and handle multi-line
            auction_lines = auction_text.strip().split("\n")
            for line in auction_lines:
                if line.startswith("[") or line.startswith("*"):
                    break
                # Remove note markers like =1=
                line = re.sub(r'=\d+=', '', line)
                # Split into bids
                bids = line.split()
                auction.extend(bids)

        # Extract notes
        notes = {}
        for match in re.finditer(r'\[Note "(\d+):([^"]*)"\]', block):
            notes[match.group(1)] = match.group(2)

        deals.append(Deal(
            board=int(tags.get("Board", 0)),
            dealer=tags.get("Dealer", "N"),
            vulnerability=tags.get("Vulnerable", "None"),
            pbn=tags.get("Deal", ""),
            scoring=tags.get("Scoring", "MP"),
            auction=auction,
            notes=notes,
        ))

    return deals


def format_auction(bids: list[str]) -> str:
    """Format auction for display, replacing trailing passes with AllPass."""
    if not bids:
        return "(none)"

    # Check for passed out
    if len(bids) == 4 and all(b.lower() == "pass" for b in bids):
        return "PassOut"

    # Replace trailing 3 passes with AllPass
    if len(bids) >= 4 and all(b.lower() == "pass" for b in bids[-3:]):
        return " ".join(bids[:-3]) + " AllPass"

    return " ".join(bids)


def normalize_auction(bids: list[str]) -> list[str]:
    """Normalize auction for comparison."""
    result = []
    for bid in bids:
        bid = bid.upper().strip()
        # Normalize pass variations
        if bid in ("PASS", "P", "--"):
            result.append("PASS")
        # Normalize double
        elif bid in ("X", "DBL", "DOUBLE", "DB"):
            result.append("X")
        # Normalize redouble
        elif bid in ("XX", "RDBL", "REDOUBLE", "RD"):
            result.append("XX")
        # Normalize NT variations (1N -> 1NT, 2N -> 2NT, etc.)
        elif len(bid) == 2 and bid[0].isdigit() and bid[1] == "N":
            result.append(bid[0] + "NT")
        else:
            result.append(bid)
    return result


def call_server(deal: Deal, scenario: str) -> tuple[bool, list[str], str]:
    """
    Call the BBA server and return (success, auction, error).
    """
    request = {
        "deal": {
            "pbn": deal.pbn,
            "dealer": deal.dealer,
            "vulnerability": deal.vulnerability,
            "scoring": deal.scoring,
        },
        "scenario": scenario,
    }

    try:
        result = subprocess.run(
            [
                "curl", "-s", "-X", "POST",
                "-H", "Content-Type: application/json",
                "-d", json.dumps(request),
                f"{SERVER_URL}/api/auction/generate",
            ],
            capture_output=True,
            text=True,
            timeout=30,
        )

        if result.returncode != 0:
            return False, [], f"curl failed: {result.stderr}"

        response = json.loads(result.stdout)
        if response.get("success"):
            return True, response.get("auction", []), ""
        else:
            return False, [], response.get("error", "Unknown error")

    except subprocess.TimeoutExpired:
        return False, [], "Request timeout"
    except json.JSONDecodeError as e:
        return False, [], f"Invalid JSON response: {e}"
    except Exception as e:
        return False, [], str(e)


def truncate_pbn(pbn: str, max_len: int = 40) -> str:
    """Truncate PBN for display."""
    # Extract just the hands part (after the colon)
    if ":" in pbn:
        hands = pbn.split(":", 1)[1]
    else:
        hands = pbn

    if len(hands) > max_len:
        return hands[: max_len - 3] + "..."
    return hands


def main():
    parser = argparse.ArgumentParser(
        description="Test BBA server auctions against reference PBN files"
    )
    parser.add_argument("scenario", help="Scenario name (e.g., Smolen)")
    parser.add_argument(
        "deals",
        nargs="?",
        default="all",
        help="Deal specification: '1-3,5' or 'all' (default: all)",
    )
    parser.add_argument(
        "--verbose", "-v", action="store_true", help="Show detailed output"
    )

    args = parser.parse_args()

    # Find PBN file
    pbn_file = os.path.join(BBA_PATH, f"{args.scenario}.pbn")
    if not os.path.exists(pbn_file):
        print(f"ERROR: PBN file not found: {pbn_file}")
        sys.exit(1)

    # Parse PBN file
    print(f"Reading {pbn_file}...")
    all_deals = parse_pbn_file(pbn_file)
    print(f"Found {len(all_deals)} deals in file")

    if not all_deals:
        print("ERROR: No deals found in PBN file")
        sys.exit(1)

    # Parse deal specification
    deal_numbers = parse_deal_list(args.deals, len(all_deals))
    deals_to_test = [d for d in all_deals if d.board in deal_numbers]

    if not deals_to_test:
        print(f"ERROR: No deals match specification '{args.deals}'")
        sys.exit(1)

    print(f"Testing {len(deals_to_test)} deals: {args.deals}")
    print()

    # Test each deal
    results = []
    matches = 0
    mismatches = 0
    errors = 0

    for deal in deals_to_test:
        success, server_auction, error = call_server(deal, args.scenario)

        if not success:
            results.append({
                "board": deal.board,
                "pbn": deal.pbn,
                "reference": deal.auction,
                "server": [],
                "match": None,
                "error": error,
            })
            errors += 1
            if args.verbose:
                print(f"  Board {deal.board}: ERROR - {error}")
        else:
            # Compare auctions
            ref_normalized = normalize_auction(deal.auction)
            server_normalized = normalize_auction(server_auction)
            is_match = ref_normalized == server_normalized

            results.append({
                "board": deal.board,
                "pbn": deal.pbn,
                "reference": deal.auction,
                "server": server_auction,
                "match": is_match,
                "error": None,
            })

            if is_match:
                matches += 1
                if args.verbose:
                    print(f"  Board {deal.board}: MATCH")
            else:
                mismatches += 1
                if args.verbose:
                    print(f"  Board {deal.board}: MISMATCH")
                    print(f"    Reference: {format_auction(deal.auction)}")
                    print(f"    Server:    {format_auction(server_auction)}")

    # Print summary
    print()
    print("=" * 100)
    print(f"RESULTS: {args.scenario} - {len(deals_to_test)} deals tested")
    print("=" * 100)
    print()

    total = matches + mismatches + errors
    print(f"  Matches:    {matches:3d} ({100*matches/total:.1f}%)")
    print(f"  Mismatches: {mismatches:3d} ({100*mismatches/total:.1f}%)")
    print(f"  Errors:     {errors:3d} ({100*errors/total:.1f}%)")
    print()

    # Print detailed table
    print("-" * 100)
    print(f"{'Board':>5} | {'Hands':<42} | {'Reference':<20} | {'Server':<20} | Result")
    print("-" * 100)

    for r in results:
        hands = truncate_pbn(r["pbn"], 40)
        ref_auction = format_auction(r["reference"])[:20]
        server_auction = format_auction(r["server"])[:20] if r["server"] else "(error)"

        if r["error"]:
            result_str = f"ERROR: {r['error'][:15]}"
        elif r["match"]:
            result_str = "✓ Match"
        else:
            result_str = "✗ MISMATCH"

        print(f"{r['board']:>5} | {hands:<42} | {ref_auction:<20} | {server_auction:<20} | {result_str}")

    print("-" * 100)

    # Print mismatches in detail
    if mismatches > 0:
        print()
        print("MISMATCHES IN DETAIL:")
        print("=" * 100)
        for r in results:
            if r["match"] is False:
                print(f"\nBoard {r['board']}:")
                print(f"  Deal: {r['pbn']}")
                print(f"  Reference: {format_auction(r['reference'])}")
                print(f"  Server:    {format_auction(r['server'])}")

    # Exit with appropriate code
    if mismatches > 0 or errors > 0:
        sys.exit(1)
    sys.exit(0)


if __name__ == "__main__":
    main()
