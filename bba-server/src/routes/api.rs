use axum::extract::connect_info::ConnectInfo;
use axum::extract::State;
use axum::http::HeaderMap;
use axum::Json;
use std::net::SocketAddr;
use std::time::Instant;
use crate::models::*;
use crate::AppState;
use crate::services::ip_anonymizer;
use epbot_core::{ConventionCard, Scoring};

/// Extract client IP from headers (Cloudflare → X-Forwarded-For → connection).
fn get_client_ip(headers: &HeaderMap, conn: &SocketAddr) -> Option<String> {
    headers
        .get("CF-Connecting-IP")
        .or_else(|| headers.get("X-Forwarded-For"))
        .or_else(|| headers.get("X-Real-IP"))
        .and_then(|v| v.to_str().ok())
        .map(|s| s.split(',').next().unwrap_or(s).trim().to_string())
        .or_else(|| Some(conn.ip().to_string()))
}

fn get_client_version(headers: &HeaderMap) -> String {
    headers
        .get("X-Client-Version")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_string()
}

/// Parse X-Client-Info header: "ext=BBOAlert; browser=Chrome; os=macOS"
pub struct ClientInfo {
    pub extension: String,
    pub browser: String,
    pub os: String,
}

fn get_client_info(headers: &HeaderMap) -> ClientInfo {
    let raw = headers
        .get("X-Client-Info")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");

    let mut extension = String::new();
    let mut browser = String::new();
    let mut os = String::new();

    for part in raw.split(';') {
        let part = part.trim();
        if let Some((key, val)) = part.split_once('=') {
            match key.trim() {
                "ext" => extension = val.trim().to_string(),
                "browser" => browser = val.trim().to_string(),
                "os" => os = val.trim().to_string(),
                _ => {}
            }
        }
    }

    ClientInfo { extension, browser, os }
}

/// POST /api/auction/generate
pub async fn generate_auction(
    State(state): State<AppState>,
    headers: HeaderMap,
    ConnectInfo(conn): ConnectInfo<SocketAddr>,
    Json(request): Json<AuctionRequest>,
) -> Json<AuctionResponse> {
    let start = Instant::now();
    let raw_ip = get_client_ip(&headers, &conn);
    let anon_ip = ip_anonymizer::anonymize(raw_ip.as_deref());
    let client_version = get_client_version(&headers);
    let client_info = get_client_info(&headers);

    // Determine convention cards
    let conventions = if let Some(ref conv) = request.conventions {
        conv.clone()
    } else if let Some(ref scenario) = request.scenario {
        let (ns, ew) = state
            .convention_service
            .get_conventions_for_scenario(scenario)
            .await;
        ConventionCards { ns, ew }
    } else {
        ConventionCards::default()
    };

    // Fetch convention card content from GitHub
    let ns_content = state
        .convention_service
        .get_bbsa_content(&conventions.ns)
        .await;
    let ew_content = state
        .convention_service
        .get_bbsa_content(&conventions.ew)
        .await;

    let response = match (ns_content, ew_content) {
        (Ok(ns_text), Ok(ew_text)) => {
            let ns_card = ConventionCard::from_content(&ns_text);
            let ew_card = ConventionCard::from_content(&ew_text);

            // Parse dealer
            let dealer = parse_dealer(&request.deal.dealer);
            let vul = parse_vulnerability(&request.deal.vulnerability);
            let scoring = if request.deal.scoring.eq_ignore_ascii_case("IMP") {
                Scoring::Imps
            } else {
                Scoring::Matchpoints
            };

            // Acquire semaphore permit for concurrency limiting
            let _permit = state.semaphore.acquire().await.unwrap();

            // Clone values needed after the spawn_blocking move
            let pbn = request.deal.pbn.clone();
            let deal_dealer = request.deal.dealer.clone();
            let deal_vul = request.deal.vulnerability.clone();
            let deal_scoring = request.deal.scoring.clone();
            let deal_pbn = request.deal.pbn.clone();
            let scenario_clone = request.scenario.clone();
            let auction_prefix = request.auction_prefix.clone();
            let single_dummy = request.single_dummy;
            let all_meanings = request.include_all_meanings;

            let result = tokio::task::spawn_blocking(move || {
                epbot_core::generate_auction_with_options(
                    &pbn,
                    dealer,
                    vul,
                    scoring,
                    Some(&ns_card),
                    Some(&ew_card),
                    auction_prefix.as_deref(),
                    single_dummy,
                    all_meanings,
                )
            })
            .await
            .unwrap();

            if result.success {
                let auction: Vec<String> = result.bids.iter().map(|b| b.bid.clone()).collect();
                let encoded = encode_bbo_format(&auction);
                let meanings: Vec<BidMeaning> = result
                    .bids
                    .iter()
                    .enumerate()
                    .map(|(i, b)| BidMeaning {
                        position: i,
                        bid: b.bid.clone(),
                        meaning: b.meaning.clone(),
                        meaning_extended: b.meaning_extended.clone(),
                        is_alert: b.is_alert,
                    })
                    .collect();

                let alerts_str = format_alerts(&meanings);
                let duration = start.elapsed().as_millis() as u64;
                let auction_readable = format_readable_auction(&auction);

                state.audit_log.log_request(
                    &anon_ip,
                    &client_version,
                    &client_info.extension,
                    &client_info.browser,
                    &client_info.os,
                    duration,
                    state.epbot_version,
                    &deal_dealer,
                    &deal_vul,
                    &deal_scoring,
                    &conventions.ns,
                    &conventions.ew,
                    scenario_clone.as_deref().unwrap_or(""),
                    &deal_pbn,
                    true,
                    &auction_readable,
                    &alerts_str,
                    "",
                );

                let (contract, declarer, sd_result, sd_score, sd_hash) = if single_dummy {
                    let (c, d) = derive_contract_and_declarer(&auction, dealer);
                    let board_number = request.board_number.unwrap_or(1);
                    let (r, s, h) = derive_single_dummy_outputs(
                        &result.analysis,
                        c.as_deref(),
                        d.as_deref(),
                        &request.deal.pbn,
                        dealer,
                        vul,
                        board_number,
                    );
                    (c, d, r, s, h)
                } else {
                    (None, None, None, None, None)
                };

                AuctionResponse {
                    success: true,
                    auction: Some(auction),
                    auction_encoded: Some(encoded),
                    conventions_used: Some(conventions),
                    meanings: Some(meanings),
                    contract,
                    declarer,
                    result: sd_result,
                    score: sd_score,
                    board_hash: sd_hash,
                    error: None,
                }
            } else {
                let err = result.error.unwrap_or_else(|| "Unknown error".into());
                let duration = start.elapsed().as_millis() as u64;

                state.audit_log.log_request(
                    &anon_ip,
                    &client_version,
                    &client_info.extension,
                    &client_info.browser,
                    &client_info.os,
                    duration,
                    state.epbot_version,
                    &deal_dealer,
                    &deal_vul,
                    &deal_scoring,
                    &conventions.ns,
                    &conventions.ew,
                    scenario_clone.as_deref().unwrap_or(""),
                    &deal_pbn,
                    false,
                    "",
                    "",
                    &err,
                );

                AuctionResponse {
                    success: false,
                    auction: None,
                    auction_encoded: None,
                    conventions_used: Some(conventions),
                    meanings: None,
                    contract: None,
                    declarer: None,
                    result: None,
                    score: None,
                    board_hash: None,
                    error: Some(err),
                }
            }
        }
        (Err(e), _) | (_, Err(e)) => AuctionResponse {
            success: false,
            auction: None,
            auction_encoded: None,
            conventions_used: Some(conventions),
            meanings: None,
            contract: None,
            declarer: None,
            result: None,
            score: None,
            board_hash: None,
            error: Some(e),
        },
    };

    Json(response)
}

/// POST /api/scenario/select
pub async fn select_scenario(
    State(state): State<AppState>,
    headers: HeaderMap,
    ConnectInfo(conn): ConnectInfo<SocketAddr>,
    Json(request): Json<ScenarioSelectRequest>,
) -> Json<serde_json::Value> {
    let raw_ip = get_client_ip(&headers, &conn);
    let anon_ip = ip_anonymizer::anonymize(raw_ip.as_deref());
    let client_version = get_client_version(&headers);
    let client_info = get_client_info(&headers);

    state.audit_log.log_scenario_selection(
        &anon_ip,
        &client_version,
        &client_info.extension,
        &client_info.browser,
        &client_info.os,
        request.scenario.as_deref().unwrap_or(""),
    );

    Json(serde_json::json!({ "success": true }))
}

/// GET /api/scenarios
pub async fn list_scenarios(
    State(state): State<AppState>,
) -> Json<serde_json::Value> {
    match state.convention_service.get_scenario_list().await {
        Ok(scenarios) => Json(serde_json::json!({ "scenarios": scenarios })),
        Err(e) => Json(serde_json::json!({ "scenarios": [], "error": e })),
    }
}

fn parse_dealer(dealer: &str) -> i32 {
    match dealer.to_uppercase().as_str() {
        "N" | "NORTH" => 0,
        "E" | "EAST" => 1,
        "S" | "SOUTH" => 2,
        "W" | "WEST" => 3,
        _ => 0,
    }
}

fn parse_vulnerability(vul: &str) -> i32 {
    match vul.to_uppercase().replace('-', "").as_str() {
        "NONE" | "" => 0,
        "EW" | "EASTWEST" => 1,
        "NS" | "NORTHSOUTH" => 2,
        "BOTH" | "ALL" => 3,
        _ => 0,
    }
}

fn encode_bbo_format(bids: &[String]) -> String {
    bids.iter()
        .map(|bid| match bid.as_str() {
            "Pass" => "--".to_string(),
            "X" => "Db".to_string(),
            "XX" => "Rd".to_string(),
            b if b.ends_with("NT") => format!("{}N", &b[..1]),
            b => format!("{:<2}", b),
        })
        .collect()
}

fn format_readable_auction(bids: &[String]) -> String {
    let s = bids.join(" ");
    if s == "Pass Pass Pass Pass" {
        return "PassOut".to_string();
    }
    if s.ends_with(" Pass Pass Pass") {
        return format!("{} AllPass", &s[..s.len() - " Pass Pass Pass".len()]);
    }
    s
}

/// Walk the bid list, return final contract string ("4H", "3NTX", etc.) and
/// declarer seat letter. Returns `(None, None)` for an all-pass auction.
fn derive_contract_and_declarer(bids: &[String], dealer: i32) -> (Option<String>, Option<String>) {
    let mut last_contract: Option<&str> = None;
    let mut last_idx = 0;
    let mut doubled = false;
    let mut redoubled = false;
    for (i, bid) in bids.iter().enumerate() {
        match bid.as_str() {
            "Pass" => {}
            "X" => { doubled = true; redoubled = false; }
            "XX" => { redoubled = true; doubled = false; }
            other => { last_contract = Some(other); last_idx = i; doubled = false; redoubled = false; }
        }
    }
    let bid = match last_contract { Some(b) => b, None => return (None, None) };
    let strain = &bid[1..];
    let suffix = if redoubled { "XX" } else if doubled { "X" } else { "" };
    let contract = format!("{}{}", bid, suffix);

    let last_pos = (dealer + last_idx as i32) % 4;
    let last_is_ns = last_pos == 0 || last_pos == 2;
    let mut declarer_pos = last_pos;
    for (i, b) in bids.iter().enumerate().take(last_idx + 1) {
        let pos = (dealer + i as i32) % 4;
        let pos_is_ns = pos == 0 || pos == 2;
        if pos_is_ns != last_is_ns { continue; }
        if b.len() > 1 && &b[1..] == strain {
            declarer_pos = pos;
            break;
        }
    }
    let declarer = match declarer_pos { 0 => "N", 1 => "E", 2 => "S", 3 => "W", _ => "?" };
    (Some(contract), Some(declarer.to_string()))
}

/// Build (result, score, board_hash) when single-dummy analysis is requested.
fn derive_single_dummy_outputs(
    analysis: &Option<epbot_core::SingleDummyAnalysis>,
    contract: Option<&str>,
    declarer: Option<&str>,
    pbn: &str,
    dealer: i32,
    vul: i32,
    board_number: u32,
) -> (Option<u8>, Option<i32>, Option<String>) {
    use epbot_core::bba_hash;
    use epbot_core::score;

    // Hash works whether or not the auction succeeded — depends only on cards.
    let board_hash = parse_pbn_for_hash(pbn).map(|hands| {
        bba_hash::encode(
            &hands,
            dealer as u8,
            vul as u8,
            bba_hash::board_extension_for(board_number),
        )
    });

    let (Some(analysis), Some(contract_str), Some(declarer_str)) = (analysis, contract, declarer)
    else {
        return (None, None, board_hash);
    };

    let Some((level, strain, doubled)) = score::parse_contract(contract_str) else {
        return (None, None, board_hash);
    };

    let strain_idx = match strain {
        score::Strain::Clubs => 0,
        score::Strain::Diamonds => 1,
        score::Strain::Hearts => 2,
        score::Strain::Spades => 3,
        score::Strain::NoTrump => 4,
    };
    let tricks = analysis.tricks[strain_idx];

    let declarer_pos = match declarer_str { "N" => 0, "E" => 1, "S" => 2, "W" => 3, _ => 0 };
    let ns_score = score::score_for_ns(level, strain, doubled, declarer_pos, vul as u8, tricks);
    (Some(tricks), Some(ns_score), board_hash)
}

/// Parse the PBN deal string into the per-player suits expected by
/// `bba_hash::encode`. Returns None if the input is malformed.
fn parse_pbn_for_hash(pbn: &str) -> Option<[epbot_core::bba_hash::HandSuits; 4]> {
    use epbot_core::bba_hash::HandSuits;
    let colon = pbn.find(':')?;
    let first_seat = match pbn[..colon].trim_end().chars().last()? {
        'N' | 'n' => 0,
        'E' | 'e' => 1,
        'S' | 's' => 2,
        'W' | 'w' => 3,
        _ => return None,
    };
    let parts: Vec<&str> = pbn[colon + 1..].split_whitespace().collect();
    if parts.len() != 4 { return None; }
    let mut hands: [HandSuits; 4] = Default::default();
    for (i, hand_str) in parts.iter().enumerate() {
        let pos = (first_seat + i) % 4;
        let suits: Vec<&str> = hand_str.split('.').collect();
        if suits.len() != 4 { return None; }
        // PBN order is S.H.D.C; HandSuits expects clubs/diamonds/hearts/spades.
        hands[pos] = HandSuits {
            clubs:    normalize_suit_chars(suits[3]),
            diamonds: normalize_suit_chars(suits[2]),
            hearts:   normalize_suit_chars(suits[1]),
            spades:   normalize_suit_chars(suits[0]),
        };
    }
    Some(hands)
}

fn normalize_suit_chars(s: &str) -> String {
    // bba_hash uses 'T' for the ten; PBN strings already use 'T' but be defensive.
    s.replace("10", "T")
}

fn format_alerts(meanings: &[BidMeaning]) -> String {
    meanings
        .iter()
        .filter(|m| m.is_alert && m.meaning.is_some())
        .map(|m| format!("{}={}", m.bid, m.meaning.as_deref().unwrap_or("")))
        .collect::<Vec<_>>()
        .join("; ")
}
