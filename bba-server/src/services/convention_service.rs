use regex::Regex;
use tracing::{info, warn};

/// Fetches convention cards (.bbsa) and scenario files (.dlr) from GitHub.
pub struct ConventionService {
    client: reqwest::Client,
    github_raw_base_url: String,
    default_ns_card: String,
    default_ew_card: String,
}

impl ConventionService {
    pub fn new(github_raw_base_url: &str, default_ns_card: &str, default_ew_card: &str) -> Self {
        Self {
            client: reqwest::Client::new(),
            github_raw_base_url: github_raw_base_url.to_string(),
            default_ns_card: default_ns_card.to_string(),
            default_ew_card: default_ew_card.to_string(),
        }
    }

    pub fn default_ns(&self) -> &str {
        &self.default_ns_card
    }

    pub fn default_ew(&self) -> &str {
        &self.default_ew_card
    }

    /// Fetch a .bbsa convention card from GitHub and return its content.
    pub async fn get_bbsa_content(&self, card_name: &str) -> Result<String, String> {
        let url = format!("{}/bbsa/{}.bbsa", self.github_raw_base_url, card_name);
        info!("Fetching convention card from GitHub: {}", url);

        let resp = self
            .client
            .get(&url)
            .send()
            .await
            .map_err(|e| format!("Failed to fetch convention card: {}", e))?;

        if !resp.status().is_success() {
            return Err(format!(
                "Convention card not found on GitHub: {} (HTTP {})",
                card_name,
                resp.status()
            ));
        }

        resp.text()
            .await
            .map_err(|e| format!("Failed to read convention card: {}", e))
    }

    /// Get conventions for a scenario by fetching its .dlr file from GitHub.
    pub async fn get_conventions_for_scenario(
        &self,
        scenario: &str,
    ) -> (String, String) {
        match self.get_convention_cards_from_dlr(scenario).await {
            (Some(ns), Some(ew)) => (ns, ew),
            (Some(ns), None) => (ns, self.default_ew_card.clone()),
            (None, Some(ew)) => (self.default_ns_card.clone(), ew),
            (None, None) => (self.default_ns_card.clone(), self.default_ew_card.clone()),
        }
    }

    async fn get_convention_cards_from_dlr(&self, scenario: &str) -> (Option<String>, Option<String>) {
        let url = format!("{}/dlr/{}.dlr", self.github_raw_base_url, scenario);

        let resp = match self.client.get(&url).send().await {
            Ok(r) => r,
            Err(e) => {
                warn!("Error fetching dlr file for {}: {}", scenario, e);
                return (None, None);
            }
        };

        if !resp.status().is_success() {
            warn!("dlr file not found for {} (HTTP {})", scenario, resp.status());
            return (None, None);
        }

        let content = match resp.text().await {
            Ok(c) => c,
            Err(_) => return (None, None),
        };

        let (ns, ew) = parse_convention_cards(&content);

        if ns.is_some() || ew.is_some() {
            info!(
                "Found convention cards NS='{}', EW='{}' for scenario '{}'",
                ns.as_deref().unwrap_or("(default)"),
                ew.as_deref().unwrap_or("(default)"),
                scenario
            );
        } else {
            info!("No convention card specified for scenario '{}', using defaults", scenario);
        }
        (ns, ew)
    }

    /// Get the list of available scenarios from GitHub.
    pub async fn get_scenario_list(&self) -> Result<Vec<String>, String> {
        let url =
            "https://api.github.com/repos/ADavidBailey/Practice-Bidding-Scenarios/contents/dlr";

        let resp = self
            .client
            .get(url)
            .header("User-Agent", "BBA-Server")
            .send()
            .await
            .map_err(|e| format!("Failed to fetch scenario list: {}", e))?;

        if !resp.status().is_success() {
            return Err(format!("GitHub API returned {}", resp.status()));
        }

        let items: Vec<serde_json::Value> = resp
            .json()
            .await
            .map_err(|e| format!("Failed to parse GitHub response: {}", e))?;

        let mut scenarios: Vec<String> = items
            .iter()
            .filter_map(|item| {
                let name = item["name"].as_str()?;
                name.strip_suffix(".dlr").map(str::to_string)
            })
            .collect();

        scenarios.sort();
        Ok(scenarios)
    }
}

/// Read the cards from a .dlr file's header, where they are comment lines:
/// `# convention-card-ns: 21GF-SPECIALS2`.
fn parse_convention_cards(content: &str) -> (Option<String>, Option<String>) {
    let card = |side: &str| {
        let re = Regex::new(&format!(r"(?mi)^#\s*convention-card-{side}:\s*(.+)$")).unwrap();
        re.captures(content)
            .map(|c| c[1].trim().to_string())
            .filter(|name| !name.is_empty())
    };
    (card("ns"), card("ew"))
}

#[cfg(test)]
mod tests {
    use super::parse_convention_cards;

    #[test]
    fn reads_both_cards_from_a_dlr_header() {
        let dlr = "# alias: WalshStyle\n\
                   # convention-card-ns: 21GF-SPECIALS2\n\
                   # convention-card-ew: 21GF-GIB\n\
                   \n\
                   /*@chat\n";
        assert_eq!(
            parse_convention_cards(dlr),
            (Some("21GF-SPECIALS2".to_string()), Some("21GF-GIB".to_string()))
        );
    }

    #[test]
    fn tolerates_crlf_line_endings() {
        let dlr = "# convention-card-ns: Precision\r\n# convention-card-ew: Basic-Bridge\r\n";
        assert_eq!(
            parse_convention_cards(dlr),
            (Some("Precision".to_string()), Some("Basic-Bridge".to_string()))
        );
    }

    #[test]
    fn a_side_with_no_line_is_none() {
        assert_eq!(
            parse_convention_cards("# convention-card-ns: Precision\n"),
            (Some("Precision".to_string()), None)
        );
        assert_eq!(parse_convention_cards("/*@chat\n*/\n"), (None, None));
    }
}
