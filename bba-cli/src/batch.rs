//! Batch processor for PBN files.
//!
//! Reads a PBN file, generates an auction for every deal with epbot-core, and
//! writes the result by editing the input in place: bba-cli sets the tags it
//! owns — the auction and what is derived from it — and leaves every other
//! byte of each board as written, so supplemental tags such as `HandType` or
//! `OptimumResultTable`, commentary and `%` directives survive the bba step.

use anyhow::{Context, Result};
use bridge_encodings::pbn::{prevailing_newline, split_lines, PbnDocument};
use bridge_parsers::{Board, Deal, Direction};
use epbot_core::bba_hash::{self, HandSuits};
use epbot_core::score::{self, Strain};
use epbot_core::{generate_auction_with_options, ConventionCard, Scoring};
use log::{debug, error, info, warn};
use std::path::Path;

/// Statistics from batch processing
#[derive(Debug, Default)]
pub struct ProcessingStats {
    pub deals_processed: usize,
    pub auctions_generated: usize,
    pub errors: usize,
}

/// Configuration for PBN output formatting
pub struct OutputConfig {
    pub event: String,
    /// `None` means "ask EPBot what the .bbsa's `System type` is called";
    /// `Some` is an explicit CLI override.
    pub ns_system_name: Option<String>,
    pub ew_system_name: Option<String>,
    pub ns_conventions_path: String,
    pub ew_conventions_path: String,
    pub scoring: Scoring,
    pub single_dummy: bool,
    /// Note every call's meaning (short | extended), not only alerts.
    pub all_meanings: bool,
}

fn direction_to_int(dir: Direction) -> i32 {
    match dir {
        Direction::North => 0,
        Direction::East => 1,
        Direction::South => 2,
        Direction::West => 3,
    }
}

fn direction_char(dir: Direction) -> &'static str {
    match dir {
        Direction::North => "N",
        Direction::East => "E",
        Direction::South => "S",
        Direction::West => "W",
    }
}

fn int_to_direction_char(pos: i32) -> &'static str {
    match pos % 4 {
        0 => "N",
        1 => "E",
        2 => "S",
        3 => "W",
        _ => "?",
    }
}

/// Parse vulnerability to EPBot integer.
/// EPBot convention: 0=None, 1=EW, 2=NS, 3=Both
fn vulnerability_to_epbot(vul: &bridge_parsers::Vulnerability) -> i32 {
    match vul {
        bridge_parsers::Vulnerability::None => 0,
        bridge_parsers::Vulnerability::EastWest => 1,
        bridge_parsers::Vulnerability::NorthSouth => 2,
        bridge_parsers::Vulnerability::Both => 3,
    }
}

fn vulnerability_to_pbn(vul: i32) -> &'static str {
    match vul {
        0 => "None",
        1 => "EW",
        2 => "NS",
        3 => "All",
        _ => "None",
    }
}

fn direction_str_to_int(s: &str) -> i32 {
    match s {
        "N" => 0,
        "E" => 1,
        "S" => 2,
        "W" => 3,
        _ => 0,
    }
}

fn strain_index(strain: Strain) -> usize {
    match strain {
        Strain::Clubs => 0,
        Strain::Diamonds => 1,
        Strain::Hearts => 2,
        Strain::Spades => 3,
        Strain::NoTrump => 4,
    }
}

fn scoring_tag(scoring: Scoring) -> &'static str {
    match scoring {
        Scoring::Matchpoints => "MP",
        Scoring::Imps => "IMP",
    }
}

/// Build the per-player suit strings expected by `bba_hash::encode`.
///
/// Cards within each suit are listed using `RANKS` order (A,K,Q,J,T,9..2),
/// using 'T' for the ten — matches the convention `bba_hash` searches by.
fn hands_for_bba_hash(deal: &Deal) -> [HandSuits; 4] {
    let dirs = [Direction::North, Direction::East, Direction::South, Direction::West];
    let mut out: [HandSuits; 4] = Default::default();
    for (i, &dir) in dirs.iter().enumerate() {
        let h = deal.hand(dir);
        out[i] = HandSuits {
            clubs:    suit_string_with_t(h, bridge_parsers::Suit::Clubs),
            diamonds: suit_string_with_t(h, bridge_parsers::Suit::Diamonds),
            hearts:   suit_string_with_t(h, bridge_parsers::Suit::Hearts),
            spades:   suit_string_with_t(h, bridge_parsers::Suit::Spades),
        };
    }
    out
}

fn suit_string_with_t(hand: &bridge_parsers::Hand, suit: bridge_parsers::Suit) -> String {
    hand.cards_in_suit(suit)
        .iter()
        .map(|c| {
            let r = format!("{}", c.rank);
            // bridge_parsers may render the ten as "10"; bba_hash expects 'T'.
            if r == "10" { "T".to_string() } else { r }
        })
        .collect()
}

/// Format a Deal as a PBN deal string: "N:S.H.D.C S.H.D.C S.H.D.C S.H.D.C"
fn format_deal_pbn(deal: &Deal) -> String {
    let dirs = [
        Direction::North,
        Direction::East,
        Direction::South,
        Direction::West,
    ];
    let hands: Vec<String> = dirs
        .iter()
        .map(|&dir| {
            let hand = deal.hand(dir);
            let suits: Vec<String> = [
                bridge_parsers::Suit::Spades,
                bridge_parsers::Suit::Hearts,
                bridge_parsers::Suit::Diamonds,
                bridge_parsers::Suit::Clubs,
            ]
            .iter()
            .map(|&suit| {
                let cards = hand.cards_in_suit(suit);
                cards
                    .iter()
                    .map(|c| format!("{}", c.rank))
                    .collect::<String>()
            })
            .collect();
            suits.join(".")
        })
        .collect();
    format!("N:{}", hands.join(" "))
}

/// Process a PBN file, generating auctions for each deal.
///
/// `auction_prefix`, if provided, forces the first N bids of every auction
/// before EPBot resumes normal bidding. Mirrors the bba-server `auctionPrefix`
/// field so the CLI and server stay interchangeable for A/B testing.
pub fn process_pbn_file(
    input_path: &Path,
    output_path: &Path,
    ns_conventions: &Path,
    ew_conventions: &Path,
    dry_run: bool,
    config: &OutputConfig,
    auction_prefix: Option<&[String]>,
) -> Result<ProcessingStats> {
    let mut stats = ProcessingStats::default();

    info!("Reading PBN file: {:?}", input_path);
    let input = std::fs::read_to_string(input_path).context("Failed to read PBN file")?;
    let newline = prevailing_newline(&input);
    // A previous run's header is replaced, not stacked under a new one.
    let mut doc =
        PbnDocument::parse(&strip_owned_header(&input)).context("Failed to parse PBN file")?;
    // Owned copies: the document is edited below while these are still read.
    let boards: Vec<Board> = doc.boards().to_vec();
    info!("Found {} games in input file", boards.len());

    // Load convention cards
    let ns_content = std::fs::read_to_string(ns_conventions)
        .context("Failed to read NS conventions file")?;
    let ew_content = std::fs::read_to_string(ew_conventions)
        .context("Failed to read EW conventions file")?;
    let ns_card = ConventionCard::from_content(&ns_content);
    let ew_card = ConventionCard::from_content(&ew_content);

    // Resolve the BidSystem tag text once, before any deal is bid.
    let system_names = SystemNames {
        ns: resolve_system_name(config.ns_system_name.as_deref(), &ns_card, 0, "N-S"),
        ew: resolve_system_name(config.ew_system_name.as_deref(), &ew_card, 1, "E-W"),
    };

    // Process each deal
    let mut results = Vec::new();

    for (idx, board) in boards.iter().enumerate() {
        let dealer = board.dealer.unwrap_or(Direction::North);
        let vul = vulnerability_to_epbot(&board.vulnerable);
        let deal_str = format_deal_pbn(&board.deal);

        stats.deals_processed += 1;

        let result = generate_auction_with_options(
            &deal_str,
            direction_to_int(dealer),
            vul,
            config.scoring,
            Some(&ns_card),
            Some(&ew_card),
            auction_prefix,
            config.single_dummy,
            config.all_meanings,
        );

        if result.success {
            stats.auctions_generated += 1;
        } else {
            stats.errors += 1;
            if let Some(ref err) = result.error {
                error!("Game {}: {}", idx + 1, err);
            }
        }

        results.push(result);
    }

    if !dry_run {
        info!("Writing output to {:?}", output_path);
        for (idx, (board, result)) in boards.iter().zip(&results).enumerate() {
            annotate_board(&mut doc, idx, board, result, config, &system_names)
                .with_context(|| format!("Failed to write game {}", idx + 1))?;
        }
        let output = format!("{}{}", owned_header(config, newline), doc.to_pbn());
        std::fs::write(output_path, output).context("Failed to write output PBN file")?;
    }

    Ok(stats)
}

/// Resolved `[BidSystemNS]`/`[BidSystemEW]` tag text for one run.
struct SystemNames {
    ns: String,
    ew: String,
}

/// The BidSystem tag text for one side: an explicit CLI override when given,
/// otherwise whatever EPBot calls the card's `System type`.
///
/// Before this was derived, both tags were hardcoded to "2/1GF - 2/1 Game
/// Force", which mislabelled every SAYC, Polish Club, Precision and Acol card
/// (issue #3).
fn resolve_system_name(
    cli_override: Option<&str>,
    card: &ConventionCard,
    side: i32,
    label: &str,
) -> String {
    if let Some(name) = cli_override {
        return name.to_string();
    }
    match card.system_name(side) {
        Ok(name) => {
            info!("{} bidding system: {}", label, name);
            name
        }
        Err(e) => {
            // Only reachable if EPBot itself is unusable, in which case the
            // auctions fail too and the run exits non-zero. Emit no tag rather
            // than an invented one.
            warn!(
                "Could not read the {} system name from EPBot ({}); omitting the BidSystem tag",
                label, e
            );
            String::new()
        }
    }
}

/// The first lines of every output file. They describe the run, not the deals,
/// so [`strip_owned_header`] removes a previous run's copy before these are
/// written.
fn owned_header(config: &OutputConfig, newline: &str) -> String {
    let mut lines = vec![
        "% PBN 2.1".to_string(),
        "% Generated by bba-cli".to_string(),
    ];
    if !config.ns_conventions_path.is_empty() {
        lines.push(format!("% CC1 - {}", config.ns_conventions_path));
    }
    if !config.ew_conventions_path.is_empty() {
        lines.push(format!("% CC2 - {}", config.ew_conventions_path));
    }
    lines
        .iter()
        .map(|line| format!("{line}{newline}"))
        .collect()
}

/// Whether a directive is one of the header lines bba-cli writes.
fn is_owned_header_line(line: &str) -> bool {
    let line = line.trim_end();
    line.starts_with("% PBN ")
        || line == "% Generated by bba-cli"
        || line.starts_with("% CC1 - ")
        || line.starts_with("% CC2 - ")
}

/// `text` without bba-cli's own header lines.
///
/// Only the run of `%` directives at the very top of the file is examined, and
/// only bba-cli's lines are dropped from it: the input's own directives —
/// `%HRTitleEvent`, a Bridge Composer header — are kept, in order. The header
/// belongs to no board, so it is the one part of the output handled as text
/// rather than through [`PbnDocument`].
fn strip_owned_header(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut in_header = true;
    for (content, term) in split_lines(text) {
        in_header = in_header && content.starts_with('%');
        if in_header && is_owned_header_line(content) {
            continue;
        }
        out.push_str(content);
        out.push_str(term);
    }
    out
}

/// BBA's board fingerprint: a directive of exactly 28 hex digits.
fn is_bba_hash(text: &str) -> bool {
    text.len() == 28 && text.chars().all(|c| c.is_ascii_hexdigit())
}

/// Tags derived from the auction. A board whose auction could not be generated
/// loses all of them, so re-bidding a file never leaves a previous run's
/// auction standing under a board that failed this time.
const AUCTION_TAGS: [&str; 5] = ["Declarer", "Contract", "Result", "Score", "Auction"];

/// Write one board's auction and everything bba-cli derives from it into the
/// document, leaving the rest of the board as the input had it.
///
/// Owned, and set or replaced: the four players (`EPBot`), the `{Shape}`,
/// `{HCP}` and `{Losers}` commentary, `Declarer`, `Contract`, `Auction` with
/// its `Note`s, and `BidSystemNS`/`BidSystemEW`. `Result` and `Score` too:
/// they describe the contract, so they are set with `--single-dummy` and
/// removed otherwise — a stale result for a replaced contract would be wrong.
/// With `--single-dummy`, also `Scoring` and the board-id hash directive.
///
/// Filled in only when missing: `Event` (always set when `--event` is given),
/// `Site`, `Board`, `Dealer` and `Vulnerable`. `Date` is never written; the
/// input's is kept.
fn annotate_board(
    doc: &mut PbnDocument,
    idx: usize,
    board: &Board,
    result: &epbot_core::AuctionResult,
    config: &OutputConfig,
    system_names: &SystemNames,
) -> Result<()> {
    let dealer = board.dealer.unwrap_or(Direction::North);
    let vul = vulnerability_to_epbot(&board.vulnerable);
    let board_num = board.number.unwrap_or((idx + 1) as u32);

    if !config.event.is_empty() {
        doc.set_tag(idx, "Event", &config.event)?;
    }
    set_if_missing(doc, idx, "Event", "")?;
    set_if_missing(doc, idx, "Site", "")?;
    set_if_missing(doc, idx, "Board", &board_num.to_string())?;
    // A dealer the reader could not parse was bid as North; say so.
    if board.dealer.is_none() {
        doc.set_tag(idx, "Dealer", direction_char(dealer))?;
    }
    set_if_missing(doc, idx, "Vulnerable", vulnerability_to_pbn(vul))?;

    if config.single_dummy {
        let hash = bba_hash::encode(
            &hands_for_bba_hash(&board.deal),
            direction_to_int(dealer) as u8,
            vul as u8,
            bba_hash::board_extension_for(board_num),
        );
        doc.set_directive(idx, "Board", &hash, is_bba_hash)?;
    }

    for seat in ["North", "East", "South", "West"] {
        doc.set_tag(idx, seat, "EPBot")?;
    }

    let (shape, hcp, losers) = hand_analysis(&board.deal);
    doc.set_comment(idx, "Deal", "Shape", &shape)?;
    doc.set_comment(idx, "Deal", "HCP", &hcp)?;
    doc.set_comment(idx, "Deal", "Losers", &losers)?;

    if result.success && !result.bids.is_empty() {
        let bid_strs: Vec<&str> = result.bids.iter().map(|b| b.bid.as_str()).collect();
        let (contract, declarer) = derive_contract_declarer(&bid_strs, direction_to_int(dealer));
        doc.set_tag(idx, "Declarer", &declarer)?;
        doc.set_tag(idx, "Contract", &contract)?;

        let mut outcome = None;
        if config.single_dummy {
            if let (Some(analysis), Some((level, strain, doubled))) =
                (result.analysis.as_ref(), score::parse_contract(&contract))
            {
                let tricks = analysis.tricks[strain_index(strain)];
                let ns_score = score::score_for_ns(
                    level,
                    strain,
                    doubled,
                    direction_str_to_int(&declarer) as u8,
                    vul as u8,
                    tricks,
                );
                outcome = Some((tricks, ns_score));
            }
            doc.set_tag(idx, "Scoring", scoring_tag(config.scoring))?;
        }
        match outcome {
            Some((tricks, ns_score)) => {
                doc.set_tag(idx, "Result", &tricks.to_string())?;
                doc.set_tag(idx, "Score", &format!("NS {ns_score}"))?;
            }
            None => {
                doc.remove_tag(idx, "Result")?;
                doc.remove_tag(idx, "Score")?;
            }
        }

        let (rows, notes) = annotated_auction(&result.bids, config.all_meanings);
        let rows: Vec<&str> = rows.iter().map(String::as_str).collect();
        let notes: Vec<&str> = notes.iter().map(String::as_str).collect();
        doc.set_section(idx, "Auction", direction_char(dealer), &rows)?;
        doc.set_tags(idx, "Note", &notes)?;
    } else {
        for tag in AUCTION_TAGS {
            doc.remove_tag(idx, tag)?;
        }
        doc.set_tags(idx, "Note", &[])?;
    }

    for (tag, name) in [
        ("BidSystemEW", &system_names.ew),
        ("BidSystemNS", &system_names.ns),
    ] {
        if name.is_empty() {
            doc.remove_tag(idx, tag)?;
        } else {
            doc.set_tag(idx, tag, name)?;
        }
    }

    debug!("Game {}: written", idx + 1);
    Ok(())
}

/// Set a tag only if the board does not already carry it.
fn set_if_missing(doc: &mut PbnDocument, idx: usize, name: &str, value: &str) -> Result<()> {
    if doc.tag(idx, name).is_none() {
        doc.set_tag(idx, name, value)?;
    }
    Ok(())
}

/// The text of the `{Shape}`, `{HCP}` and `{Losers}` comments, each listing
/// North, East, South, West.
fn hand_analysis(deal: &Deal) -> (String, String, String) {
    let dirs = [
        Direction::North,
        Direction::East,
        Direction::South,
        Direction::West,
    ];
    let shape = dirs
        .iter()
        .map(|&dir| {
            let l = deal.hand(dir).suit_lengths();
            format!("{}{}{}{}", l[0], l[1], l[2], l[3])
        })
        .collect::<Vec<_>>()
        .join(" ");
    let hcp = dirs
        .iter()
        .map(|&dir| deal.hand(dir).hcp().to_string())
        .collect::<Vec<_>>()
        .join(" ");
    let losers = dirs
        .iter()
        .map(|&dir| deal.hand(dir).losers().to_string())
        .collect::<Vec<_>>()
        .join(" ");
    (shape, hcp, losers)
}

/// Derive contract and declarer from auction bids
fn derive_contract_declarer(bids: &[&str], dealer: i32) -> (String, String) {
    let mut last_contract_bid = None;
    let mut last_contract_idx = 0;
    let mut doubled = false;
    let mut redoubled = false;

    for (i, bid) in bids.iter().enumerate() {
        match *bid {
            "Pass" | "P" => {}
            "X" => {
                doubled = true;
                redoubled = false;
            }
            "XX" => {
                redoubled = true;
                doubled = false;
            }
            _ => {
                last_contract_bid = Some(*bid);
                last_contract_idx = i;
                doubled = false;
                redoubled = false;
            }
        }
    }

    let contract_bid = match last_contract_bid {
        Some(bid) => bid,
        None => return ("Pass".to_string(), "?".to_string()),
    };

    let raw = if redoubled {
        format!("{}XX", contract_bid)
    } else if doubled {
        format!("{}X", contract_bid)
    } else {
        contract_bid.to_string()
    };
    // Emit "3N" rather than "3NT" to match the legacy bba-cli-mac output
    // that David's filter pipeline parses. Strain matching below still uses
    // contract_bid ("NT" form) against the unmodified auction bids.
    let contract = raw.replace("NT", "N");

    let declaring_pos = (dealer + last_contract_idx as i32) % 4;
    let declaring_side_is_ns = declaring_pos == 0 || declaring_pos == 2;
    let strain = &contract_bid[1..];

    let mut declarer = declaring_pos;
    for (i, bid) in bids.iter().enumerate() {
        let bidder = (dealer + i as i32) % 4;
        let bidder_is_ns = bidder == 0 || bidder == 2;
        if bidder_is_ns != declaring_side_is_ns {
            continue;
        }
        if bid.len() > 1 && &bid[1..] == strain {
            declarer = bidder;
            break;
        }
    }

    (contract, int_to_direction_char(declarer).to_string())
}

/// The auction's call rows, column-aligned with `=N=` note references, and the
/// `Note` tag values explaining them.
fn annotated_auction(
    bids: &[epbot_core::BidInfo],
    all_meanings: bool,
) -> (Vec<String>, Vec<String>) {
    let mut notes: Vec<String> = Vec::new();
    let mut entries: Vec<String> = Vec::new();

    for bid in bids {
        // PBN auction uses "1N"/"3N" rather than "1NT"/"3NT" — matches
        // legacy bba-cli-mac. See note in derive_contract_declarer.
        let bid_str = bid.bid.replace("NT", "N");
        let meaning = if all_meanings {
            [bid.meaning.as_deref(), bid.meaning_extended.as_deref()]
                .into_iter()
                .flatten()
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .collect::<Vec<_>>()
                .join(" | ")
        } else {
            bid.meaning.as_deref().unwrap_or("").to_string()
        };
        if !meaning.is_empty() {
            let note_num = notes.len() + 1;
            notes.push(format!("{}:{}", note_num, note_text(&meaning)));
            entries.push(format!("{} ={}=", bid_str, note_num));
        } else {
            entries.push(bid_str);
        }
    }

    let rows = entries
        .chunks(4)
        .map(|chunk| {
            let mut line = String::new();
            for (j, entry) in chunk.iter().enumerate() {
                let is_last = j == chunk.len() - 1;
                if is_last {
                    line.push_str(entry);
                } else {
                    let width = if entry.len() <= 4 { 6 } else { entry.len() + 4 };
                    line.push_str(&format!("{:<width$}", entry, width = width));
                }
            }
            line.trim_end().to_string()
        })
        .collect();

    (rows, notes)
}

/// A meaning made safe for a one-line quoted tag value. None of 173,206 notes
/// in Practice-Bidding-Scenarios' bba output needed this, but EPBot text is
/// not under our control, and one bad meaning should not fail a whole file.
fn note_text(meaning: &str) -> String {
    meaning.replace('"', "'").replace(['\r', '\n'], " ")
}
