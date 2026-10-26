mod harness;

use tracelean_core::drt::gen;
use tracelean_core::drt::rust_runner::{call_site, Entry};
use tracelean_core::drt::schema::Schema;

fn names() -> Schema {
    Schema::Str { max_len: Some(0), examples: vec!["a".into(), "b".into(), "src".into()] }
}

fn text() -> Schema {
    Schema::Str {
        max_len: Some(3),
        examples: vec![
            "REQ-X.c".into(),
            "a\"b".into(),
            "a\\b".into(),
            "x\ny".into(),
            "caf\u{e9}".into(),
            "\u{1}".into(),
        ],
    }
}

fn entry() -> Schema {
    Schema::Struct {
        fields: [
            ("op".to_string(), text()),
            (
                "call_path".to_string(),
                Schema::Str { max_len: Some(0), examples: vec!["k::f".into(), "tracelean_core::drt::gen::value".into()] },
            ),
            ("parameters".to_string(), Schema::List { inner: Box::new(names()), max_len: Some(3) }),
            (
                "sources".to_string(),
                Schema::List {
                    inner: Box::new(Schema::Str {
                        max_len: Some(0),
                        examples: vec!["a".into(), "b".into(), "src".into(), "x\"y".into()],
                    }),
                    max_len: Some(3),
                },
            ),
        ]
        .into_iter()
        .collect(),
    }
}

fn input() -> Schema {
    Schema::Struct { fields: [("entry".to_string(), entry())].into_iter().collect() }
}

/// @drt REQ-DRT-RUST.types_inferred
/// @tests REQ-DRT-RUST.types_inferred
#[test]
#[ignore = "builds a Lean package and a Rust crate; run with --ignored"]
fn model_and_implementation_agree_on_the_generated_call_site() {
    let op = "REQ-DRT-RUST.types_inferred";
    harness::agree(op, &input(), 4242, 5_000);
}

/// @tests REQ-DRT-COVER.law_coverage
/// @tests REQ-DRT-RUST.types_inferred
#[test]
fn generation_reaches_every_shape_of_call_site() {
    let mut rng = gen::Rng::new(4242);
    let mut counts = [0u64; 4];
    for _ in 0..5_000 {
        let v = gen::value(&input(), &mut rng);
        let entry: Entry = serde_json::from_value(v["entry"].clone()).unwrap();
        let escapes = |s: &str| s.chars().any(|c| c == '"' || c == '\\' || !(' '..='~').contains(&c));
        if entry.parameters.is_empty() {
            counts[0] += 1;
        }
        if entry.parameters.len() > 1 {
            counts[1] += 1;
        }
        if entry.parameters.iter().zip(&entry.sources).any(|(p, s)| p != s) {
            counts[2] += 1;
        }
        if escapes(&entry.op) || entry.sources.iter().any(|s| escapes(s)) {
            counts[3] += 1;
        }
        let arm = call_site(entry);
        assert!(!arm.contains("::<") && !arm.contains("let a:"), "the call site named a type:\n{arm}");
    }
    harness::covered(
        "REQ-DRT-RUST.types_inferred",
        &[
            ("an entry taking nothing", counts[0]),
            ("an entry taking several arguments", counts[1]),
            ("an argument read from a renamed field", counts[2]),
            ("a literal needing an escape", counts[3]),
        ],
    );
}
