mod harness;

use harness::{name, strukt};

use std::collections::BTreeMap;

use tracelean_core::drt::schema::Schema;
use tracelean_core::surface::report::{page, Shared};
use tracelean_core::trace::checker::{check, Policy};
use tracelean_core::trace::diff::{states, Change};
use tracelean_core::trace::index::build;
use tracelean_core::trace::lockfile::render;
use tracelean_core::trace::proofcheck::ProofReport;

/// @drt REQ-REPORT.why_not_held
/// @tests REQ-REPORT.why_not_held
#[test]
#[ignore = "builds a Lean package and a Rust crate; run with --ignored"]
fn model_and_implementation_agree_on_why_a_mark_is_not_held() {
    let marks: BTreeMap<String, Option<Box<Schema>>> =
        ["annotated", "judged", "tested", "proved"].iter().map(|m| (m.to_string(), None)).collect();
    let roles: BTreeMap<String, Option<Box<Schema>>> =
        ["models", "implements", "drt", "tests", "proves", "pins"].iter().map(|m| (m.to_string(), None)).collect();
    let facts = strukt(&[
        ("held", Schema::List { inner: Box::new(Schema::Enum { variants: marks }), max_len: Some(4) }),
        ("structural", Schema::Bool),
        ("roles", Schema::List { inner: Box::new(Schema::Enum { variants: roles }), max_len: Some(5) }),
        ("refused", Schema::List { inner: Box::new(name(&["t: silent", "u: vacuous"])), max_len: Some(2) }),
    ]);
    harness::agree("REQ-REPORT.why_not_held", &strukt(&[("facts", facts)]), 91, 3_000);
}

/// @tests REQ-REPORT.static_page
/// @tests REQ-REPORT.card_per_clause
/// @tests REQ-REPORT.evidence_on_card
/// @tests REQ-REPORT.findings_on_card
/// @tests REQ-REPORT.changes_marked
#[test]
fn every_clause_gets_a_card_with_its_evidence_findings_and_changes() {
    let dir = harness::scratch("report-fixture");
    std::fs::create_dir_all(dir.join("reqs")).unwrap();
    std::fs::create_dir_all(dir.join("src")).unwrap();
    std::fs::create_dir_all(dir.join("formal")).unwrap();
    std::fs::write(
        dir.join("reqs/a.md"),
        "---\nid: REQ-A\ntitle: A fixture\nclauses:\n  one: The system shall <double> a number.\n  two: The system shall do two.\n---\nbody",
    )
    .unwrap();
    std::fs::write(dir.join("formal/M.lean"), "/-- @models REQ-A.one -/\ndef double (n : Nat) : Nat := n + n\n").unwrap();
    std::fs::write(dir.join("src/i.rs"), "/// @implements REQ-A.one\npub fn double(n: u32) -> u32 { n * 2 }\n").unwrap();
    let index = build(&dir);
    let lock = render(&index, vec![]);
    let proofs = ProofReport::default();
    let found = check(&index, &Policy::default());
    let files: BTreeMap<String, String> = ["reqs/a.md", "formal/M.lean", "src/i.rs"]
        .iter()
        .map(|f| (f.to_string(), std::fs::read_to_string(dir.join(f)).unwrap()))
        .collect();
    let mut changes = BTreeMap::new();
    changes.insert(
        ("REQ-A".to_string(), Some("one".to_string())),
        vec![Change::Reworded { before: "The system shall halve.".into(), after: "The system shall <double> a number.".into() }],
    );
    let strengths = BTreeMap::new();
    let shared = Shared {
        index: &index,
        lock: &lock,
        proofs: &proofs,
        strengths: &strengths,
        findings: &found,
        files: &files,
        changes: &changes,
        base: Some("main"),
    };
    let html = page(&shared, &states(&lock, &index.requirements, &proofs));

    assert!(html.starts_with("<!doctype html>"));
    for external in ["src=\"http", "href=\"http", "<link", "@import", "fetch("] {
        assert!(!html.contains(external), "the page reaches out with `{external}`");
    }
    assert_eq!(html.matches("<article").count(), 2, "one card per clause");
    let one = &html[html[..html.find("id=\"REQ-A.one\"").unwrap()].rfind("<article").unwrap()..];
    let one = &one[..one.find("</article>").unwrap()];
    assert!(one.contains("The system shall &lt;double&gt; a number."), "the text, escaped");
    assert!(one.contains("def double (n : Nat)"), "the model's source");
    assert!(one.contains("src/i.rs:2"), "the implementation's place");
    assert!(one.contains("model and code exist, but nothing compares them"), "why T is not held");
    assert!(one.contains("Unbound"), "the finding about the clause");
    assert!(one.contains("Changed since main") && one.contains("The system shall halve."), "the change");
    let tag = &one[..one.find('>').unwrap()];
    assert!(tag.contains(" changed") && tag.contains(" noT"), "the card is marked changed and untested: {tag}");
    let two = &html[html.find("id=\"REQ-A.two\"").unwrap()..];
    let two = &two[..two.find("</article>").unwrap()];
    assert!(two.contains("no annotation names this clause"));
    assert!(!two.contains("Changed since"));
    let _ = std::fs::remove_dir_all(&dir);
}
