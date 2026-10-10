//! Requirement names in a comment, checked against the model: what is a name,
//! where it ends, and where it may start; and the findings view, whose
//! messages are read for names the same way.

mod harness;

use std::collections::BTreeMap;

use tracelean_core::drt::run::{run, RunOptions};
use tracelean_core::drt::schema::Schema;

mod support;

/// Fragments that are names, near-names and the characters around them: a
/// clause key, digits in a group, a lower-case letter running into the name, a
/// non-ASCII letter, a name cut short.
const FRAGMENTS: &[&str] = &[
    "REQ-SHOW", "REQ-DRT-PROTO.ops_unique", "ARCH-NO-DRIVING", "REQ-X", "R-X", "REQ-", "REQ-Xyz",
    "xREQ-A", "REQ-A.b_2", "REQ-A.", "REQ-A.B", "é", " ", "// ", ".", "-", "A1-B",
];

/// A comment is two fragments joined, so every name meets every neighbour on
/// both sides; the range drawn over it may start late and stop short or past
/// the end.
fn comment() -> Schema {
    let mut texts = Vec::new();
    for a in FRAGMENTS {
        for b in FRAGMENTS {
            texts.push(format!("{a}{b}"));
        }
    }
    let mut fields = BTreeMap::new();
    fields.insert("text".to_string(), Schema::Str { max_len: Some(0), examples: texts });
    fields.insert("start".to_string(), Schema::Nat { max: Some(6), edges: vec![0] });
    fields.insert("stop".to_string(), Schema::Nat { max: Some(60), edges: vec![0, 1000] });
    Schema::Struct { fields }
}

/// @drt REQ-SHOW.references_are_links
/// @tests REQ-SHOW.references_are_links
#[test]
#[ignore = "builds a Lean package and a Rust crate; run with --ignored"]
fn model_and_implementation_agree_on_the_names_a_comment_holds() {
    let op = "REQ-SHOW.references_are_links";
    let scratch = harness::scratch("highlight-names");
    let implementation = harness::rust_runner(
        "REQ-SHOW",
        "references_are_links",
        "crates/core/src/surface/highlight.rs::requirement_names",
        &scratch,
    );
    let model = harness::lean_runner(
        "TraceLean.Highlight",
        "TraceLean.Highlight.requirementNames",
        op,
        &["text", "start", "stop"],
        &scratch,
    );
    let result = run(
        op,
        &comment(),
        &model,
        &implementation,
        RunOptions { seed: 401, cases: 2_000, shrink_rounds: 100 },
    )
    .expect("both runners answer");
    support::agreed(&result);
    let _ = std::fs::remove_dir_all(&scratch);
}

/// @drt REQ-SHOW.findings_lead_somewhere
/// @tests REQ-SHOW.findings_lead_somewhere
#[test]
#[ignore = "builds a Lean package and a Rust crate; run with --ignored"]
fn model_and_implementation_agree_on_where_findings_lead() {
    let op = "REQ-SHOW.findings_lead_somewhere";
    let scratch = harness::scratch("findings-view");
    let implementation = harness::rust_runner(
        "REQ-SHOW",
        "findings_lead_somewhere",
        "crates/core/src/surface/findings_view.rs::findings_view_owned",
        &scratch,
    );
    let model = harness::lean_runner(
        "TraceLean.FindingsView",
        "TraceLean.FindingsView.findingsView",
        op,
        &["title", "found"],
        &scratch,
    );
    let Schema::Struct { fields: comment } = comment() else { unreachable!() };
    let mut found = BTreeMap::new();
    found.insert("kind".to_string(), Schema::Str { max_len: Some(0), examples: vec!["unmodeled".into(), "".into()] });
    found.insert("file".to_string(), Schema::Str { max_len: Some(0), examples: vec!["reqs/REQ-X.md".into(), "é.rs".into()] });
    found.insert("line".to_string(), Schema::Nat { max: Some(120), edges: vec![0, 1] });
    found.insert("message".to_string(), comment["text"].clone());
    let mut fields = BTreeMap::new();
    fields.insert("title".to_string(), Schema::Str { max_len: Some(0), examples: vec!["findings".into(), "check".into()] });
    fields.insert("found".to_string(), Schema::List { inner: Box::new(Schema::Struct { fields: found }), max_len: Some(3) });
    let result = run(
        op,
        &Schema::Struct { fields },
        &model,
        &implementation,
        RunOptions { seed: 403, cases: 1_500, shrink_rounds: 100 },
    )
    .expect("both runners answer");
    support::agreed(&result);
    let _ = std::fs::remove_dir_all(&scratch);
}
