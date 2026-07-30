//! Batch processor for PBN files.
//!
//! Reads PBN files using bridge-parsers, generates auctions using epbot-core,
//! and writes rich PBN output matching BBA.exe format.

use anyhow::{Context, Result};
use bridge_parsers::pbn::reader::read_pbn_file as bp_read_pbn;
use bridge_parsers::{Board, Deal, Direction};
use epbot_core::bba_hash::{self, HandSuits};
use epbot_core::score::{self, Strain};
use epbot_core::{generate_auction_with_options, ConventionCard, Scoring};
use log::{debug, error, info, warn};
use std::io::{BufWriter, Write};
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
            clubs:    suit_string_with_t(&h, bridge_parsers::Suit::Clubs),
            diamonds: suit_string_with_t(&h, bridge_parsers::Suit::Diamonds),
            hearts:   suit_string_with_t(&h, bridge_parsers::Suit::Hearts),
            spades:   suit_string_with_t(&h, bridge_parsers::Suit::Spades),
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
    let boards = bp_read_pbn(input_path).context("Failed to parse PBN file")?;
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
            false,
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
        write_rich_pbn(output_path, &boards, &results, config, &system_names)?;
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

/// Write PBN output matching BBA.exe format
fn write_rich_pbn(
    path: &Path,
    boards: &[Board],
    results: &[epbot_core::AuctionResult],
    config: &OutputConfig,
    system_names: &SystemNames,
) -> Result<()> {
    let file = std::fs::File::create(path).context("Failed to create output PBN file")?;
    let mut writer = BufWriter::new(file);

    let today = chrono_date();

    writeln!(writer, "% PBN 2.1")?;
    writeln!(writer, "% Generated by bba-cli")?;
    if !config.ns_conventions_path.is_empty() {
        writeln!(writer, "% CC1 - {}", config.ns_conventions_path)?;
    }
    if !config.ew_conventions_path.is_empty() {
        writeln!(writer, "% CC2 - {}", config.ew_conventions_path)?;
    }

    for (idx, (board, result)) in boards.iter().zip(results.iter()).enumerate() {
        if idx > 0 {
            writeln!(writer)?;
        }

        let dealer = board.dealer.unwrap_or(Direction::North);
        let vul = vulnerability_to_epbot(&board.vulnerable);
        let has_auction = result.success && !result.bids.is_empty();
        let board_num = board.number.unwrap_or((idx + 1) as u32);
        let deal_str = format_deal_pbn(&board.deal);

        writeln!(writer, "[Event \"{}\"]", config.event)?;
        writeln!(writer, "[Site \"\"]")?;
        writeln!(writer, "[Date \"{}\"]", today)?;
        writeln!(writer, "[Board \"{}\"]", board_num)?;

        // BBA-style 28-hex board fingerprint, only with --single-dummy.
        if config.single_dummy {
            let hands_for_hash = hands_for_bba_hash(&board.deal);
            let hash = bba_hash::encode(
                &hands_for_hash,
                direction_to_int(dealer) as u8,
                vul as u8,
                bba_hash::board_extension_for(board_num),
            );
            writeln!(writer, "% {}", hash)?;
        }

        writeln!(writer, "[North \"EPBot\"]")?;
        writeln!(writer, "[East \"EPBot\"]")?;
        writeln!(writer, "[South \"EPBot\"]")?;
        writeln!(writer, "[West \"EPBot\"]")?;
        writeln!(writer, "[Dealer \"{}\"]", direction_char(dealer))?;
        writeln!(writer, "[Vulnerable \"{}\"]", vulnerability_to_pbn(vul))?;
        writeln!(writer, "[Deal \"{}\"]", deal_str)?;

        // Hand analysis
        write_hand_analysis(&mut writer, &board.deal)?;

        if has_auction {
            let bid_strs: Vec<&str> = result.bids.iter().map(|b| b.bid.as_str()).collect();
            let (contract, declarer) =
                derive_contract_declarer(&bid_strs, direction_to_int(dealer));
            writeln!(writer, "[Declarer \"{}\"]", declarer)?;
            writeln!(writer, "[Contract \"{}\"]", contract)?;

            // [Result], [Score], [Scoring] only with --single-dummy.
            if config.single_dummy {
                if let Some(analysis) = result.analysis.as_ref() {
                    if let Some((level, strain, doubled)) = score::parse_contract(&contract) {
                        let strain_idx = strain_index(strain);
                        let tricks = analysis.tricks[strain_idx];
                        let declarer_pos = direction_str_to_int(&declarer);
                        let ns_score = score::score_for_ns(
                            level,
                            strain,
                            doubled,
                            declarer_pos as u8,
                            vul as u8,
                            tricks,
                        );
                        writeln!(writer, "[Result \"{}\"]", tricks)?;
                        writeln!(writer, "[Score \"NS {}\"]", ns_score)?;
                    }
                }
                writeln!(writer, "[Scoring \"{}\"]", scoring_tag(config.scoring))?;
            }

            writeln!(writer, "[Auction \"{}\"]", direction_char(dealer))?;
            write_annotated_auction(&mut writer, &result.bids)?;
        }

        if !system_names.ew.is_empty() {
            writeln!(writer, "[BidSystemEW \"{}\"]", system_names.ew)?;
        }
        if !system_names.ns.is_empty() {
            writeln!(writer, "[BidSystemNS \"{}\"]", system_names.ns)?;
        }

        debug!("Game {}: written", idx + 1);
    }

    writer.flush()?;
    Ok(())
}

/// Write {Shape}, {HCP}, {Losers} comments
fn write_hand_analysis(writer: &mut impl Write, deal: &Deal) -> Result<()> {
    let dirs = [
        Direction::North,
        Direction::East,
        Direction::South,
        Direction::West,
    ];

    let shapes: Vec<String> = dirs
        .iter()
        .map(|&dir| {
            let l = deal.hand(dir).suit_lengths();
            format!("{}{}{}{}", l[0], l[1], l[2], l[3])
        })
        .collect();
    writeln!(
        writer,
        "{{Shape {} {} {} {}}}",
        shapes[0], shapes[1], shapes[2], shapes[3]
    )?;

    let hcps: Vec<u8> = dirs.iter().map(|&dir| deal.hand(dir).hcp()).collect();
    writeln!(
        writer,
        "{{HCP {} {} {} {}}}",
        hcps[0], hcps[1], hcps[2], hcps[3]
    )?;

    let losers: Vec<u8> = dirs.iter().map(|&dir| deal.hand(dir).losers()).collect();
    writeln!(
        writer,
        "{{Losers {} {} {} {}}}",
        losers[0], losers[1], losers[2], losers[3]
    )?;

    Ok(())
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

/// Write auction with column alignment and =N= annotations
fn write_annotated_auction(
    writer: &mut impl Write,
    bids: &[epbot_core::BidInfo],
) -> Result<()> {
    let mut notes: Vec<(usize, String)> = Vec::new();
    let mut entries: Vec<String> = Vec::new();

    for bid in bids {
        // PBN auction uses "1N"/"3N" rather than "1NT"/"3NT" — matches
        // legacy bba-cli-mac. See note in derive_contract_declarer.
        let bid_str = bid.bid.replace("NT", "N");
        let meaning = bid.meaning.as_deref().unwrap_or("");
        if !meaning.is_empty() {
            let note_num = notes.len() + 1;
            notes.push((note_num, meaning.to_string()));
            entries.push(format!("{} ={}=", bid_str, note_num));
        } else {
            entries.push(bid_str);
        }
    }

    for chunk in entries.chunks(4) {
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
        writeln!(writer, "{}", line.trim_end())?;
    }

    for (num, meaning) in &notes {
        writeln!(writer, "[Note \"{}:{}\"]", num, meaning)?;
    }

    Ok(())
}

fn chrono_date() -> String {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let days = now / 86400;
    let mut year = 1970i64;
    let mut remaining_days = days as i64;

    loop {
        let days_in_year = if is_leap_year(year) { 366 } else { 365 };
        if remaining_days < days_in_year {
            break;
        }
        remaining_days -= days_in_year;
        year += 1;
    }

    let days_in_months = if is_leap_year(year) {
        [31, 29, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31]
    } else {
        [31, 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31]
    };

    let mut month = 1;
    for &dim in &days_in_months {
        if remaining_days < dim {
            break;
        }
        remaining_days -= dim;
        month += 1;
    }
    let day = remaining_days + 1;

    format!("{:04}.{:02}.{:02}", year, month, day)
}

fn is_leap_year(year: i64) -> bool {
    (year % 4 == 0 && year % 100 != 0) || (year % 400 == 0)
}
