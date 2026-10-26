mod harness;

use harness::{name, strukt};

use tracelean_core::drt::schema::Schema;

fn fields() -> Schema {
    Schema::List {
        inner: Box::new(Schema::Tuple { items: vec![name(&["model", "floors", "implementation", "waive"]), name(&["1", "2", "[]"])] }),
        max_len: Some(4),
    }
}

/// @drt REQ-DRT-BIND.derived_unless_written
/// @tests REQ-DRT-BIND.derived_unless_written
#[test]
#[ignore = "builds a Lean package and a Rust crate; run with --ignored"]
fn model_and_implementation_agree_on_what_a_written_binding_replaces() {
    harness::agree("REQ-DRT-BIND.derived_unless_written", &strukt(&[("derived", fields()), ("written", fields())]), 61, 2_000);
}

/// @tests REQ-DRT-BIND.derived_unless_written
#[test]
fn a_written_field_replaces_the_derived_one_and_leaves_the_rest() {
    use tracelean_core::drt::config::overlay;
    let derived = serde_json::json!({"req_id": "REQ-A", "clause": "b", "implementation": {"language": "rust", "entry": "a.rs::f"}});
    let written = serde_json::json!({"req_id": "REQ-A", "clause": "b", "implementation": {"language": "rust", "entry": "a.rs::g"}, "also_checks": ["c"]});
    let binding = overlay(Some(derived), written).expect("a binding");
    assert_eq!(binding.implementation.entry, "a.rs::g");
    assert_eq!(binding.clauses(), vec!["REQ-A.b".to_string(), "REQ-A.c".to_string()]);
    assert!(overlay(None, serde_json::json!({"req_id": "REQ-A"})).is_err(), "nothing to call");
}
