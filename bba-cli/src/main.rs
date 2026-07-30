//! BBA-CLI: Bridge Bidding Analyzer Command Line Interface
//!
//! Generates bridge auctions for deals in PBN files using the native EPBot engine.
//! Cross-platform: macOS, Linux, and Windows.

use anyhow::{Context, Result};
use clap::Parser;
use log::{debug, error, info, warn};
use std::path::PathBuf;

mod batch;

use batch::{process_pbn_file, OutputConfig};

/// Bridge Bidding Analyzer CLI
///
/// Generates bridge auctions for deals in PBN files using the EPBot engine.
#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
#[command(propagate_version = true)]
struct Args {
    /// Input PBN file containing deals to analyze
    #[arg(short, long, value_name = "FILE")]
    input: PathBuf,

    /// Output PBN file for results with generated auctions
    #[arg(short, long, value_name = "FILE")]
    output: PathBuf,

    /// Convention file (.bbsa) for North-South partnership
    #[arg(long = "ns-conventions", value_name = "FILE")]
    ns_conventions: PathBuf,

    /// Convention file (.bbsa) for East-West partnership
    #[arg(long = "ew-conventions", value_name = "FILE")]
    ew_conventions: PathBuf,

    /// Event name for PBN output
    #[arg(long, default_value = "")]
    event: String,

    /// Override the N-S system name in the BidSystemNS tag. Defaults to the
    /// name EPBot reports for the .bbsa's `System type`.
    #[arg(long = "ns-system-name")]
    ns_system_name: Option<String>,

    /// Override the E-W system name in the BidSystemEW tag. Defaults to the
    /// name EPBot reports for the .bbsa's `System type`.
    #[arg(long = "ew-system-name")]
    ew_system_name: Option<String>,

    /// Enable verbose logging (use -vv for debug output)
    #[arg(short, long, action = clap::ArgAction::Count)]
    verbose: u8,

    /// Dry run - parse input but don't write output
    #[arg(long, default_value_t = false)]
    dry_run: bool,

    /// Force the first N bids of every auction (whitespace-separated, e.g.
    /// "1C Pass 1H Pass"). Each token must be Pass, X, XX, or {1-7}{C|D|H|S|NT}.
    /// EPBot resumes normal bidding after the prefix. Useful for "what if it had
    /// gone X" practice and for A/B testing alongside bba-server.
    #[arg(long = "auction-prefix", value_name = "BIDS")]
    auction_prefix: Option<String>,

    /// Compute single-dummy analysis after each auction. Adds [Result], [Score],
    /// [Scoring], and a board-id hash comment to the PBN output. Off by default;
    /// adds roughly 0.22 ms per board (~3-4% on a 500-board file).
    #[arg(long = "single-dummy", default_value_t = false)]
    single_dummy: bool,

    /// Scoring mode for the auction. Affects [Score] computation and the
    /// [Scoring] tag.
    #[arg(long, value_name = "MODE", default_value = "MP", value_parser = parse_scoring_arg)]
    scoring: epbot_core::Scoring,
}

fn parse_scoring_arg(s: &str) -> std::result::Result<epbot_core::Scoring, String> {
    match s.to_uppercase().as_str() {
        "MP" | "MATCHPOINTS" => Ok(epbot_core::Scoring::Matchpoints),
        "IMP" | "IMPS" => Ok(epbot_core::Scoring::Imps),
        other => Err(format!("unknown scoring mode '{}'; expected MP or IMP", other)),
    }
}

fn main() -> Result<()> {
    let args = Args::parse();

    let log_level = match args.verbose {
        0 => "info",
        1 => "debug",
        _ => "trace",
    };

    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or(log_level))
        .format_timestamp_millis()
        .init();

    // Probe EPBot at startup. If this fails, every per-deal create will fail
    // identically — surface the underlying reason now so callers don't have to
    // dig through per-deal output to learn that the engine is dead.
    match epbot_core::version() {
        Ok(v) => info!("BBA-CLI v{} (EPBot {})", env!("CARGO_PKG_VERSION"), v),
        Err(e) => {
            info!("BBA-CLI v{} (EPBot version check failed)", env!("CARGO_PKG_VERSION"));
            warn!("EPBot startup probe failed: {}", e);
        }
    }

    debug!("Input: {:?}", args.input);
    debug!("Output: {:?}", args.output);
    debug!("NS Conventions: {:?}", args.ns_conventions);
    debug!("EW Conventions: {:?}", args.ew_conventions);

    // Validate input files
    if !args.input.exists() {
        anyhow::bail!("Input file not found: {:?}", args.input);
    }
    if !args.ns_conventions.exists() {
        anyhow::bail!("NS conventions file not found: {:?}", args.ns_conventions);
    }
    if !args.ew_conventions.exists() {
        anyhow::bail!("EW conventions file not found: {:?}", args.ew_conventions);
    }

    let auction_prefix: Option<Vec<String>> = args
        .auction_prefix
        .as_deref()
        .map(|s| s.split_whitespace().map(|t| t.to_string()).collect());

    if let Some(ref bids) = auction_prefix {
        info!("Auction prefix: {} bid(s) — {}", bids.len(), bids.join(" "));
    }

    let config = OutputConfig {
        event: args.event,
        ns_system_name: args.ns_system_name,
        ew_system_name: args.ew_system_name,
        ns_conventions_path: args.ns_conventions.display().to_string(),
        ew_conventions_path: args.ew_conventions.display().to_string(),
        scoring: args.scoring,
        single_dummy: args.single_dummy,
    };

    if args.single_dummy {
        info!("Single-dummy analysis enabled (Result/Score/board-id will be emitted)");
    }

    info!("Processing {:?}...", args.input);

    let stats = process_pbn_file(
        &args.input,
        &args.output,
        &args.ns_conventions,
        &args.ew_conventions,
        args.dry_run,
        &config,
        auction_prefix.as_deref(),
    )
    .context("Failed to process PBN file")?;

    info!(
        "Processed {} deals, generated {} auctions",
        stats.deals_processed, stats.auctions_generated
    );

    if stats.errors > 0 {
        error!("{} deals had errors", stats.errors);
    }

    if args.dry_run {
        info!("Dry run complete - no output written");
    } else {
        info!("Output written to {:?}", args.output);
    }

    // Fail loudly when nothing bid. Previously bba-cli exited 0 even if every
    // deal errored, so a pipeline couldn't tell a healthy run from a dead engine
    // and an empty bba could get committed by mistake. A run that processed deals
    // but produced zero auctions is a failure — most often EPBot construction
    // failing on every deal (see the overflow bug in CLAUDE.md).
    if stats.auctions_generated == 0 {
        anyhow::bail!(
            "No auctions generated ({} deals processed, {} errors) — refusing to report success",
            stats.deals_processed,
            stats.errors
        );
    }

    Ok(())
}
