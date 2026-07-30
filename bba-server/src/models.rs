use serde::{Deserialize, Serialize};

/// Request to generate an auction for a deal.
#[derive(Deserialize)]
pub struct AuctionRequest {
    pub deal: DealInfo,
    pub scenario: Option<String>,
    pub conventions: Option<ConventionCards>,
    /// Optional forced bid sequence to use for the first N positions of the
    /// auction. Each entry must be one of: "Pass", "X", "XX", or
    /// {1-7}{C|D|H|S|NT}. Used by "what if I had bid X" practice flows.
    #[serde(default, alias = "auctionPrefix")]
    pub auction_prefix: Option<Vec<String>>,
    /// When true, also compute single-dummy analysis and include
    /// `result`, `score`, and `boardHash` in the response. Off by default
    /// because it adds latency.
    #[serde(default, alias = "singleDummy")]
    pub single_dummy: bool,
    /// When true, `meanings[]` carries `meaning`/`meaningExtended` for every
    /// bid instead of only alertable ones. Off by default — it roughly triples
    /// the response size, and the browser extensions only render alerts.
    #[serde(default, alias = "includeAllMeanings")]
    pub include_all_meanings: bool,
    /// Optional PBN [Board] number, used to derive the board-id hash's
    /// `board_extension` nibble when `singleDummy` is true. Defaults to 1.
    #[serde(default, alias = "boardNumber")]
    pub board_number: Option<u32>,
}

/// Deal information in PBN format.
#[derive(Deserialize)]
pub struct DealInfo {
    pub pbn: String,
    pub dealer: String,
    pub vulnerability: String,
    #[serde(default = "default_scoring")]
    pub scoring: String,
}

fn default_scoring() -> String {
    "MP".to_string()
}

/// Convention card specifications.
#[derive(Deserialize, Serialize, Clone)]
pub struct ConventionCards {
    #[serde(default = "default_ns_card")]
    pub ns: String,
    #[serde(default = "default_ew_card")]
    pub ew: String,
}

fn default_ns_card() -> String {
    "21GF-DEFAULT".to_string()
}
fn default_ew_card() -> String {
    "21GF-GIB".to_string()
}

impl Default for ConventionCards {
    fn default() -> Self {
        Self {
            ns: default_ns_card(),
            ew: default_ew_card(),
        }
    }
}

/// Response from auction generation.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AuctionResponse {
    pub success: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub auction: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub auction_encoded: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub conventions_used: Option<ConventionCards>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub meanings: Option<Vec<BidMeaning>>,
    /// Final contract (e.g. "4H", "3NT", "5CX"). Present whenever the
    /// auction produced a contract.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub contract: Option<String>,
    /// Declarer seat ("N", "E", "S", "W"). Present whenever the auction
    /// produced a contract.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub declarer: Option<String>,
    /// Single-dummy estimated tricks for the contract's strain. Only set
    /// when the request included `singleDummy: true`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<u8>,
    /// Score from NS perspective for the contract+result+vul. Only set when
    /// the request included `singleDummy: true`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub score: Option<i32>,
    /// 28-hex BBA-style board fingerprint. Only set when the request
    /// included `singleDummy: true`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub board_hash: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// Meaning of a bid from EPBot.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BidMeaning {
    pub position: usize,
    pub bid: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub meaning: Option<String>,
    /// The longer/detailed meaning (from EPBot info_meaning_extended).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub meaning_extended: Option<String>,
    pub is_alert: bool,
}

/// Request to record a scenario selection.
#[derive(Deserialize)]
pub struct ScenarioSelectRequest {
    pub scenario: Option<String>,
}
