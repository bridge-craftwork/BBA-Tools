//! EPBot Core — shared bridge bidding engine orchestration.
//!
//! Wraps Edward Piwowar's native EPBot library (C FFI) into a high-level
//! Rust API for generating bridge auctions. Used by both the CLI and web server.

pub mod bba_hash;
pub mod ffi;
pub mod score;

use std::ffi::{CStr, CString};
use std::os::raw::{c_char, c_void};
use thiserror::Error;

// Re-export FFI constants
pub use ffi::{ERR_BUFFER_TOO_SMALL, ERR_EXCEPTION, ERR_NULL_HANDLE, OK};

/// Errors from the EPBot engine.
#[derive(Error, Debug)]
pub enum EPBotError {
    #[error("Failed to create EPBot instance: {0}")]
    CreateFailed(String),
    #[error("EPBot FFI error (code {code}): {message}")]
    FfiError { code: i32, message: String },
    #[error("Invalid PBN deal: {0}")]
    InvalidDeal(String),
    #[error("Convention loading error: {0}")]
    ConventionError(String),
}

/// A single bid in an auction with optional meaning.
#[derive(Debug, Clone)]
pub struct BidInfo {
    /// The bid string (e.g., "1NT", "Pass", "X")
    pub bid: String,
    /// The bid code (0=Pass, 1=X, 2=XX, 5-39=contract bids)
    pub code: i32,
    /// Position of the bidder (0=N, 1=E, 2=S, 3=W)
    pub position: i32,
    /// Short bid meaning from partner's perspective (if alertable)
    pub meaning: Option<String>,
    /// Longer/detailed bid meaning from partner's perspective (if alertable)
    pub meaning_extended: Option<String>,
    /// Whether this bid should be alerted
    pub is_alert: bool,
}

/// Single-dummy analysis from declarer's perspective, populated when
/// `AuctionOptions::single_dummy` is set.
///
/// Index 0..5 corresponds to strains in EPBot's order:
/// 0=Clubs, 1=Diamonds, 2=Hearts, 3=Spades, 4=NoTrump.
#[derive(Debug, Clone, Default)]
pub struct SingleDummyAnalysis {
    /// Estimated tricks for the declaring side, per strain.
    pub tricks: [u8; 5],
    /// Confidence percentage (0..100), per strain.
    pub percentages: [u8; 5],
}

/// Result of generating an auction for a deal.
#[derive(Debug, Clone)]
pub struct AuctionResult {
    pub bids: Vec<BidInfo>,
    pub success: bool,
    pub error: Option<String>,
    /// Single-dummy analysis, if requested via `AuctionOptions::single_dummy`.
    pub analysis: Option<SingleDummyAnalysis>,
}

/// Scoring mode for the auction.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Scoring {
    Matchpoints = 0,
    Imps = 1,
}

impl Default for Scoring {
    fn default() -> Self {
        Scoring::Matchpoints
    }
}

/// Parsed convention card content (lines from a .bbsa file).
#[derive(Debug, Clone)]
pub struct ConventionCard {
    pub lines: Vec<String>,
}

impl ConventionCard {
    /// Parse a .bbsa file from its content string.
    pub fn from_content(content: &str) -> Self {
        Self {
            lines: content.lines().map(|l| l.to_string()).collect(),
        }
    }

    /// Parse a .bbsa file from a line array.
    pub fn from_lines(lines: Vec<String>) -> Self {
        Self { lines }
    }

    /// The bidding-system name EPBot associates with this card, e.g.
    /// `"Sayc - Standard American Yellow Card"`.
    ///
    /// The name is read back **from EPBot** after loading the card rather than
    /// mapped from a table here, so it always agrees with the system the bots
    /// actually bid. EPBot's own numbering is 0 = 2/1GF, 1 = SAYC, 2 = Polish
    /// Club, 3 = Precision Club, 4 = Acol; anything else reports "Not defined".
    /// A card with no `System type` line inherits EPBot's default, which is 0.
    ///
    /// Creates and destroys a throwaway instance, so call it once per card and
    /// keep the result — not per deal.
    pub fn system_name(&self, side: i32) -> Result<String, EPBotError> {
        let instance = unsafe { ffi::epbot_create() };
        if instance.is_null() {
            return Err(EPBotError::CreateFailed(get_last_error()));
        }

        let result = (|| {
            self.apply_to(instance, side)?;
            let mut buf = [0 as c_char; 512];
            let rc = unsafe {
                ffi::epbot_system_name(instance, side, buf.as_mut_ptr(), buf.len() as i32)
            };
            if rc != ffi::OK {
                return Err(EPBotError::FfiError {
                    code: rc,
                    message: format!("epbot_system_name(side {side}) failed"),
                });
            }
            Ok(unsafe { CStr::from_ptr(buf.as_ptr()) }
                .to_str()
                .unwrap_or("")
                .to_string())
        })();

        unsafe { ffi::epbot_destroy(instance) };
        result
    }

    /// Load conventions into an EPBot instance for the given side (0=NS, 1=EW).
    /// Mirrors the C# LoadConventions logic from EPBotService.cs.
    fn apply_to(&self, instance: *mut c_void, side: i32) -> Result<(), EPBotError> {
        for line in &self.lines {
            let trimmed = line.trim();
            if trimmed.is_empty() || trimmed.starts_with('#') || trimmed.starts_with(';') {
                continue;
            }

            let parts: Vec<&str> = trimmed.splitn(2, '=').collect();
            if parts.len() != 2 {
                continue;
            }

            let key = parts[0].trim();
            let value_str = parts[1].trim();

            // Try parsing as integer
            if let Ok(int_value) = value_str.parse::<i32>() {
                if key.eq_ignore_ascii_case("System type") {
                    let rc = unsafe { ffi::epbot_set_system_type(instance, side, int_value) };
                    if rc < 0 && rc != ffi::ERR_EXCEPTION {
                        log::warn!("set_system_type({}, {}) returned {}", side, int_value, rc);
                    }
                } else if key.eq_ignore_ascii_case("Opponent type") {
                    let rc = unsafe { ffi::epbot_set_opponent_type(instance, side, int_value) };
                    if rc < 0 && rc != ffi::ERR_EXCEPTION {
                        log::warn!("set_opponent_type({}, {}) returned {}", side, int_value, rc);
                    }
                } else if let Ok(key_c) = CString::new(key) {
                    let bool_val = if int_value == 1 { 1 } else { 0 };
                    unsafe {
                        ffi::epbot_set_conventions(instance, side, key_c.as_ptr(), bool_val);
                    }
                }
            } else if value_str.eq_ignore_ascii_case("true") {
                if let Ok(key_c) = CString::new(key) {
                    unsafe {
                        ffi::epbot_set_conventions(instance, side, key_c.as_ptr(), 1);
                    }
                }
            } else if value_str.eq_ignore_ascii_case("false") {
                if let Ok(key_c) = CString::new(key) {
                    unsafe {
                        ffi::epbot_set_conventions(instance, side, key_c.as_ptr(), 0);
                    }
                }
            }
        }
        Ok(())
    }
}

/// Parse a PBN deal string into per-player hands in EPBot's C.D.H.S order.
///
/// Input format: "N:AKQ.JT9.876.543 ... ... ..."
/// PBN suit order is S.H.D.C, but EPBot expects C.D.H.S.
///
/// Returns (first_seat, hands) where hands[position] = "clubs\ndiamonds\nhearts\nspades"
fn parse_pbn_deal(pbn: &str) -> Result<(i32, [String; 4]), EPBotError> {
    let colon_pos = pbn
        .find(':')
        .ok_or_else(|| EPBotError::InvalidDeal("Missing colon in PBN deal".into()))?;

    if colon_pos == 0 {
        return Err(EPBotError::InvalidDeal("Missing seat before colon".into()));
    }

    let first_seat_char = &pbn[colon_pos - 1..colon_pos];
    let first_seat = match first_seat_char.to_uppercase().as_str() {
        "N" => 0,
        "E" => 1,
        "S" => 2,
        "W" => 3,
        _ => return Err(EPBotError::InvalidDeal(format!("Invalid seat: {}", first_seat_char))),
    };

    let hands_str = &pbn[colon_pos + 1..];
    let hand_parts: Vec<&str> = hands_str.split_whitespace().collect();

    if hand_parts.len() != 4 {
        return Err(EPBotError::InvalidDeal(format!(
            "Expected 4 hands, got {}",
            hand_parts.len()
        )));
    }

    let mut hands: [String; 4] = Default::default();

    for (i, hand_str) in hand_parts.iter().enumerate() {
        let pos = (first_seat + i as i32) % 4;
        let suits: Vec<&str> = hand_str.split('.').collect();

        if suits.len() != 4 {
            return Err(EPBotError::InvalidDeal(format!(
                "Expected 4 suits in hand {}, got {}",
                i,
                suits.len()
            )));
        }

        // PBN is S.H.D.C, EPBot wants C.D.H.S (reversed)
        hands[pos as usize] = format!("{}\n{}\n{}\n{}", suits[3], suits[2], suits[1], suits[0]);
    }

    Ok((first_seat, hands))
}

/// Decode an EPBot bid code to a human-readable string.
pub fn decode_bid(code: i32) -> String {
    match code {
        0 => "Pass".to_string(),
        1 => "X".to_string(),
        2 => "XX".to_string(),
        c if (5..=39).contains(&c) => {
            let adjusted = c - 5;
            let level = adjusted / 5 + 1;
            let suit = adjusted % 5;
            let suit_str = match suit {
                0 => "C",
                1 => "D",
                2 => "H",
                3 => "S",
                4 => "NT",
                _ => "?",
            };
            format!("{}{}", level, suit_str)
        }
        _ => format!("?{}", code),
    }
}

/// Encode a bid string to an EPBot bid code, returning an error on invalid input.
/// Pass=0, X=1, XX=2, 1C=5, 1D=6, ..., 7NT=39 (also tolerates "1N" as "1NT").
pub fn try_encode_bid(bid: &str) -> Result<i32, String> {
    match bid {
        "Pass" | "P" | "pass" => Ok(0),
        "X" => Ok(1),
        "XX" => Ok(2),
        _ => {
            let bytes = bid.as_bytes();
            if bytes.len() >= 2 && bytes[0].is_ascii_digit() {
                let level = (bytes[0] - b'0') as i32;
                let suit = match &bid[1..] {
                    "C" => 0,
                    "D" => 1,
                    "H" => 2,
                    "S" => 3,
                    "N" | "NT" => 4,
                    other => return Err(format!("unrecognised strain '{}'", other)),
                };
                if (1..=7).contains(&level) {
                    Ok(5 + (level - 1) * 5 + suit)
                } else {
                    Err(format!("invalid bid level: {}", level))
                }
            } else {
                Err(format!("unrecognised bid '{}'", bid))
            }
        }
    }
}

/// Encode a bid string to an EPBot bid code (silently returns 0 on invalid input).
/// Prefer `try_encode_bid` when invalid input should be reported.
pub fn encode_bid(bid: &str) -> i32 {
    try_encode_bid(bid).unwrap_or(0)
}

/// Get the EPBot library version number.
pub fn version() -> Result<i32, EPBotError> {
    let inst = unsafe { ffi::epbot_create() };
    if inst.is_null() {
        return Err(EPBotError::CreateFailed(get_last_error()));
    }
    let v = unsafe { ffi::epbot_version(inst) };
    unsafe { ffi::epbot_destroy(inst) };
    Ok(v)
}

/// Get the EPBot copyright string.
pub fn copyright() -> Result<String, EPBotError> {
    let inst = unsafe { ffi::epbot_create() };
    if inst.is_null() {
        return Err(EPBotError::CreateFailed(get_last_error()));
    }
    let mut buf = [0 as c_char; 512];
    let rc = unsafe { ffi::epbot_copyright(inst, buf.as_mut_ptr(), buf.len() as i32) };
    unsafe { ffi::epbot_destroy(inst) };

    if rc != ffi::OK {
        return Err(EPBotError::FfiError {
            code: rc,
            message: "copyright() failed".into(),
        });
    }

    let s = unsafe { CStr::from_ptr(buf.as_ptr()) }
        .to_str()
        .unwrap_or("?")
        .to_string();
    Ok(s)
}

/// Get the last FFI error message.
fn get_last_error() -> String {
    unsafe {
        let ptr = ffi::epbot_get_last_error();
        if ptr.is_null() {
            "Unknown error".to_string()
        } else {
            CStr::from_ptr(ptr)
                .to_str()
                .unwrap_or("?")
                .to_string()
        }
    }
}

/// Generate a complete auction for a deal.
///
/// This is the main entry point. It:
/// 1. Parses the PBN deal string
/// 2. Creates 4 EPBot instances (one per player)
/// 3. Loads conventions into each instance
/// 4. Runs the bidding loop until the auction completes
/// 5. Collects bid meanings from partner perspectives
///
/// # Arguments
/// * `pbn` - PBN deal string, e.g. "N:AKQ.JT9.876.543 ..."
/// * `dealer` - Dealer position (0=N, 1=E, 2=S, 3=W)
/// * `vulnerability` - 0=None, 1=EW, 2=NS, 3=Both (EPBot convention from EPBotService.cs)
/// * `scoring` - Scoring mode
/// * `ns_card` - Convention card for NS (or None for defaults)
/// * `ew_card` - Convention card for EW (or None for defaults)
pub fn generate_auction(
    pbn: &str,
    dealer: i32,
    vulnerability: i32,
    scoring: Scoring,
    ns_card: Option<&ConventionCard>,
    ew_card: Option<&ConventionCard>,
) -> AuctionResult {
    generate_auction_with_prefix(pbn, dealer, vulnerability, scoring, ns_card, ew_card, None)
}

/// Generate an auction, optionally forcing the first N bids from `auction_prefix`.
///
/// When `auction_prefix` is provided, the bidding loop uses those bids for the
/// first N positions instead of calling `epbot_get_bid`. `epbot_set_bid` is still
/// called on every player so EPBot's internal state stays consistent. After the
/// prefix the bots resume normal bidding from the new state.
pub fn generate_auction_with_prefix(
    pbn: &str,
    dealer: i32,
    vulnerability: i32,
    scoring: Scoring,
    ns_card: Option<&ConventionCard>,
    ew_card: Option<&ConventionCard>,
    auction_prefix: Option<&[String]>,
) -> AuctionResult {
    generate_auction_with_options(
        pbn,
        dealer,
        vulnerability,
        scoring,
        ns_card,
        ew_card,
        auction_prefix,
        false,
        false,
    )
}

/// Full-featured entry point. `single_dummy` requests EPBot's single-dummy
/// trick estimate from declarer's perspective once the auction completes.
///
/// SD analysis is opt-in. Measured cost on a 500-board PBN file (macOS arm64,
/// EPBot 8740): ~0.22 ms per board, ~3.6% of total per-deal latency. Cheap
/// enough to default on, but kept opt-in so callers control when the extra
/// FFI call happens.
///
/// `all_meanings` collects `meaning`/`meaning_extended` for *every* bid rather
/// than only alertable ones. Off by default: it roughly triples the size of a
/// serialized auction and most callers only care about alerts.
pub fn generate_auction_with_options(
    pbn: &str,
    dealer: i32,
    vulnerability: i32,
    scoring: Scoring,
    ns_card: Option<&ConventionCard>,
    ew_card: Option<&ConventionCard>,
    auction_prefix: Option<&[String]>,
    single_dummy: bool,
    all_meanings: bool,
) -> AuctionResult {
    match generate_auction_inner(pbn, dealer, vulnerability, scoring, ns_card, ew_card, auction_prefix, single_dummy, all_meanings) {
        Ok((bids, analysis)) => AuctionResult {
            bids,
            success: true,
            error: None,
            analysis,
        },
        Err(e) => AuctionResult {
            bids: Vec::new(),
            success: false,
            error: Some(e.to_string()),
            analysis: None,
        },
    }
}

fn generate_auction_inner(
    pbn: &str,
    dealer: i32,
    vulnerability: i32,
    scoring: Scoring,
    ns_card: Option<&ConventionCard>,
    ew_card: Option<&ConventionCard>,
    auction_prefix: Option<&[String]>,
    single_dummy: bool,
    all_meanings: bool,
) -> Result<(Vec<BidInfo>, Option<SingleDummyAnalysis>), EPBotError> {
    let (_first_seat, hands) = parse_pbn_deal(pbn)?;

    // Create 4 EPBot instances — one per player
    let mut players: [*mut c_void; 4] = [std::ptr::null_mut(); 4];
    let empty_alert = CString::new("").unwrap();

    for i in 0..4 {
        players[i] = unsafe { ffi::epbot_create() };
        if players[i].is_null() {
            let reason = get_last_error();
            // Clean up already-created instances
            for j in 0..i {
                unsafe { ffi::epbot_destroy(players[j]) };
            }
            return Err(EPBotError::CreateFailed(format!(
                "player {} of 4: {}", i, reason
            )));
        }
    }

    // Use a closure-like pattern to ensure cleanup on any error
    let bids_result = run_auction(&players, &hands, dealer, vulnerability, scoring, ns_card, ew_card, &empty_alert, auction_prefix, all_meanings);

    let final_result = match bids_result {
        Ok(bids) => {
            let analysis = if single_dummy {
                match compute_single_dummy(&players, &hands, &bids) {
                    Ok(a) => Some(a),
                    Err(e) => {
                        log::warn!("single-dummy analysis failed: {}", e);
                        None
                    }
                }
            } else {
                None
            };
            Ok((bids, analysis))
        }
        Err(e) => Err(e),
    };

    // Always destroy all instances
    for p in &players {
        if !p.is_null() {
            unsafe { ffi::epbot_destroy(*p) };
        }
    }

    final_result
}

/// Estimate single-dummy tricks for the declaring side.
///
/// Resolves declarer from the auction (last bid, plus first time the declaring
/// side bid the strain), then calls `epbot_get_sd_tricks` on declarer's
/// instance, passing dummy's hand. Returns one entry per strain in EPBot order
/// (C, D, H, S, NT).
fn compute_single_dummy(
    players: &[*mut c_void; 4],
    hands: &[String; 4],
    bids: &[BidInfo],
) -> Result<SingleDummyAnalysis, EPBotError> {
    let Some(declarer) = derive_declarer(bids) else {
        return Err(EPBotError::FfiError {
            code: 0,
            message: "auction has no contract — cannot compute single-dummy tricks".into(),
        });
    };

    let dummy = (declarer + 2) % 4;
    let dummy_hand_c = CString::new(hands[dummy as usize].as_str()).map_err(|e| {
        EPBotError::InvalidDeal(format!("Invalid dummy hand string: {}", e))
    })?;

    // Buffers sized generously. EPBot's SD-tricks output is at least
    // strain-by-declarer (5 * 4 = 20); we over-allocate further to absorb any
    // additional rows EPBot may include and trust `count_out`.
    let mut tricks_buf = [0i32; 64];
    let mut pct_buf = [0i32; 64];
    let mut tricks_count: i32 = 0;
    let mut pct_count: i32 = 0;

    let rc = unsafe {
        ffi::epbot_get_sd_tricks(
            players[declarer as usize],
            dummy_hand_c.as_ptr(),
            tricks_buf.as_mut_ptr(),
            tricks_buf.len() as i32,
            &mut tricks_count,
            pct_buf.as_mut_ptr(),
            pct_buf.len() as i32,
            &mut pct_count,
        )
    };
    if rc < 0 {
        return Err(EPBotError::FfiError {
            code: rc,
            message: format!("epbot_get_sd_tricks failed: {}", get_last_error()),
        });
    }

    log::debug!(
        "SD raw: tricks_count={} pct_count={} tricks={:?} pct={:?}",
        tricks_count,
        pct_count,
        &tricks_buf[..(tricks_count as usize).min(tricks_buf.len())],
        &pct_buf[..(pct_count as usize).min(pct_buf.len())]
    );

    let mut analysis = SingleDummyAnalysis::default();
    let n_tricks = (tricks_count as usize).min(5);
    let n_pct = (pct_count as usize).min(5);
    for i in 0..n_tricks {
        analysis.tricks[i] = tricks_buf[i].clamp(0, 13) as u8;
    }
    for i in 0..n_pct {
        analysis.percentages[i] = pct_buf[i].clamp(0, 100) as u8;
    }
    Ok(analysis)
}

/// Determine the declarer position (0..3) from a completed auction.
/// Returns None for "all pass" auctions.
fn derive_declarer(bids: &[BidInfo]) -> Option<i32> {
    // Find the final contract bid (not Pass/X/XX).
    let last_contract = bids.iter().rposition(|b| {
        let s = b.bid.as_str();
        s != "Pass" && s != "X" && s != "XX" && !s.is_empty()
    })?;

    let strain = strain_of_bid(&bids[last_contract].bid)?;
    let declaring_pos = bids[last_contract].position;
    let declaring_is_ns = declaring_pos == 0 || declaring_pos == 2;

    // Declarer is the first player on the declaring side to name that strain.
    for b in bids.iter().take(last_contract + 1) {
        let pos_is_ns = b.position == 0 || b.position == 2;
        if pos_is_ns != declaring_is_ns {
            continue;
        }
        if let Some(s) = strain_of_bid(&b.bid) {
            if s == strain {
                return Some(b.position);
            }
        }
    }
    Some(declaring_pos)
}

/// Strain code 0..4 (C/D/H/S/NT) for a contract bid like "3NT" or "4S".
fn strain_of_bid(bid: &str) -> Option<usize> {
    let rest = bid.get(1..)?;
    let rest_upper = rest.to_uppercase();
    match rest_upper.as_str() {
        "C" => Some(0),
        "D" => Some(1),
        "H" => Some(2),
        "S" => Some(3),
        "N" | "NT" => Some(4),
        _ => None,
    }
}

fn run_auction(
    players: &[*mut c_void; 4],
    hands: &[String; 4],
    dealer: i32,
    vulnerability: i32,
    scoring: Scoring,
    ns_card: Option<&ConventionCard>,
    ew_card: Option<&ConventionCard>,
    empty_alert: &CString,
    auction_prefix: Option<&[String]>,
    all_meanings: bool,
) -> Result<Vec<BidInfo>, EPBotError> {
    // Initialize each player
    for i in 0..4 {
        let hand_c = CString::new(hands[i].as_str()).map_err(|e| {
            EPBotError::InvalidDeal(format!("Invalid hand string for position {}: {}", i, e))
        })?;

        let rc = unsafe {
            ffi::epbot_new_hand(
                players[i],
                i as i32,
                hand_c.as_ptr(),
                dealer,
                vulnerability,
                0, // not repeating
                0, // not playing
            )
        };

        if rc < 0 {
            return Err(EPBotError::FfiError {
                code: rc,
                message: format!("new_hand failed for position {}: {}", i, get_last_error()),
            });
        }

        // Set scoring
        unsafe { ffi::epbot_set_scoring(players[i], scoring as i32) };

        // Load conventions
        if let Some(card) = ns_card {
            card.apply_to(players[i], 0)?;
        }
        if let Some(card) = ew_card {
            card.apply_to(players[i], 1)?;
        }
    }

    // Run the auction
    let mut bids = Vec::new();
    let mut current_pos = dealer;
    let mut pass_count = 0;
    let mut has_bid = false;
    let prefix_len = auction_prefix.map(|p| p.len()).unwrap_or(0);

    for round in 0..100 {
        // Get bid: from forced prefix if we're still in it, otherwise from EPBot.
        let (bid_code, bid_str) = if round < prefix_len {
            let forced = &auction_prefix.unwrap()[round];
            let code = try_encode_bid(forced).map_err(|e| EPBotError::FfiError {
                code: 0,
                message: format!("Invalid auctionPrefix at index {}: {}", round, e),
            })?;
            (code, decode_bid(code))
        } else {
            let code = unsafe { ffi::epbot_get_bid(players[current_pos as usize]) };
            if code < 0 {
                return Err(EPBotError::FfiError {
                    code,
                    message: format!("get_bid failed for position {}: {}", current_pos, get_last_error()),
                });
            }
            (code, decode_bid(code))
        };

        // Broadcast bid to all players
        for j in 0..4 {
            let rc = unsafe {
                ffi::epbot_set_bid(
                    players[j],
                    current_pos,
                    bid_code,
                    empty_alert.as_ptr(),
                )
            };
            if rc < 0 {
                log::warn!(
                    "set_bid({}, {}, {}) failed with code {}",
                    j,
                    current_pos,
                    bid_code,
                    rc
                );
            }
        }

        // Get bid meaning from partner's perspective
        let partner_pos = (current_pos + 2) % 4;
        let mut meaning = None;
        let mut meaning_extended = None;
        let mut is_alert = false;

        let alert_rc = unsafe { ffi::epbot_get_info_alerting(players[partner_pos as usize], current_pos) };
        if alert_rc == 1 {
            is_alert = true;
        }

        // EPBot fills the info block on every bid, so a description is
        // available for natural bids too — but by default we only pull it for
        // alertable ones, which is all the browser extensions need. Callers
        // that want the full picture opt in via `all_meanings`.
        if is_alert || all_meanings {
            let mut buf = [0 as c_char; 1024];
            let meaning_rc = unsafe {
                ffi::epbot_get_info_meaning(
                    players[partner_pos as usize],
                    current_pos,
                    buf.as_mut_ptr(),
                    buf.len() as i32,
                )
            };
            if meaning_rc == ffi::OK {
                let s = unsafe { CStr::from_ptr(buf.as_ptr()) }
                    .to_str()
                    .unwrap_or("")
                    .to_string();
                if !s.is_empty() {
                    meaning = Some(s);
                }
            }

            // Fetch the extended meaning independently — a failure here must
            // not drop the short meaning we already have.
            let mut ext_buf = [0 as c_char; 4096];
            let ext_rc = unsafe {
                ffi::epbot_get_info_meaning_extended(
                    players[partner_pos as usize],
                    current_pos,
                    ext_buf.as_mut_ptr(),
                    ext_buf.len() as i32,
                )
            };
            if ext_rc == ffi::OK {
                let s = unsafe { CStr::from_ptr(ext_buf.as_ptr()) }
                    .to_str()
                    .unwrap_or("")
                    .to_string();
                if !s.is_empty() {
                    meaning_extended = Some(s);
                }
            }
        }

        bids.push(BidInfo {
            bid: bid_str.clone(),
            code: bid_code,
            position: current_pos,
            meaning,
            meaning_extended,
            is_alert,
        });

        // Track passes for auction termination
        if bid_str == "Pass" {
            pass_count += 1;
        } else {
            pass_count = 0;
            has_bid = true;
        }

        // Auction ends: 3 passes after a bid, or 4 initial passes
        if (has_bid && pass_count >= 3) || (!has_bid && pass_count >= 4) {
            break;
        }

        current_pos = (current_pos + 1) % 4;
    }

    Ok(bids)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_decode_bid() {
        assert_eq!(decode_bid(0), "Pass");
        assert_eq!(decode_bid(1), "X");
        assert_eq!(decode_bid(2), "XX");
        assert_eq!(decode_bid(5), "1C");
        assert_eq!(decode_bid(9), "1NT");
        assert_eq!(decode_bid(10), "2C");
        assert_eq!(decode_bid(39), "7NT");
    }

    #[test]
    fn test_encode_bid() {
        assert_eq!(encode_bid("Pass"), 0);
        assert_eq!(encode_bid("X"), 1);
        assert_eq!(encode_bid("XX"), 2);
        assert_eq!(encode_bid("1C"), 5);
        assert_eq!(encode_bid("1NT"), 9);
        assert_eq!(encode_bid("7NT"), 39);
    }

    #[test]
    fn test_parse_pbn_deal() {
        let pbn = "N:AKQ.JT9.876.543 JT9.876.543.AKQ 876.543.AKQ.JT9 543.AKQ.JT9.876";
        let (first_seat, hands) = parse_pbn_deal(pbn).unwrap();
        assert_eq!(first_seat, 0); // North

        // North hand: PBN is S.H.D.C = AKQ.JT9.876.543
        // EPBot wants C.D.H.S = 543.876.JT9.AKQ
        assert_eq!(hands[0], "543\n876\nJT9\nAKQ");
    }

    #[test]
    fn test_parse_pbn_deal_south_first() {
        let pbn = "S:AKQ.JT9.876.543 JT9.876.543.AKQ 876.543.AKQ.JT9 543.AKQ.JT9.876";
        let (first_seat, hands) = parse_pbn_deal(pbn).unwrap();
        assert_eq!(first_seat, 2); // South

        // First hand in string is South's
        assert_eq!(hands[2], "543\n876\nJT9\nAKQ");
    }

    #[test]
    fn test_convention_card_parse() {
        let content = "# Comment\nSystem type = 5\nOpponent type = 0\nSMOLEN = 1\n; another comment\nGarbage Stayman = true\nUnused = false\n";
        let card = ConventionCard::from_content(content);
        assert_eq!(card.lines.len(), 7);
    }

    /// The `[BidSystemNS]`/`[BidSystemEW]` tags bba-cli writes come from this,
    /// so a regression here silently mislabels every generated PBN (issue #3).
    /// EPBot's numbering: 0 = 2/1GF, 1 = SAYC, 2 = Polish Club, 3 = Precision
    /// Club, 4 = Acol.
    #[test]
    fn test_system_name_follows_card_system_type() {
        for (system_type, expected) in [
            (0, "2/1GF"),
            (1, "Sayc"),
            (2, "WJ"),
            (3, "PC"),
            (4, "Acol"),
        ] {
            let card = ConventionCard::from_content(&format!("System type = {system_type}\n"));
            let name = card
                .system_name(0)
                .unwrap_or_else(|e| panic!("system type {system_type}: {e}"));
            assert!(
                name.starts_with(expected),
                "system type {system_type} should name a {expected} system, got {name:?}"
            );
        }
    }

    /// A card that never sets `System type` inherits EPBot's default rather
    /// than reporting nothing, and that default is 2/1GF.
    #[test]
    fn test_system_name_defaults_to_two_over_one() {
        let card = ConventionCard::from_content("SMOLEN = 1\n");
        assert!(card.system_name(0).unwrap().starts_with("2/1GF"));
    }

    /// Both sides are resolved independently — the E-W tag must not inherit
    /// the N-S card's system.
    #[test]
    fn test_system_name_is_per_side() {
        let sayc = ConventionCard::from_content("System type = 1\n");
        assert!(sayc.system_name(1).unwrap().starts_with("Sayc"));
    }
}
