//! What a station holds, checked against the model.
//!
//! Four stations, three producers and a price table. The requirement index and
//! the refinement graph are the same rows ordered differently; the sandbox is a
//! record of what a tool changed, said and is estimated to have spent; and what
//! a station stands for is itself a function, because a list of stations
//! written once in the core and again in each frontend is a list that drifts.

mod harness;

use std::collections::BTreeMap;

use tracelean_core::drt::run::{run, RunOptions};
use tracelean_core::drt::schema::Schema;

mod support;

/// Small alphabets, deliberately. Ids drawn from three names and refinements
/// from the same three is what makes a cycle, a shared parent and a node no
/// root reaches occur often enough to be tested rather than hoped for.
fn node() -> Schema {
    let name = |examples: Vec<String>| Schema::Str { max_len: Some(0), examples };
    let mut fields = BTreeMap::new();
    fields.insert("id".to_string(), name(vec!["A".into(), "B".into(), "C".into()]));
    fields.insert("title".to_string(), name(vec!["one".into(), "é".into()]));
    fields.insert(
        "refines".to_string(),
        Schema::List {
            inner: Box::new(name(vec!["A".into(), "B".into(), "C".into()])),
            max_len: Some(2),
        },
    );
    fields.insert("level".to_string(), Schema::simple_enum(&["L1", "L2", "L3", "L4"]));
    Schema::Struct { fields }
}

fn nodes() -> Schema {
    Schema::List { inner: Box::new(node()), max_len: Some(4) }
}

fn one_node_list() -> Schema {
    let mut fields = BTreeMap::new();
    fields.insert("nodes".to_string(), nodes());
    Schema::Struct { fields }
}

/// A token count. The edges are a round million and nothing, so a run holds
/// both totals that survive the division and totals that truncate away.
fn tokens() -> Schema {
    Schema::Nat { max: Some(2), edges: vec![0, 1_000_000] }
}

fn rate() -> Schema {
    Schema::Nat { max: Some(3), edges: vec![0, 3_000_000, 15_000_000] }
}

fn model() -> Schema {
    Schema::Str {
        max_len: Some(0),
        examples: vec!["sonnet".into(), "opus".into(), "future".into()],
    }
}

fn counts(model_field: Schema) -> BTreeMap<String, Schema> {
    let mut fields = BTreeMap::new();
    fields.insert("model".to_string(), model_field);
    for field in ["input", "cached", "cacheWrite", "output"] {
        fields.insert(field.to_string(), tokens());
    }
    fields
}

fn usage() -> Schema {
    Schema::Struct { fields: counts(model()) }
}

fn price() -> Schema {
    let mut fields = counts(model());
    for field in ["input", "cached", "cacheWrite", "output"] {
        fields.insert(field.to_string(), rate());
    }
    Schema::Struct { fields }
}

fn spend() -> Schema {
    let mut fields = BTreeMap::new();
    fields.insert(
        "amount".to_string(),
        Schema::Nat { max: Some(2_000_000), edges: vec![0, 1_000_005, 999_999] },
    );
    fields.insert(
        "unpriced".to_string(),
        Schema::List { inner: Box::new(model()), max_len: Some(2) },
    );
    Schema::Struct { fields }
}

fn check(
    req: &str,
    op: &str,
    import: &str,
    function: &str,
    entry: &str,
    arguments: &[&str],
    schema: Schema,
    seed: u64,
) {
    let clause = op.split_once('.').expect("an op names a clause").1;
    let scratch = harness::scratch(&format!("station-{clause}"));
    let implementation = harness::rust_runner(req, clause, entry, &scratch);
    let model = harness::lean_runner(import, function, op, arguments, &scratch);

    let result = run(
        op,
        &schema,
        &model,
        &implementation,
        RunOptions { seed, cases: 2_000, shrink_rounds: 100 },
    )
    .expect("both runners answer");

    support::agreed(&result);

    let _ = std::fs::remove_dir_all(&scratch);
}

/// @drt REQ-SHOW.index_from_requirements
/// @tests REQ-SHOW.index_from_requirements
#[test]
#[ignore = "builds a Lean package and a Rust crate; run with --ignored"]
fn model_and_implementation_agree_on_the_requirement_index() {
    check(
        "REQ-SHOW",
        "REQ-SHOW.index_from_requirements",
        "TraceLean.Produce",
        "TraceLean.Produce.requirementsBuffer",
        "crates/core/src/surface/produce.rs::requirements_buffer",
        &["nodes"],
        one_node_list(),
        61,
    );
}

/// @drt REQ-SHOW.graph_from_refinement
/// @tests REQ-SHOW.graph_from_refinement
#[test]
#[ignore = "builds a Lean package and a Rust crate; run with --ignored"]
fn model_and_implementation_agree_on_the_refinement_graph() {
    check(
        "REQ-SHOW",
        "REQ-SHOW.graph_from_refinement",
        "TraceLean.Produce",
        "TraceLean.Produce.designBuffer",
        "crates/core/src/surface/produce.rs::design_buffer",
        &["nodes"],
        one_node_list(),
        67,
    );
}

/// @drt REQ-SHOW.sandbox_from_observation
/// @tests REQ-SHOW.sandbox_from_observation
#[test]
#[ignore = "builds a Lean package and a Rust crate; run with --ignored"]
fn model_and_implementation_agree_on_what_the_sandbox_station_shows() {
    let mut event = BTreeMap::new();
    event.insert(
        "kind".to_string(),
        Schema::Str { max_len: Some(0), examples: vec!["say".into(), "tool".into()] },
    );
    event.insert(
        "text".to_string(),
        Schema::Str { max_len: Some(0), examples: vec!["one".into(), "é".into()] },
    );
    let mut fields = BTreeMap::new();
    fields.insert(
        "changed".to_string(),
        Schema::List {
            inner: Box::new(Schema::Str {
                max_len: Some(0),
                examples: vec!["a.rs".into(), "src/é.rs".into()],
            }),
            max_len: Some(2),
        },
    );
    fields.insert(
        "said".to_string(),
        Schema::List { inner: Box::new(Schema::Struct { fields: event }), max_len: Some(2) },
    );
    fields.insert("spend".to_string(), spend());
    check(
        "REQ-SHOW",
        "REQ-SHOW.sandbox_from_observation",
        "TraceLean.Produce",
        "TraceLean.Produce.sandboxEvents",
        "crates/core/src/surface/produce.rs::sandbox_events",
        &["changed", "said", "spend"],
        Schema::Struct { fields },
        71,
    );
}

/// @drt REQ-SCREEN.station_produces_a_buffer
/// @tests REQ-SCREEN.station_produces_a_buffer
#[test]
#[ignore = "builds a Lean package and a Rust crate; run with --ignored"]
fn model_and_implementation_agree_on_what_a_station_stands_for() {
    let mut fields = BTreeMap::new();
    fields.insert(
        "station".to_string(),
        Schema::Str {
            max_len: Some(0),
            examples: vec![
                "project".into(),
                "sandbox".into(),
                "requirements".into(),
                "design".into(),
                "history".into(),
                "Project".into(),
            ],
        },
    );
    check(
        "REQ-SCREEN",
        "REQ-SCREEN.station_produces_a_buffer",
        "TraceLean.Screen",
        "TraceLean.Screen.stationKind",
        "crates/core/src/surface/screen.rs::station_kind",
        &["station"],
        Schema::Struct { fields },
        73,
    );
}

/// @drt REQ-COST.cost_from_usage
/// @tests REQ-COST.cost_from_usage
/// @tests REQ-COST.price_is_per_model
#[test]
#[ignore = "builds a Lean package and a Rust crate; run with --ignored"]
fn model_and_implementation_agree_on_what_a_run_is_estimated_to_have_cost() {
    let mut fields = BTreeMap::new();
    fields.insert(
        "table".to_string(),
        Schema::List { inner: Box::new(price()), max_len: Some(2) },
    );
    fields.insert(
        "usage".to_string(),
        Schema::List { inner: Box::new(usage()), max_len: Some(3) },
    );
    check(
        "REQ-COST",
        "REQ-COST.cost_from_usage",
        "TraceLean.Cost",
        "TraceLean.Cost.spendOf",
        "crates/core/src/observe/cost.rs::spend_of",
        &["table", "usage"],
        Schema::Struct { fields },
        79,
    );
}

/// @drt REQ-COST.cache_priced_apart
/// @tests REQ-COST.cache_priced_apart
#[test]
#[ignore = "builds a Lean package and a Rust crate; run with --ignored"]
fn model_and_implementation_agree_on_what_one_exchange_cost() {
    let mut fields = BTreeMap::new();
    fields.insert("price".to_string(), price());
    fields.insert("usage".to_string(), usage());
    check(
        "REQ-COST",
        "REQ-COST.cache_priced_apart",
        "TraceLean.Cost",
        "TraceLean.Cost.amountOf",
        "crates/core/src/observe/cost.rs::amount_of",
        &["price", "usage"],
        Schema::Struct { fields },
        83,
    );
}

/// @drt REQ-COST.estimate_is_labelled
/// @tests REQ-COST.estimate_is_labelled
#[test]
#[ignore = "builds a Lean package and a Rust crate; run with --ignored"]
fn model_and_implementation_agree_on_how_an_estimate_reads() {
    let mut fields = BTreeMap::new();
    fields.insert("spend".to_string(), spend());
    check(
        "REQ-COST",
        "REQ-COST.estimate_is_labelled",
        "TraceLean.Cost",
        "TraceLean.Cost.estimateLine",
        "crates/core/src/observe/cost.rs::estimate_line",
        &["spend"],
        Schema::Struct { fields },
        89,
    );
}

/// The two stations' rows, and the shapes of graph that make the drawing hard.
///
/// A run of graphs that were all trees would agree about a walk that mishandled
/// a cycle, and a run whose requirements all reached the same level would agree
/// about a producer that ignored the level entirely.
///
/// @tests REQ-SHOW.index_from_requirements
/// @tests REQ-SHOW.graph_from_refinement
/// @tests REQ-DRT-COVER.law_coverage
#[test]
fn generation_reaches_the_graphs_that_are_hard_to_draw() {
    use tracelean_core::drt::gen;
    use tracelean_core::evidence::Level;
    use tracelean_core::surface::produce::{design_rows, requirements_buffer, Node};
    use tracelean_core::surface::view::{faults, Role};

    let schema = nodes();
    let mut rng = gen::Rng::new(61);
    let (mut empty, mut lowest, mut highest, mut several) = (0u64, 0u64, 0u64, 0u64);
    let (mut nothing, mut under, mut twice, mut unreached, mut cyclic) =
        (0u64, 0u64, 0u64, 0u64, 0u64);
    for _ in 0..3_000 {
        let value = gen::value(&schema, &mut rng);
        let set: Vec<Node> = serde_json::from_value(value).expect("the schema generates nodes");

        // Whatever the graph, the index is a well-formed buffer and every row
        // carries its own grade — which is the clause, asked of the run rather
        // than of an example.
        let index = requirements_buffer(set.clone());
        assert_eq!(faults(index.clone()), vec![], "the index had a malformed span");
        let graded = index
            .spans
            .iter()
            .filter(|span| matches!(span.role, Role::Level { .. }))
            .count();
        assert!(graded <= set.len(), "more grades than requirements");

        if set.is_empty() {
            empty += 1;
        }
        if set.len() > 1 {
            several += 1;
        }
        if set.iter().any(|node| node.level == Level::L1) {
            lowest += 1;
        }
        if set.iter().any(|node| node.level == Level::L4) {
            highest += 1;
        }

        let rows = design_rows(set.clone());
        if rows.is_empty() {
            nothing += 1;
        }
        if rows.iter().any(|row| row.indent > 0) {
            under += 1;
        }
        // One requirement refining two roots is drawn under both, which is the
        // case a walk that marked nodes visited would get wrong.
        let drawn_twice = set.iter().any(|node| {
            rows.iter().filter(|row| row.node.id == node.id).count() > 1
        });
        if drawn_twice {
            twice += 1;
        }
        if set.iter().any(|node| !rows.iter().any(|row| row.node.id == node.id)) {
            unreached += 1;
        }
        // A two-cycle: each names the other. The bound is what keeps the walk a
        // function, and a run without one never exercises it.
        let cycle = set.iter().any(|a| {
            set.iter().any(|b| {
                a.id != b.id && a.refines.contains(&b.id) && b.refines.contains(&a.id)
            })
        });
        if cycle {
            cyclic += 1;
        }
    }
    support::covered(
        "REQ-SHOW.index_from_requirements",
        &[
            ("no requirements at all", empty),
            ("a requirement whose evidence reached L1", lowest),
            ("a requirement whose evidence reached L4", highest),
            ("two requirements or more", several),
        ],
    );
    support::covered(
        "REQ-SHOW.graph_from_refinement",
        &[
            ("nothing to draw", nothing),
            ("a root with something under it", under),
            ("a requirement refining two others, drawn under each", twice),
            ("a requirement no root reaches, drawn nowhere", unreached),
            ("a cycle in refines", cyclic),
        ],
    );
}

/// The sandbox station's rows, and the pricing behind the last of them.
///
/// @tests REQ-SHOW.sandbox_from_observation
/// @tests REQ-COST.cost_from_usage
/// @tests REQ-COST.cache_priced_apart
/// @tests REQ-COST.estimate_is_labelled
/// @tests REQ-DRT-COVER.law_coverage
#[test]
fn generation_reaches_unpriced_models_and_every_kind_of_token() {
    use tracelean_core::drt::gen;
    use tracelean_core::observe::cost::{amount_of, estimate_line, spend_of, Price, Spend, Usage};
    use tracelean_core::observe::transcript::Event;
    use tracelean_core::surface::produce::sandbox_events;

    let mut table_fields = BTreeMap::new();
    table_fields
        .insert("table".to_string(), Schema::List { inner: Box::new(price()), max_len: Some(2) });
    table_fields
        .insert("usage".to_string(), Schema::List { inner: Box::new(usage()), max_len: Some(3) });
    let schema = Schema::Struct { fields: table_fields };

    let mut rng = gen::Rng::new(79);
    let (mut none, mut all_priced, mut some_unpriced, mut repeated) = (0u64, 0u64, 0u64, 0u64);
    let (mut only_input, mut only_cached, mut only_write, mut only_output, mut truncated) =
        (0u64, 0u64, 0u64, 0u64, 0u64);
    for _ in 0..3_000 {
        let value = gen::value(&schema, &mut rng);
        let table: Vec<Price> = serde_json::from_value(value["table"].clone()).unwrap();
        let records: Vec<Usage> = serde_json::from_value(value["usage"].clone()).unwrap();

        let spend = spend_of(table.clone(), records.clone());
        // Named once each, however many records mentioned it: a list that
        // repeated a model would read as several problems where there is one.
        let mut seen = spend.unpriced.clone();
        seen.sort();
        seen.dedup();
        assert_eq!(seen.len(), spend.unpriced.len(), "an unpriced model was named twice");
        // Nothing the table does not price contributes to the total.
        for name in &spend.unpriced {
            assert!(
                !table.iter().any(|price| &price.model == name),
                "`{name}` was reported unpriced and the table prices it"
            );
        }

        if records.is_empty() {
            none += 1;
        } else if spend.unpriced.is_empty() {
            all_priced += 1;
        } else {
            some_unpriced += 1;
        }
        let unpriced_named: Vec<&String> = records
            .iter()
            .map(|record| &record.model)
            .filter(|model| spend.unpriced.contains(model))
            .collect();
        if unpriced_named.len() > spend.unpriced.len() {
            repeated += 1;
        }

        for record in &records {
            let only = |a: u64, b: u64, c: u64| a != 0 && b == 0 && c == 0;
            if only(record.input, record.cached, record.cache_write) && record.output == 0 {
                only_input += 1;
            }
            if only(record.cached, record.input, record.cache_write) && record.output == 0 {
                only_cached += 1;
            }
            if only(record.cache_write, record.input, record.cached) && record.output == 0 {
                only_write += 1;
            }
            if only(record.output, record.input, record.cached) && record.cache_write == 0 {
                only_output += 1;
            }
            if let Some(price) = table.iter().find(|price| price.model == record.model) {
                if amount_of(price.clone(), record.clone()) == 0 {
                    truncated += 1;
                }
            }
        }
    }
    support::covered(
        "REQ-COST.cost_from_usage",
        &[
            ("no usage at all", none),
            ("every model priced", all_priced),
            ("a model the table does not price", some_unpriced),
            ("the same unpriced model twice", repeated),
        ],
    );
    support::covered(
        "REQ-COST.cache_priced_apart",
        &[
            ("ordinary input only", only_input),
            ("cached input only", only_cached),
            ("a cache write only", only_write),
            ("output only", only_output),
            ("a total below one millionth, which truncates to nothing", truncated),
        ],
    );

    // The estimate, and the rows it is the last of.
    let mut rng = gen::Rng::new(89);
    let schema = spend();
    let (mut nil, mut padded, mut one, mut many) = (0u64, 0u64, 0u64, 0u64);
    let (mut quiet, mut changed_something, mut said_something, mut unpriced_shown) =
        (0u64, 0u64, 0u64, 0u64);
    for at in 0..3_000u64 {
        let value = gen::value(&schema, &mut rng);
        let spend: Spend = serde_json::from_value(value).expect("the schema generates a spend");
        let line = estimate_line(spend.clone());
        // The clause, over every generated spend: it reads as an estimate.
        assert!(line.starts_with("estimated "), "`{line}` does not say it is an estimate");

        if spend.amount == 0 {
            nil += 1;
        }
        if spend.amount % 1_000_000 != 0 && spend.amount % 1_000_000 < 100_000 {
            padded += 1;
        }
        match spend.unpriced.len() {
            0 => {}
            1 => one += 1,
            _ => many += 1,
        }

        // And the rows it belongs to. The estimate is always the last one, so a
        // reader never has to look for it.
        let changed: Vec<String> =
            if at % 3 == 0 { vec![] } else { vec![format!("file{}.rs", at % 5)] };
        let said: Vec<Event> = if at % 2 == 0 {
            vec![]
        } else {
            vec![Event { kind: "say".into(), text: "working".into() }]
        };
        let rows = sandbox_events(changed.clone(), said.clone(), spend.clone());
        assert_eq!(rows.last().map(|row| row.kind.as_str()), Some("cost"));
        if changed.is_empty() && said.is_empty() {
            quiet += 1;
        }
        if !changed.is_empty() {
            changed_something += 1;
        }
        if !said.is_empty() {
            said_something += 1;
        }
        if !spend.unpriced.is_empty() {
            unpriced_shown += 1;
        }
    }
    support::covered(
        "REQ-COST.estimate_is_labelled",
        &[
            ("nothing spent", nil),
            ("a fractional part needing padding", padded),
            ("one model unpriced", one),
            ("several models unpriced", many),
        ],
    );
    support::covered(
        "REQ-SHOW.sandbox_from_observation",
        &[
            ("nothing changed and nothing said", quiet),
            ("something changed", changed_something),
            ("the tool said something", said_something),
            ("an estimate with something unpriced", unpriced_shown),
        ],
    );
}

/// Every station on the bar produces something, and nothing else does.
///
/// @tests REQ-SCREEN.station_produces_a_buffer
/// @tests REQ-DRT-COVER.law_coverage
#[test]
fn generation_reaches_every_station_and_a_name_that_is_none() {
    use tracelean_core::drt::gen;
    use tracelean_core::surface::screen::{station_entries, station_kind};

    // The claim the bar rests on: every row drawn leads somewhere. A station
    // that is on the screen and does nothing is the worse of the two failures
    // available — the screen says it is there and pressing it is silence.
    for entry in station_entries() {
        assert!(
            station_kind(entry.key.clone()).is_some(),
            "the `{}` station is drawn and produces nothing",
            entry.key
        );
    }

    let mut fields = BTreeMap::new();
    fields.insert(
        "station".to_string(),
        Schema::Str {
            max_len: Some(0),
            examples: vec![
                "project".into(),
                "sandbox".into(),
                "requirements".into(),
                "design".into(),
                "history".into(),
                "Project".into(),
            ],
        },
    );
    let schema = Schema::Struct { fields };
    let mut rng = gen::Rng::new(73);
    let (mut real, mut absent) = (0u64, 0u64);
    let mut reached: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
    for _ in 0..2_000 {
        let value = gen::value(&schema, &mut rng);
        let name = value["station"].as_str().unwrap().to_string();
        match station_kind(name.clone()) {
            Some(_) => {
                real += 1;
                reached.insert(name);
            }
            None => absent += 1,
        }
    }
    support::covered(
        "REQ-SCREEN.station_produces_a_buffer",
        &[
            ("a station that exists", real),
            ("a name that is no station", absent),
            ("each of the five stations", reached.len() as u64),
        ],
    );
}
