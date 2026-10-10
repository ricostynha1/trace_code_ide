//! An agent's context, checked against the model: the neighbourhood of a
//! requirement in the refinement graph, which parts a person's choice puts in
//! the copied text, and which part a label names.

mod harness;

use std::collections::BTreeMap;

use tracelean_core::drt::gen;
use tracelean_core::drt::run::{run, RunOptions};
use tracelean_core::drt::schema::Schema;
use tracelean_core::surface::context::{ancestors, descendants, part_named, parts_shown, Node, Part, ALL_PARTS};

mod support;

/// Four names, so chains, shared parents, cycles and a name declared twice
/// all occur often enough to be tested rather than hoped for.
fn name() -> Schema {
    Schema::Str { max_len: Some(0), examples: vec!["A".into(), "B".into(), "C".into(), "D".into()] }
}

fn graph() -> BTreeMap<String, Schema> {
    let mut node = BTreeMap::new();
    node.insert("id".to_string(), name());
    node.insert("refines".to_string(), Schema::List { inner: Box::new(name()), max_len: Some(2) });
    let mut fields = BTreeMap::new();
    fields.insert("nodes".to_string(), Schema::List { inner: Box::new(Schema::Struct { fields: node }), max_len: Some(4) });
    fields.insert("id".to_string(), name());
    fields
}

fn parts() -> Schema {
    let names = ["requirement", "refines", "refinedBy", "code", "tests", "models", "affected"];
    Schema::List { inner: Box::new(Schema::simple_enum(&names)), max_len: Some(5) }
}

fn choice() -> BTreeMap<String, Schema> {
    let mut fields = BTreeMap::new();
    fields.insert("included".to_string(), parts());
    fields.insert("filled".to_string(), parts());
    fields
}

fn label() -> BTreeMap<String, Schema> {
    let mut examples: Vec<String> = ALL_PARTS.iter().map(|p| p.label().to_string()).collect();
    examples.extend(["refinedBy".to_string(), "Code".to_string(), "everything".to_string(), String::new()]);
    let mut fields = BTreeMap::new();
    fields.insert("name".to_string(), Schema::Str { max_len: Some(0), examples });
    fields
}

#[allow(clippy::too_many_arguments)]
fn check(req: &str, op: &str, function: &str, entry: &str, arguments: &[&str], fields: BTreeMap<String, Schema>, seed: u64) {
    let clause = op.split_once('.').expect("an op names a clause").1;
    let scratch = harness::scratch(&format!("context-{clause}-{seed}"));
    let implementation = harness::rust_runner(req, clause, entry, &scratch);
    let model = harness::lean_runner("TraceLean.Context", function, op, arguments, &scratch);
    let result = run(
        op,
        &Schema::Struct { fields },
        &model,
        &implementation,
        RunOptions { seed, cases: 2_000, shrink_rounds: 100 },
    )
    .expect("both runners answer");
    support::agreed(&result);
    let _ = std::fs::remove_dir_all(&scratch);
}

/// @drt REQ-CONTEXT.neighbourhood_is_closed
/// @tests REQ-CONTEXT.neighbourhood_is_closed
#[test]
#[ignore = "builds a Lean package and a Rust crate; run with --ignored"]
fn model_and_implementation_agree_on_a_requirements_neighbourhood() {
    // Both directions in one call: what it refines and what refines it. Two
    // suites once named `ancestors` and `descendants` under the one op, and
    // the binding drove only the first.
    check(
        "REQ-CONTEXT",
        "REQ-CONTEXT.neighbourhood_is_closed",
        "TraceLean.Context.neighbourhood",
        "crates/core/src/surface/context.rs::neighbourhood",
        &["nodes", "id"],
        graph(),
        97,
    );
}

/// @drt REQ-CONTEXT.person_chooses
/// @tests REQ-CONTEXT.person_chooses
#[test]
#[ignore = "builds a Lean package and a Rust crate; run with --ignored"]
fn model_and_implementation_agree_on_what_is_copied() {
    check(
        "REQ-CONTEXT",
        "REQ-CONTEXT.person_chooses",
        "TraceLean.Context.partsShown",
        "crates/core/src/surface/context.rs::parts_shown",
        &["included", "filled"],
        choice(),
        99,
    );
}

/// @drt REQ-CONTEXT.part_named
/// @tests REQ-CONTEXT.part_named
#[test]
#[ignore = "builds a Lean package and a Rust crate; run with --ignored"]
fn model_and_implementation_agree_on_what_a_label_names() {
    check(
        "REQ-CONTEXT",
        "REQ-CONTEXT.part_named",
        "TraceLean.Context.partNamed",
        "crates/core/src/surface/context.rs::part_named",
        &["name"],
        label(),
        100,
    );
}

/// What a shell may type for `--parts`: nothing, `all`, or a list of labels
/// with dashes, stray blanks, repeats, and names that are no part.
fn shell_choice() -> BTreeMap<String, Schema> {
    let mut examples: Vec<String> = ALL_PARTS.iter().map(|p| p.label().replace(' ', "-")).collect();
    examples.extend(
        ["all", " code ", "tests,", "code,tests", "tests,code,code", "refined-by,nope", "affected tests", "", "Code"]
            .map(String::from),
    );
    let mut fields = BTreeMap::new();
    fields.insert(
        "parts".to_string(),
        Schema::Option { inner: Box::new(Schema::Str { max_len: Some(0), examples }) },
    );
    fields
}

/// @drt REQ-CONTEXT.from_the_shell
/// @tests REQ-CONTEXT.from_the_shell
#[test]
#[ignore = "builds a Lean package and a Rust crate; run with --ignored"]
fn model_and_implementation_agree_on_the_parts_a_shell_asks_for() {
    check(
        "REQ-CONTEXT",
        "REQ-CONTEXT.from_the_shell",
        "TraceLean.Context.shellParts",
        "crates/core/src/surface/context.rs::shell_parts",
        &["parts"],
        shell_choice(),
        101,
    );
}

fn words(examples: &[&str]) -> Schema {
    Schema::Str { max_len: Some(0), examples: examples.iter().map(|s| s.to_string()).collect() }
}

/// A claim over two requirements, two clauses, a few anchors (so one item can
/// carry several claims), test and non-test paths, and sources that name a
/// symbol on its own, inside a longer word, or not at all.
fn claim() -> Schema {
    let mut fields = BTreeMap::new();
    fields.insert(
        "role".to_string(),
        words(&["implements", "tests", "models", "proves", "drt", "pins", "specifies"]),
    );
    fields.insert("req".to_string(), words(&["REQ-A", "REQ-B"]));
    fields.insert("clause".to_string(), Schema::Option { inner: Box::new(words(&["x", "y"])) });
    fields.insert("ident".to_string(), words(&["i1", "i2", "i3"]));
    fields.insert(
        "path".to_string(),
        words(&["src/a.rs", "tests/t.rs", "src/test_x.rs", "b/tests/c.rs", "src/b.rs"]),
    );
    fields.insert("line".to_string(), Schema::Nat { max: Some(30), edges: vec![] });
    fields.insert(
        "symbol".to_string(),
        Schema::Option { inner: Box::new(words(&["f", "m::f", "m::g", "a::b::h"])) },
    );
    fields.insert(
        "source".to_string(),
        words(&["f()", "let x = fg();", "g f", "// h", "", "see h_1", "(h)"]),
    );
    Schema::Struct { fields }
}

fn claims_in() -> BTreeMap<String, Schema> {
    let mut fields = BTreeMap::new();
    fields.insert("claims".to_string(), Schema::List { inner: Box::new(claim()), max_len: Some(6) });
    fields.insert("id".to_string(), words(&["REQ-A", "REQ-B", "REQ-C"]));
    fields.insert("clause".to_string(), Schema::Option { inner: Box::new(words(&["x", "y"])) });
    fields
}

/// Claims weighted towards implementing and testing, the two roles the tests
/// a change may break are about, so an implementing item with a symbol and a
/// test file nobody claimed meet often.
fn affected_in() -> BTreeMap<String, Schema> {
    let mut fields = claims_in();
    let Schema::Struct { fields: mut one } = claim() else { unreachable!() };
    one.insert("role".to_string(), words(&["implements", "implements", "implements", "tests", "tests", "models"]));
    one.insert("req".to_string(), words(&["REQ-A", "REQ-A", "REQ-B"]));
    one.insert("symbol".to_string(), Schema::Option { inner: Box::new(words(&["f", "m::f", "a::b::h"])) });
    fields.insert("claims".to_string(), Schema::List { inner: Box::new(Schema::Struct { fields: one }), max_len: Some(6) });
    fields.insert("id".to_string(), words(&["REQ-A", "REQ-A", "REQ-A", "REQ-C"]));
    fields.insert("down".to_string(), Schema::List { inner: Box::new(words(&["REQ-A", "REQ-B"])), max_len: Some(2) });
    let file = Schema::Tuple {
        items: vec![
            words(&["tests/t.rs", "src/a.rs", "tests/u.rs", "tests/v.rs", "src/test_x.rs"]),
            words(&["f()", "x\r\nf\n", "fn g() { h(); }\nf\n\n", "", "fg\nf f\n", "// f\r\n", "h(1)\n"]),
        ],
    };
    fields.insert("files".to_string(), Schema::List { inner: Box::new(file), max_len: Some(3) });
    fields
}

/// @drt REQ-CONTEXT.claims_with_source
/// @tests REQ-CONTEXT.claims_with_source
#[test]
#[ignore = "builds a Lean package and a Rust crate; run with --ignored"]
fn model_and_implementation_agree_on_what_claims_a_target() {
    check(
        "REQ-CONTEXT",
        "REQ-CONTEXT.claims_with_source",
        "TraceLean.Context.claimsOn",
        "crates/core/src/surface/context.rs::claims_on",
        &["claims", "id", "clause"],
        claims_in(),
        102,
    );
}

/// @drt REQ-CONTEXT.affected_tests
/// @tests REQ-CONTEXT.affected_tests
#[test]
#[ignore = "builds a Lean package and a Rust crate; run with --ignored"]
fn model_and_implementation_agree_on_the_tests_a_change_may_break() {
    check(
        "REQ-CONTEXT",
        "REQ-CONTEXT.affected_tests",
        "TraceLean.Context.affected",
        "crates/core/src/surface/context.rs::affected",
        &["claims", "id", "clause", "down", "files"],
        affected_in(),
        103,
    );
}

/// The generators reach what the clauses are about.
///
/// @tests REQ-DRT-COVER.law_coverage
#[test]
fn generation_reaches_chains_cycles_and_every_kind_of_choice() {
    let mut rng = gen::Rng::new(97);
    let (mut none, mut deep, mut cycle, mut twice) = (0u64, 0u64, 0u64, 0u64);
    for _ in 0..2_000 {
        let value = gen::value(&Schema::Struct { fields: graph() }, &mut rng);
        let nodes: Vec<Node> = serde_json::from_value(value["nodes"].clone()).expect("nodes");
        let id = value["id"].as_str().expect("an id").to_string();
        let up = ancestors(nodes.clone(), id.clone());
        let down = descendants(nodes.clone(), id.clone());
        if up.is_empty() {
            none += 1;
        }
        let direct: Vec<String> = nodes.iter().filter(|n| n.id == id).flat_map(|n| n.refines.clone()).collect();
        if up.iter().any(|a| !direct.contains(a)) {
            deep += 1;
        }
        if up.iter().any(|a| down.contains(a)) || direct.contains(&id) {
            cycle += 1;
        }
        if nodes.iter().filter(|n| n.id == id).count() > 1 {
            twice += 1;
        }
    }
    support::covered(
        "REQ-CONTEXT.neighbourhood_is_closed",
        &[
            ("nothing above", none),
            ("two or more steps up", deep),
            ("a cycle back to the target", cycle),
            ("a name declared twice", twice),
        ],
    );

    let mut rng = gen::Rng::new(99);
    let (mut nothing, mut empty, mut disorder, mut repeated) = (0u64, 0u64, 0u64, 0u64);
    for _ in 0..2_000 {
        let value = gen::value(&Schema::Struct { fields: choice() }, &mut rng);
        let included: Vec<Part> = serde_json::from_value(value["included"].clone()).expect("parts");
        let filled: Vec<Part> = serde_json::from_value(value["filled"].clone()).expect("parts");
        if included.is_empty() {
            nothing += 1;
        }
        if included.iter().any(|p| !filled.contains(p)) {
            empty += 1;
        }
        if included.windows(2).any(|w| w[0] > w[1]) {
            disorder += 1;
        }
        if included.iter().enumerate().any(|(n, p)| included[..n].contains(p)) {
            repeated += 1;
        }
        let shown = parts_shown(included.clone(), filled.clone());
        assert!(shown.iter().all(|p| included.contains(p) && filled.contains(p)));
    }
    support::covered(
        "REQ-CONTEXT.person_chooses",
        &[
            ("nothing chosen", nothing),
            ("a part chosen with nothing in it", empty),
            ("chosen out of order", disorder),
            ("a part chosen twice", repeated),
        ],
    );

    let mut rng = gen::Rng::new(100);
    let (mut named, mut unnamed) = (0u64, 0u64);
    for _ in 0..2_000 {
        let value = gen::value(&Schema::Struct { fields: label() }, &mut rng);
        match part_named(value["name"].as_str().expect("a name").to_string()) {
            Some(_) => named += 1,
            None => unnamed += 1,
        }
    }
    support::covered(
        "REQ-CONTEXT.part_named",
        &[("a label that names a part", named), ("a name that is no part", unnamed)],
    );
}

/// The shell's choices, the claims on a target and the tests a change may
/// break reach what their floors name.
///
/// @tests REQ-DRT-COVER.law_coverage
#[test]
fn generation_reaches_shell_lists_shared_items_and_unclaimed_uses() {
    use tracelean_core::surface::context::{affected, claims_on, shell_parts, Claim, ShellParts};

    let mut rng = gen::Rng::new(101);
    let (mut nothing, mut list, mut unknown) = (0u64, 0u64, 0u64);
    for _ in 0..2_000 {
        let value = gen::value(&Schema::Struct { fields: shell_choice() }, &mut rng);
        let parts = value["parts"].as_str().map(str::to_string);
        if parts.is_none() {
            nothing += 1;
        }
        match shell_parts(parts.clone()) {
            ShellParts::Unknown { .. } => unknown += 1,
            ShellParts::Chosen { .. } if parts.is_some() => list += 1,
            ShellParts::Chosen { .. } => {}
        }
    }
    support::covered(
        "REQ-CONTEXT.from_the_shell",
        &[("nothing asked for", nothing), ("a list of labels", list), ("a name that is no part", unknown)],
    );

    let mut rng = gen::Rng::new(102);
    let (mut asked, mut shared) = (0u64, 0u64);
    for _ in 0..2_000 {
        let value = gen::value(&Schema::Struct { fields: claims_in() }, &mut rng);
        let claims: Vec<Claim> = serde_json::from_value(value["claims"].clone()).expect("claims");
        let id = value["id"].as_str().expect("an id").to_string();
        let clause = value["clause"].as_str().map(str::to_string);
        if clause.is_some() {
            asked += 1;
        }
        let on: Vec<&Claim> =
            claims.iter().filter(|c| c.req == id && (clause.is_none() || c.clause == clause)).collect();
        if on.iter().enumerate().any(|(n, c)| on[..n].iter().any(|d| d.ident == c.ident)) {
            shared += 1;
        }
        let _ = claims_on(claims, id, clause);
    }
    support::covered(
        "REQ-CONTEXT.claims_with_source",
        &[("a clause asked for", asked), ("two claims on one item", shared)],
    );

    let mut rng = gen::Rng::new(103);
    let (mut named, mut used) = (0u64, 0u64);
    for _ in 0..2_000 {
        let value = gen::value(&Schema::Struct { fields: affected_in() }, &mut rng);
        let claims: Vec<Claim> = serde_json::from_value(value["claims"].clone()).expect("claims");
        let down: Vec<String> = serde_json::from_value(value["down"].clone()).expect("down");
        let files: Vec<(String, String)> = serde_json::from_value(value["files"].clone()).expect("files");
        let items = affected(
            claims,
            value["id"].as_str().expect("an id").to_string(),
            value["clause"].as_str().map(str::to_string),
            down,
            files,
        );
        if items.iter().any(|i| i.role == "tests") {
            named += 1;
        }
        if items.iter().any(|i| i.role == "uses") {
            used += 1;
        }
    }
    support::covered(
        "REQ-CONTEXT.affected_tests",
        &[("a test that names an implementing item", named), ("a line of an unclaimed test file", used)],
    );
}
