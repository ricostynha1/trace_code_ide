use std::path::{Path, PathBuf};
use std::process::Command;

fn copy(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).unwrap();
    for entry in std::fs::read_dir(from).unwrap().flatten() {
        let name = entry.file_name();
        if name == "target" {
            continue;
        }
        if entry.path().is_dir() {
            copy(&entry.path(), &to.join(&name));
        } else {
            std::fs::copy(entry.path(), to.join(&name)).unwrap();
        }
    }
}

fn demo(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("tracelean-cli-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    copy(&Path::new(env!("CARGO_MANIFEST_DIR")).join("../../demo"), &dir);
    dir
}

struct Ran {
    code: i32,
    out: String,
    err: String,
}

fn trace(dir: &Path, args: &[&str]) -> Ran {
    let out = Command::new(env!("CARGO_BIN_EXE_tracelean-trace")).arg(dir).args(args).output().unwrap();
    Ran {
        code: out.status.code().unwrap_or(-1),
        out: String::from_utf8_lossy(&out.stdout).to_string(),
        err: String::from_utf8_lossy(&out.stderr).to_string(),
    }
}

fn git(dir: &Path, args: &[&str]) {
    let done = Command::new("git")
        .args(["-c", "user.name=t", "-c", "user.email=t@t", "-c", "commit.gpgsign=false"])
        .args(args)
        .current_dir(dir)
        .output()
        .unwrap();
    assert!(done.status.success(), "{}", String::from_utf8_lossy(&done.stderr));
}

fn committed(name: &str) -> PathBuf {
    let dir = demo(name);
    git(&dir, &["init", "-q"]);
    git(&dir, &["add", "."]);
    git(&dir, &["commit", "-q", "-m", "base"]);
    dir
}

fn edit(path: &Path, from: &str, to: &str) {
    let text = std::fs::read_to_string(path).unwrap();
    assert!(text.contains(from), "{from} is not in {}", path.display());
    std::fs::write(path, text.replacen(from, to, 1)).unwrap();
}

/// @tests REQ-CHECK.named_kinds
/// @tests REQ-DOCLINK.hash_moves_review
/// @tests REQ-DOCLINK.dangling_reported
#[test]
fn the_listing_names_requirements_links_documents_and_findings_by_kind() {
    let dir = demo("listing");
    std::fs::create_dir_all(dir.join("docs")).unwrap();
    std::fs::write(
        dir.join("docs/thermo.md"),
        "---\ndescribes: [REQ-THERMO, REQ-GONE]\ndescribed_hash:\n  REQ-THERMO: 0000000000000000\n  REQ-GONE: 0000000000000000\n---\n\n# Thermo\n",
    )
    .unwrap();
    let ran = trace(&dir, &["--show", "Unbound", "--today", "2026-10-26"]);
    assert_eq!(ran.code, 0, "{}", ran.err);
    for wanted in [
        "== requirements (2) ==",
        "REQ-THERMO",
        "== links (",
        "@models      REQ-THERMO.to_celsius",
        "== documents (2) ==",
        "docs/thermo.md -> REQ-THERMO: in-review",
        "docs/thermo.md -> REQ-GONE: dangling",
        "== findings (",
        "== Unbound ==",
        "REQ-THERMO.to_celsius",
        "blocking:",
    ] {
        assert!(ran.out.contains(wanted), "no `{wanted}` in\n{}", ran.out);
    }
    let hashes = trace(&dir, &["--hashes"]);
    let line = hashes.out.lines().find(|l| l.starts_with("REQ-THERMO\t")).expect("REQ-THERMO is hashed");
    assert_eq!(line.split('\t').nth(1).map(str::len), Some(16), "{line}");
    let _ = std::fs::remove_dir_all(&dir);
}

/// @tests REQ-CONTEXT.from_the_shell
#[test]
fn the_context_of_a_clause_is_printed_and_an_unknown_one_is_refused() {
    let dir = demo("context");
    let ran = trace(&dir, &["--context", "REQ-THERMO.to_celsius"]);
    assert_eq!(ran.code, 0, "{}", ran.err);
    assert!(ran.out.contains("REQ-THERMO.to_celsius") && ran.out.contains("toCelsius"), "{}", ran.out);
    let unknown = trace(&dir, &["--context", "REQ-NOPE.x"]);
    assert_eq!(unknown.code, 2, "{}", unknown.out);
    assert!(!unknown.err.is_empty());
    let _ = std::fs::remove_dir_all(&dir);
}

/// @tests REQ-JUDGE.human_decides
/// @tests REQ-JUDGE.prompt_exported
/// @tests REQ-JUDGE.drift_recorded
/// @tests REQ-JUDGE.proposal_not_mutation
#[test]
fn a_judgement_needs_a_clause_a_model_a_verdict_and_a_name() {
    let dir = demo("judge");
    let judge = |args: &[&str]| {
        let mut all = vec!["--judge"];
        all.extend_from_slice(args);
        trace(&dir, &all)
    };
    assert!(judge(&[]).err.contains("--judge needs a clause"));
    assert!(judge(&["REQ-TABLE.every_sample"]).err.contains("nothing models REQ-TABLE.every_sample"));
    let prompt = judge(&["REQ-THERMO.to_celsius"]);
    assert!(prompt.out.contains("toCelsius") && prompt.out.contains("Celsius"), "{}", prompt.out);
    assert!(judge(&["REQ-THERMO.to_celsius", "--verdict", "agrees"]).err.contains("--verdict needs --by"));
    assert!(judge(&["REQ-THERMO.to_celsius", "--verdict", "maybe", "--by", "x"]).err.contains("`maybe` is not a verdict"));
    let stranger = judge(&["REQ-THERMO.to_celsius", "--verdict", "agrees", "--by", "stranger"]);
    assert_eq!(stranger.code, 2, "{}", stranger.out);
    assert!(stranger.err.starts_with("refused:"), "{}", stranger.err);

    let by = ["--by", "claude-review", "--delegated-by", "ricostynha", "--note", "checked"];
    let agrees = judge(&[&["REQ-THERMO.to_celsius", "--verdict", "agrees"][..], &by].concat());
    assert!(agrees.out.contains("REQ-THERMO.to_celsius: agrees, recorded at L2"), "{}{}", agrees.out, agrees.err);
    let drift = judge(&[&["REQ-THERMO.to_fahrenheit", "--verdict", "drift"][..], &by].concat());
    assert!(drift.out.contains("drift recorded at L1"), "{}{}", drift.out, drift.err);
    let unmodelable = judge(&[&["REQ-THERMO.to_fahrenheit", "--verdict", "unmodelable"][..], &by].concat());
    assert!(unmodelable.out.contains("unmodelable, recorded at L1, and a proposal"), "{}{}", unmodelable.out, unmodelable.err);
    let held = std::fs::read_to_string(dir.join(".tracelean/trace.lock")).unwrap();
    assert!(held.contains("claude-review"), "the lock does not carry the judgement");

    std::fs::remove_file(dir.join(".tracelean/judges.json")).unwrap();
    let unlisted = judge(&["REQ-THERMO.to_celsius", "--verdict", "agrees", "--by", "someone"]);
    assert!(unlisted.out.contains("no .tracelean/judges.json"), "{}", unlisted.out);
    std::fs::write(dir.join(".tracelean/judges.json"), "{ not json").unwrap();
    assert_eq!(judge(&["REQ-THERMO.to_celsius", "--verdict", "agrees", "--by", "someone"]).code, 2);
    let _ = std::fs::remove_dir_all(&dir);
}

/// @tests REQ-DIFF.base_from_history
/// @tests REQ-DIFF.text_shown
/// @tests REQ-REPORT.static_page
/// @tests REQ-REPORT.changes_marked
#[test]
fn the_difference_and_the_report_are_taken_against_a_revision() {
    let dir = committed("diff");
    edit(&dir.join("reqs/REQ-THERMO.md"), "to_celsius:", "to_celsius: Rounded,");
    let text = trace(&dir, &["--diff", "HEAD"]);
    assert_eq!(text.code, 0, "{}", text.err);
    assert!(text.out.contains("REQ-THERMO.to_celsius") && text.out.contains("Rounded,"), "{}", text.out);
    let json = trace(&dir, &["--diff", "HEAD", "--json"]);
    let parsed: serde_json::Value = serde_json::from_str(&json.out).expect("the difference is JSON");
    assert!(parsed.as_array().is_some_and(|a| !a.is_empty()), "{parsed}");
    assert_eq!(trace(&dir, &["--diff", "no-such-revision"]).code, 2);

    let out = dir.join("target/report");
    let plain = trace(&dir, &["--report", out.to_str().unwrap()]);
    assert_eq!(plain.out.trim(), out.join("index.html").display().to_string(), "{}", plain.err);
    let page = std::fs::read_to_string(out.join("index.html")).unwrap();
    assert!(page.contains("<html") && page.contains("REQ-THERMO.to_celsius"));
    let since = trace(&dir, &["--report", out.to_str().unwrap(), "--diff", "HEAD"]);
    assert_eq!(since.code, 0, "{}", since.err);
    assert!(std::fs::read_to_string(out.join("index.html")).unwrap().contains("Rounded,"));
    assert_eq!(trace(&dir, &["--report", out.to_str().unwrap(), "--diff", "no-such-revision"]).code, 2);
    let _ = std::fs::remove_dir_all(&dir);
}

/// @tests REQ-STRENGTH.obligation_generated
/// @tests REQ-STRENGTH.strength_per_model
#[test]
fn the_strength_of_each_model_is_listed_with_the_obligation_asked_for() {
    let dir = demo("pins");
    let pins = trace(&dir, &["--pins"]);
    assert!(pins.out.contains("== strength ("), "{}", pins.out);
    assert!(pins.out.contains("toFahrenheit"), "{}", pins.out);
    assert!(!pins.out.contains("obligation:"), "{}", pins.out);
    let one = trace(&dir, &["--strength", "toCelsius"]);
    assert!(one.out.contains("obligation:") && one.out.contains("∀ (g : Int → Int)"), "{}", one.out);
    let _ = std::fs::remove_dir_all(&dir);
}

/// @tests REQ-LOCK.evidence_preserved
/// @tests REQ-STALE.stale_is_visible
/// @tests REQ-STALE.listed_for_a_script
#[test]
fn a_change_is_listed_as_stale_and_the_lock_drops_what_lost_its_link() {
    let dir = demo("stale");
    let lock = trace(&dir, &["--lock"]);
    assert!(lock.out.contains("2 requirements") && lock.out.contains("evidence records"), "{}{}", lock.out, lock.err);

    edit(&dir.join("specs/Thermo.lean"), "def toCelsius", "def toCelsius'");
    edit(&dir.join("src/celsius.rs"), "pub fn describe", "pub fn describe_it");
    std::fs::create_dir_all(dir.join("docs")).unwrap();
    std::fs::write(dir.join("docs/thermo.md"), "---\ndescribes: REQ-THERMO\ndescribed_hash: 0000000000000000\n---\n# T\n").unwrap();
    let stale = trace(&dir, &["--stale"]);
    assert_eq!(stale.code, 1, "{}", stale.out);
    for wanted in ["judgement", "redo:", "document           docs/thermo.md -> REQ-THERMO: in-review", "coverage           src/celsius.rs"] {
        assert!(stale.out.contains(wanted), "no `{wanted}` in\n{}", stale.out);
    }

    std::fs::remove_file(dir.join("specs/Thermo.lean")).unwrap();
    let dropped = trace(&dir, &["--lock"]);
    assert!(dropped.out.contains("0 evidence records"), "{}", dropped.out);
    assert!(dropped.out.contains("  stale: REQ-THERMO.to_celsius ModelProof"), "{}", dropped.out);
    let _ = std::fs::remove_dir_all(&dir);
}

/// @tests REQ-ANCHOR.imprecise_capped
#[test]
fn a_file_is_said_to_parse_or_its_broken_lines_are_named() {
    let dir = demo("unparsed");
    assert!(trace(&dir, &["--unparsed"]).err.contains("--unparsed needs a file"));
    assert!(trace(&dir, &["--unparsed", "src/none.rs"]).err.contains("cannot read"));
    assert_eq!(trace(&dir, &["--unparsed", "src/celsius.rs"]).out, "src/celsius.rs: parses cleanly\n");
    std::fs::write(dir.join("src/broken.rs"), "fn ok() {}\nfn broken( {\n").unwrap();
    let broken = trace(&dir, &["--unparsed", "src/broken.rs"]);
    assert!(broken.out.starts_with("src/broken.rs:") && !broken.out.contains("parses cleanly"), "{}", broken.out);
    let _ = std::fs::remove_dir_all(&dir);
}

/// @tests REQ-STALE.measured_not_retaken
/// @tests REQ-STALE.agreed_not_rerun
#[test]
#[ignore = "builds the demo's tests and its bindings; run with --ignored"]
fn coverage_and_bindings_are_measured_once_and_then_held() {
    let dir = demo("measured");
    let first = trace(&dir, &["--coverage", "--again"]);
    assert!(first.out.contains("tests, ") && first.out.contains("src/celsius.rs"), "{}{}", first.out, first.err);
    assert!(trace(&dir, &["--coverage"]).out.starts_with("cached:"));
    let due = trace(&dir, &["--drt-due"]);
    assert!(due.out.lines().all(|l| l.starts_with("current ") || l.starts_with("due     ")), "{}", due.out);
    let drt = trace(&dir, &["--drt"]);
    assert!(drt.out.contains("REQ-THERMO.to_celsius"), "{}{}", drt.out, drt.err);
    let _ = std::fs::remove_dir_all(&dir);
}

/// @tests REQ-STRENGTH.proof_states_the_model
/// @tests REQ-STRENGTH.proved_only_when_pinned
#[test]
#[ignore = "builds a Lean package; run with --ignored"]
fn the_proof_check_names_each_refused_theorem_with_its_reason() {
    let dir = demo("proofs");
    let lean = dir.join("specs/Thermo.lean");
    let text = std::fs::read_to_string(&lean).unwrap();
    let at = text.rfind("end ").unwrap_or(text.len());
    let refused = "/-- @proves REQ-THERMO.to_celsius -/\ntheorem freezing_is_zero : toCelsius 32 = 0 := by decide\n\n";
    std::fs::write(&lean, format!("{}{refused}{}", &text[..at], &text[at..])).unwrap();
    let ran = trace(&dir, &["--proofs"]);
    assert_eq!(ran.code, 0, "{}", ran.err);
    assert!(ran.out.contains("theorems accepted;") && ran.out.contains("models pinned"), "{}", ran.out);
    assert!(ran.out.contains("refused  REQ-THERMO.to_celsius") && ran.out.contains("freezing_is_zero"), "{}", ran.out);
    let _ = std::fs::remove_dir_all(&dir);
}
