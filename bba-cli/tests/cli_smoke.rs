//! Smoke test: run `bba-cli --single-dummy` against the curated fixture deals
//! and compare to `tests/fixtures/expected/deals-with-sd.pbn`.
//!
//! This catches regressions in any of the recently-added single-dummy plumbing:
//! BBA hash encoding, score derivation, SD trick lookup, or PBN writer changes.
//! It also exercises the cross-platform dynamic-loader path setup.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitStatus};

fn manifest_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn fixture_path(rel: &str) -> PathBuf {
    let mut p = manifest_dir();
    p.pop(); // bba-cli -> repo root
    p.push("tests/fixtures");
    p.push(rel);
    p
}

fn epbot_libs_dir() -> PathBuf {
    let mut p = manifest_dir();
    p.pop(); // repo root
    let triple = if cfg!(all(target_os = "macos", target_arch = "aarch64")) {
        "macos/arm64"
    } else if cfg!(all(target_os = "linux", target_arch = "x86_64")) {
        "linux/x64"
    } else if cfg!(all(target_os = "linux", target_arch = "aarch64")) {
        "linux/arm64"
    } else if cfg!(all(target_os = "windows", target_arch = "x86_64")) {
        "windows/x64"
    } else {
        "unsupported"
    };
    p.push("epbot-libs");
    p.push(triple);
    p
}

/// Drop the one thing that depends on the machine rather than the algorithm:
/// the absolute paths to the convention files. bba-cli writes no `[Date]` of
/// its own, so output is otherwise reproducible byte for byte.
fn normalize(s: &str) -> String {
    s.lines()
        .filter(|l| !l.starts_with("% CC1 ") && !l.starts_with("% CC2 "))
        .collect::<Vec<_>>()
        .join("\n")
}

/// Run bba-cli on `input` with both sides on 21GF-DEFAULT, writing `output`.
fn run_bba_cli(input: &Path, output: &Path, extra: &[&str]) -> ExitStatus {
    // Cross-platform dynamic-loader env. Windows finds the dll via PATH.
    let lib_var = if cfg!(target_os = "macos") {
        "DYLD_LIBRARY_PATH"
    } else if cfg!(target_os = "linux") {
        "LD_LIBRARY_PATH"
    } else {
        "PATH"
    };
    let card = fixture_path("21GF-DEFAULT.bbsa");
    Command::new(env!("CARGO_BIN_EXE_bba-cli"))
        .env(lib_var, epbot_libs_dir())
        .args(["--input", input.to_str().unwrap()])
        .args(["--output", output.to_str().unwrap()])
        .args(["--ns-conventions", card.to_str().unwrap()])
        .args(["--ew-conventions", card.to_str().unwrap()])
        .args(extra)
        .status()
        .expect("failed to spawn bba-cli")
}

fn temp_path(name: &str) -> PathBuf {
    let path = std::env::temp_dir().join(format!("bba-cli-smoke-{name}.pbn"));
    let _ = fs::remove_file(&path);
    path
}

/// Spawn bba-cli on `input` with single-dummy enabled and compare its
/// normalized output to `golden`. Panics on divergence with a diff path.
fn run_and_compare(label: &str, input: PathBuf, golden: PathBuf) {
    let tmp = temp_path(label);
    let status = run_bba_cli(&input, &tmp, &["--single-dummy"]);
    assert!(status.success(), "bba-cli ({label}) exited with {status}");

    let actual = fs::read_to_string(&tmp).expect("read produced PBN");
    let expected = fs::read_to_string(&golden).expect("read golden PBN");

    let actual_n = normalize(&actual);
    let expected_n = normalize(&expected);

    if actual_n != expected_n {
        let diff_path = std::env::temp_dir().join(format!("bba-cli-smoke-{label}-diff.txt"));
        let _ = fs::write(
            &diff_path,
            format!("--- expected ---\n{expected_n}\n\n--- actual ---\n{actual_n}\n"),
        );
        panic!(
            "{label}: fixture output diverged from golden. See {} for full content.",
            diff_path.display()
        );
    }
}

/// Fast smoke test: 8 curated deals across all dealer/vul combos. Catches
/// any structural regression (hash, score, SD, tag emission). Runs every
/// `cargo test`.
#[test]
fn fixture_deals_with_single_dummy_match_golden() {
    run_and_compare(
        "fast",
        fixture_path("deals.pbn"),
        fixture_path("expected/deals-with-sd.pbn"),
    );
}

/// Slow regression test: 500-board PBN. Together with `slow_1N`, covers
/// 1000 deals. Catches subtle bidder drift (e.g., EPBot version bumps) and
/// memory issues that 8 deals can't surface. Excluded from default test
/// runs because it adds ~3s; opt in with `cargo test -- --ignored`.
#[test]
#[ignore = "slow: 500-board fixture, run with --ignored"]
fn slow_fourth_suit_forcing() {
    run_and_compare(
        "slow-fsf",
        fixture_path("slow/Fourth_Suit_Forcing.pbn"),
        fixture_path("expected/slow/Fourth_Suit_Forcing.pbn"),
    );
}

/// Slow regression test: 500-board 1NT-opening PBN. Different dealer (S)
/// and different bidding shape than `slow_fourth_suit_forcing`.
#[test]
#[ignore = "slow: 500-board fixture, run with --ignored"]
fn slow_one_no_trump() {
    run_and_compare(
        "slow-1n",
        fixture_path("slow/1N.pbn"),
        fixture_path("expected/slow/1N.pbn"),
    );
}

// ---- Editing the input in place (issue #26) --------------------------------

/// Every `[Name "value"]` line of `text` whose tag is `name`.
fn tag_lines<'a>(text: &'a str, name: &str) -> Vec<&'a str> {
    let prefix = format!("[{name} ");
    text.lines().filter(|l| l.starts_with(&prefix)).collect()
}

/// How many `=N=` note references the board's auction rows carry.
fn note_references(board: &str) -> usize {
    board
        .lines()
        .skip_while(|l| !l.starts_with("[Auction "))
        .skip(1)
        .take_while(|l| !l.starts_with('['))
        .flat_map(str::split_whitespace)
        .filter(|t| t.len() > 2 && t.starts_with('=') && t.ends_with('='))
        .count()
}

/// The text's boards, split on blank lines.
fn board_texts(text: &str) -> Vec<&str> {
    text.split("\n\n").filter(|b| b.contains("[Deal ")).collect()
}

/// Bid `leveled.pbn` once, the way the PBS pipeline does.
fn bid_leveled(label: &str) -> (String, PathBuf) {
    let out = temp_path(label);
    let status = run_bba_cli(
        &fixture_path("leveled.pbn"),
        &out,
        &["--single-dummy", "--event", "2N and 1 Minor"],
    );
    assert!(status.success(), "bba-cli ({label}) exited with {status}");
    (fs::read_to_string(&out).expect("read output"), out)
}

/// Tags and lines bba-cli does not own come through the bba step untouched:
/// supplemental tags, a section with its rows, and the input's `%` directives.
#[test]
fn tags_bba_cli_does_not_own_survive_unchanged() {
    let input = fs::read_to_string(fixture_path("leveled.pbn")).unwrap();
    let (output, _) = bid_leveled("keep");

    assert_eq!(tag_lines(&output, "HandType"), tag_lines(&input, "HandType"));
    assert_eq!(tag_lines(&input, "HandType").len(), 3);
    assert!(output.contains(concat!(
        "[OptimumResultTable \"Declarer;Denomination\\2R;Result\\2R\"]\n",
        "N NT 9\n",
        "N S 8\n",
        "S NT 9\n",
    )));
    for directive in [
        "% Three leveled deals from Practice-Bidding-Scenarios",
        "%HRTitleEvent \"2N_and_1_Minor\"",
        "%HRSeed 639810 (offset 1)",
    ] {
        assert_eq!(output.matches(directive).count(), 1, "{directive}");
    }
    // And the auction was still added, once per board.
    assert_eq!(tag_lines(&output, "Auction").len(), 3);
    assert_eq!(tag_lines(&output, "Event"), vec!["[Event \"2N and 1 Minor\"]"; 3]);
}

/// Re-bidding a bba/ file changes nothing when nothing about the bidding has:
/// no tag, note, comment, hash or header line is duplicated or moved.
#[test]
fn re_running_on_its_own_output_is_byte_identical() {
    let (first, first_path) = bid_leveled("rerun-1");
    let second_path = temp_path("rerun-2");
    let status = run_bba_cli(
        &first_path,
        &second_path,
        &["--single-dummy", "--event", "2N and 1 Minor"],
    );
    assert!(status.success());
    assert_eq!(fs::read_to_string(&second_path).unwrap(), first);
}

/// A different auction replaces the old one wholesale: one `[Auction]` per
/// board, and exactly the notes the new auction references — none left over
/// from the old one.
#[test]
fn re_bidding_replaces_the_auction_and_every_note() {
    let (first, first_path) = bid_leveled("rebid-1");
    let second_path = temp_path("rebid-2");
    // Forcing three opening passes gives every board a different auction.
    let status = run_bba_cli(
        &first_path,
        &second_path,
        &["--single-dummy", "--auction-prefix", "Pass Pass Pass"],
    );
    assert!(status.success());
    let second = fs::read_to_string(&second_path).unwrap();
    assert_ne!(tag_lines(&second, "Auction"), Vec::<&str>::new());

    for (old_board, board) in board_texts(&first).iter().zip(board_texts(&second)) {
        assert_eq!(tag_lines(board, "Auction").len(), 1);
        assert_ne!(
            old_board.split("[Auction ").nth(1),
            board.split("[Auction ").nth(1),
            "the prefix should have changed the auction"
        );
        let refs = note_references(board);
        let notes = tag_lines(board, "Note");
        assert_eq!(notes.len(), refs, "notes must match the new auction's references");
        for (i, note) in notes.iter().enumerate() {
            assert!(note.starts_with(&format!("[Note \"{}:", i + 1)), "{note}");
        }
        for tag in ["Declarer", "Contract", "Result", "Score", "Scoring", "HandType"] {
            assert!(tag_lines(board, tag).len() <= 1, "{tag} duplicated");
        }
        assert_eq!(board.matches("{HCP ").count(), 1);
    }
}

/// A board that fails to bid this time must not keep the previous run's
/// auction: it would read as a perfectly good result for a deal that failed.
#[test]
fn a_board_that_fails_to_bid_loses_its_old_auction() {
    let (_, first_path) = bid_leveled("fail-1");
    let second_path = temp_path("fail-2");
    // An invalid prefix fails every board; bba-cli writes the file, then exits
    // non-zero because nothing bid.
    let status = run_bba_cli(&first_path, &second_path, &["--auction-prefix", "9Z"]);
    assert!(!status.success(), "a run that bid nothing must fail");
    let second = fs::read_to_string(&second_path).unwrap();

    for tag in ["Auction", "Note", "Declarer", "Contract", "Result", "Score"] {
        assert_eq!(tag_lines(&second, tag), Vec::<&str>::new(), "stale {tag}");
    }
    assert!(!second.contains(" =1="), "stale auction rows");
    // What bba-cli does not own is still there.
    assert_eq!(tag_lines(&second, "HandType").len(), 3);
    assert!(second.contains("[OptimumResultTable "));
}

