// One-shot probe: run David's Fourth_Suit_Forcing Board 188 with SD analysis
// and dump the raw single-dummy table EPBot returns. Use to confirm the
// shape/order of the trick + percentage arrays before wiring downstream.

fn main() {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();
    let pbn = "N:AJ64.T72.A3.KJ98 Q95.J86.J64.T542 K32.AQ953.KQ72.7 T87.K4.T985.AQ63";
    let result = epbot_core::generate_auction_with_options(
        pbn,
        0, // dealer N
        2, // vul NS
        epbot_core::Scoring::Matchpoints,
        None,
        None,
        None,
        true,
        false,
    );
    println!("success: {}", result.success);
    println!(
        "auction: {}",
        result
            .bids
            .iter()
            .map(|b| b.bid.as_str())
            .collect::<Vec<_>>()
            .join(" ")
    );
    if let Some(a) = result.analysis {
        println!("SD tricks (C, D, H, S, NT): {:?}", a.tricks);
        println!("SD pct    (C, D, H, S, NT): {:?}", a.percentages);
    } else {
        println!("no analysis: {:?}", result.error);
    }
}
