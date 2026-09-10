//! Editing performance regression benchmark (docs/bug_editing_slow.md).
//!
//! Simulates a fast-typing burst on a synthetic ~50KB Markdown file and guards
//! the per-keystroke `apply_command` + `get_highlights` round-trip against the
//! O(file-size) work that widened the cursor-teleport race window:
//!
//!   * Layer 2 — `apply_command` must stay ~O(1) per keystroke (no full-file
//!     hash, no double char→byte scan). Asserted with a tight, profile-scaled
//!     wall-clock ceiling.
//!   * Layer 3 — Markdown highlighting must reparse *incrementally* from the
//!     cached tree, not from scratch. Asserted relatively (incremental refresh
//!     must beat a cold parse), which is robust across debug/release and CI
//!     noise since it's a ratio, not an absolute.
//!   * A generous, profile-scaled ceiling on the whole round-trip catches any
//!     gross regression (same numeric-guard shape as integration_cost_reduction).
//!
//! The teleport itself (Layer 1) is a CodeMirror-level race and is user/UI
//! validated, not covered here.

use std::path::PathBuf;
use std::time::Instant;
use tracelean_core::commands::Command;
use tracelean_core::parser;
use tracelean_core::service;
use tracelean_core::state::AppState;

/// Realistic Markdown prose (multi-sentence paragraphs, occasional headings and
/// code fences) of at least `target_bytes` bytes. Realistic block sizes matter:
/// a wall of single-word bullet items produces thousands of tiny inline blocks
/// and is not representative of the "semi-big .md file" in the bug report.
fn realistic_markdown(target_bytes: usize) -> String {
    let para = "This is a paragraph of ordinary prose with some **bold** words and a bit of \
_emphasis_, plus an inline `code` reference and a [link](http://example.com) here and there. \
It runs for several sentences so that each inline block spans a realistic amount of text \
rather than a couple of words.\n\n";
    let mut s = String::with_capacity(target_bytes + 1024);
    let mut i = 0;
    while s.len() < target_bytes {
        if i % 5 == 0 {
            s.push_str(&format!("## Heading number {i}\n\n"));
        }
        s.push_str(para);
        if i % 7 == 0 {
            s.push_str("```rust\nfn demo() { let x = 1; println!(\"{}\", x); }\n```\n\n");
        }
        i += 1;
    }
    s
}

/// nth-percentile of a latency sample set (microseconds).
fn percentile(samples: &[u128], pct: f64) -> u128 {
    let mut v = samples.to_vec();
    v.sort_unstable();
    let idx = ((v.len() as f64) * pct).ceil() as usize;
    v[idx.saturating_sub(1).min(v.len() - 1)]
}

fn median(samples: &[u128]) -> u128 {
    percentile(samples, 0.50)
}

/// Debug builds run tree-sitter and the query engine an order of magnitude
/// slower than release; scale wall-clock budgets so the guard never flakes on a
/// plain `cargo test` while still catching real regressions in either profile.
const fn profile_scale() -> u128 {
    if cfg!(debug_assertions) {
        10
    } else {
        1
    }
}

#[test]
fn editing_roundtrip_stays_fast_on_50kb_markdown() {
    let path = PathBuf::from("notes.md");
    let content = realistic_markdown(50 * 1024);
    assert!(content.len() >= 50 * 1024, "fixture must be at least 50KB");

    let mut state = AppState::new();
    state.load_file(path.clone(), content.clone());
    state.record_file_open();

    // Warm the compiled-query and incremental-parse caches so we measure
    // steady-state editing, not the cold first parse.
    let _ = parser::get_highlights_query(&path, state.get_content(&path).unwrap());

    // Append at EOF, mimicking typing at the end of the buffer — the worst case
    // for the char→byte scan the Layer-2 fix removed a second copy of.
    let mut char_len = content.chars().count();

    const EDITS: usize = 100;
    let mut apply_us: Vec<u128> = Vec::with_capacity(EDITS);
    let mut roundtrip_us: Vec<u128> = Vec::with_capacity(EDITS);
    let mut incremental_hl_us: Vec<u128> = Vec::with_capacity(EDITS);
    let mut cold_hl_us: Vec<u128> = Vec::with_capacity(EDITS);

    for n in 0..EDITS {
        // Sprinkle newlines so tree-sitter sees structural edits, not one run.
        let ch = if n % 20 == 19 { '\n' } else { 'x' };
        let cmd = Command::Replace {
            file: path.clone(),
            at: char_len,
            old: String::new(),
            new: ch.to_string(),
        };

        let t_apply = Instant::now();
        service::apply_command(&mut state, cmd).expect("apply_command should succeed");
        apply_us.push(t_apply.elapsed().as_micros());
        char_len += 1;

        let current = state.get_content(&path).unwrap().to_string();

        // Incremental highlight refresh on the *stable* path (cache warm → the
        // Markdown tree is reparsed incrementally from the previous edit).
        let t_hl = Instant::now();
        let _ = parser::get_highlights_query(&path, &current);
        let inc = t_hl.elapsed().as_micros();
        incremental_hl_us.push(inc);
        roundtrip_us.push(apply_us[n] + inc);

        // Cold highlight of the same content on a never-seen path (cache miss →
        // full from-scratch parse). Everything else (query, byte→char map) is
        // identical, so the delta isolates the incremental-parse win (Layer 3).
        let cold_path = PathBuf::from(format!("cold_{n}.md"));
        let t_cold = Instant::now();
        let _ = parser::get_highlights_query(&cold_path, &current);
        cold_hl_us.push(t_cold.elapsed().as_micros());
    }

    // Correctness: the incrementally-maintained tree must produce exactly the
    // same highlights as a from-scratch parse of the identical final content.
    // A broken InputEdit would make incremental faster *and wrong*; this guards
    // against that.
    let final_content = state.get_content(&path).unwrap().to_string();
    let incremental_spans = parser::get_highlights_query(&path, &final_content);
    let cold_spans = parser::get_highlights_query(&PathBuf::from("verify_cold.md"), &final_content);
    assert_eq!(
        incremental_spans, cold_spans,
        "incremental Markdown parse produced different highlights than a cold parse"
    );

    let apply_p99 = percentile(&apply_us, 0.99);
    let rt_p99 = percentile(&roundtrip_us, 0.99);
    let inc_med = median(&incremental_hl_us);
    let cold_med = median(&cold_hl_us);

    eprintln!(
        "50KB md editing: apply p99={}µs | round-trip p99={}µs | highlight median inc={}µs cold={}µs (debug={})",
        apply_p99,
        rt_p99,
        inc_med,
        cold_med,
        cfg!(debug_assertions),
    );

    // Layer 2: applying a keystroke must not scale with file size. A full-file
    // hash or a second char→byte scan would push this well past the ceiling.
    let apply_budget = 3_000 * profile_scale(); // 3ms release / 30ms debug
    assert!(
        apply_p99 < apply_budget,
        "apply_command p99 {}µs exceeded {}µs — per-keystroke work regressed to O(file size)",
        apply_p99,
        apply_budget
    );

    // Layer 3: the incremental Markdown reparse must be cheaper than a cold
    // from-scratch parse. If incrementality breaks (e.g. reverting to a
    // full-document inline parse), the two converge and this trips. Relative,
    // so it's immune to absolute machine/profile speed.
    assert!(
        inc_med < cold_med,
        "incremental highlight median {}µs was not faster than cold {}µs — Markdown reparse is no longer incremental",
        inc_med,
        cold_med
    );

    // Gross round-trip ceiling — generous headroom so CI noise can't flake it,
    // tight enough to catch a catastrophic return to full-file-per-keystroke.
    let rt_budget = 120_000 * profile_scale(); // 120ms release / 1.2s debug
    assert!(
        rt_p99 < rt_budget,
        "round-trip p99 {}µs exceeded {}µs — editing latency regressed",
        rt_p99,
        rt_budget
    );
}
