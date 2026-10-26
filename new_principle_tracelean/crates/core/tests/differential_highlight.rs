mod harness;

use std::collections::BTreeMap;

use tracelean_core::drt::schema::Schema;

const FRAGMENTS: &[&str] = &[
    "REQ-SHOW", "REQ-DRT-PROTO.ops_unique", "ARCH-NO-DRIVING", "REQ-X", "R-X", "REQ-", "REQ-Xyz",
    "xREQ-A", "REQ-A.b_2", "REQ-A.", "REQ-A.B", "é", " ", "// ", ".", "-", "A1-B",
];

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
    harness::agree(op, &comment(), 401, 2_000);
}

/// @drt REQ-SHOW.findings_lead_somewhere
/// @tests REQ-SHOW.findings_lead_somewhere
#[test]
#[ignore = "builds a Lean package and a Rust crate; run with --ignored"]
fn model_and_implementation_agree_on_where_findings_lead() {
    let op = "REQ-SHOW.findings_lead_somewhere";
    let Schema::Struct { fields: comment } = comment() else { unreachable!() };
    let mut found = BTreeMap::new();
    found.insert("kind".to_string(), Schema::Str { max_len: Some(0), examples: vec!["unmodeled".into(), "".into()] });
    found.insert("file".to_string(), Schema::Str { max_len: Some(0), examples: vec!["reqs/REQ-X.md".into(), "é.rs".into()] });
    found.insert("line".to_string(), Schema::Nat { max: Some(120), edges: vec![0, 1] });
    found.insert("message".to_string(), comment["text"].clone());
    let mut fields = BTreeMap::new();
    fields.insert("title".to_string(), Schema::Str { max_len: Some(0), examples: vec!["findings".into(), "check".into()] });
    fields.insert("found".to_string(), Schema::List { inner: Box::new(Schema::Struct { fields: found }), max_len: Some(3) });
    harness::agree(op, &Schema::Struct { fields }, 403, 1_500);
}
