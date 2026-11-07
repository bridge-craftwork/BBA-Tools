use regex::Regex;
use tracing::{info, warn};

/// The convention cards a scenario implies.
pub struct ScenarioConventions {
    pub ns: String,
    pub ew: String,
    /// False when the scenario's .pbs file could not be read, so both cards are
    /// defaults standing in for whatever it would have said. A scenario that was
    /// read and names no card is resolved: the defaults are what it implies.
    pub resolved: bool,
}

/// Fetches convention cards (.bbsa) and scenario files (.pbs) from GitHub.
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

    /// Get conventions for a scenario by fetching its .pbs file from GitHub.
    pub async fn get_conventions_for_scenario(
        &self,
        scenario: &str,
    ) -> ScenarioConventions {
        let cards = self.get_convention_cards_from_pbs(scenario).await;
        let resolved = cards.is_some();
        let (ns, ew) = cards.unwrap_or((None, None));
        ScenarioConventions {
            ns: ns.unwrap_or_else(|| self.default_ns_card.clone()),
            ew: ew.unwrap_or_else(|| self.default_ew_card.clone()),
            resolved,
        }
    }

    /// The cards a scenario's .pbs file names, or `None` if the file could not
    /// be read.
    async fn get_convention_cards_from_pbs(
        &self,
        scenario: &str,
    ) -> Option<(Option<String>, Option<String>)> {
        let url = format!("{}/pbs-release/{}.pbs", self.github_raw_base_url, scenario);

        let resp = match self.client.get(&url).send().await {
            Ok(r) => r,
            Err(e) => {
                warn!("Error fetching PBS file for {}: {}", scenario, e);
                return None;
            }
        };

        if !resp.status().is_success() {
            warn!("PBS file not found for {} (HTTP {})", scenario, resp.status());
            return None;
        }

        let content = match resp.text().await {
            Ok(c) => c,
            Err(_) => return None,
        };

        // Try new format: convention-card-ns and convention-card-ew
        let ns_re = Regex::new(r"(?mi)^convention-card-ns:\s*(.+)$").unwrap();
        let ew_re = Regex::new(r"(?mi)^convention-card-ew:\s*(.+)$").unwrap();

        let ns = ns_re.captures(&content).map(|c| c[1].trim().to_string());
        let ew = ew_re.captures(&content).map(|c| c[1].trim().to_string());

        if ns.is_some() || ew.is_some() {
            info!(
                "Found convention cards NS='{}', EW='{}' for scenario '{}'",
                ns.as_deref().unwrap_or("(default)"),
                ew.as_deref().unwrap_or("(default)"),
                scenario
            );
            return Some((ns, ew));
        }

        // Fall back to old format: convention-card (NS only)
        let old_re = Regex::new(r"(?mi)^#?\s*convention-card:\s*(.+)$").unwrap();
        if let Some(caps) = old_re.captures(&content) {
            let value = caps[1].trim().to_string();
            if !value.is_empty() {
                info!(
                    "Found convention card '{}' (old format) for scenario '{}'",
                    value, scenario
                );
                return Some((Some(value), None));
            }
        }

        info!("No convention card specified for scenario '{}', using defaults", scenario);
        Some((None, None))
    }

    /// Get the list of available scenarios from GitHub.
    pub async fn get_scenario_list(&self) -> Result<Vec<String>, String> {
        let url =
            "https://api.github.com/repos/ADavidBailey/Practice-Bidding-Scenarios/contents/pbs-release";

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
                if name.ends_with(".pbs") {
                    Some(name.trim_end_matches(".pbs").to_string())
                } else {
                    None
                }
            })
            .collect();

        scenarios.sort();
        Ok(scenarios)
    }
}

#[cfg(test)]
mod scenario_tests {
    use super::*;
    use axum::routing::get;
    use axum::Router;

    /// A stand-in for the raw GitHub host, serving two .pbs files.
    async fn pbs_host() -> String {
        let app = Router::new()
            .route(
                "/pbs-release/Named.pbs",
                get(|| async { "convention-card-ns: NS-Card\nconvention-card-ew: EW-Card\n" }),
            )
            .route(
                "/pbs-release/OldFormat.pbs",
                get(|| async { "# convention-card: Old-Card\n" }),
            )
            .route(
                "/pbs-release/NoCards.pbs",
                get(|| async { "dealer north\n" }),
            );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move { axum::serve(listener, app).await });
        format!("http://{addr}")
    }

    fn service(base: &str) -> ConventionService {
        ConventionService::new(base, "Default-NS", "Default-EW")
    }

    #[tokio::test]
    async fn a_scenario_that_names_its_cards_is_resolved() {
        let got = service(&pbs_host().await)
            .get_conventions_for_scenario("Named")
            .await;
        assert_eq!((got.ns.as_str(), got.ew.as_str()), ("NS-Card", "EW-Card"));
        assert!(got.resolved);
    }

    #[tokio::test]
    async fn a_scenario_naming_only_one_card_is_resolved() {
        let got = service(&pbs_host().await)
            .get_conventions_for_scenario("OldFormat")
            .await;
        assert_eq!((got.ns.as_str(), got.ew.as_str()), ("Old-Card", "Default-EW"));
        assert!(got.resolved);
    }

    #[tokio::test]
    async fn a_scenario_that_names_no_card_is_resolved_to_the_defaults() {
        let got = service(&pbs_host().await)
            .get_conventions_for_scenario("NoCards")
            .await;
        assert_eq!((got.ns.as_str(), got.ew.as_str()), ("Default-NS", "Default-EW"));
        assert!(got.resolved);
    }

    #[tokio::test]
    async fn a_scenario_with_no_file_is_not_resolved() {
        let got = service(&pbs_host().await)
            .get_conventions_for_scenario("No_Such_Scenario")
            .await;
        assert_eq!((got.ns.as_str(), got.ew.as_str()), ("Default-NS", "Default-EW"));
        assert!(!got.resolved);
    }

    #[tokio::test]
    async fn an_unreachable_host_is_not_resolved() {
        let got = service("http://127.0.0.1:1")
            .get_conventions_for_scenario("Named")
            .await;
        assert!(!got.resolved);
    }
}
