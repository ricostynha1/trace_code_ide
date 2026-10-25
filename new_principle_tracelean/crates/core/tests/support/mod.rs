#![allow(dead_code)]

use std::collections::BTreeMap;

use tracelean_core::drt::coverage::{level, verdict, Floor, Observed, Verdict};
use tracelean_core::drt::schema::Schema;
use tracelean_core::evidence::Level;

pub fn role() -> Schema {
    let mut variants: BTreeMap<String, Option<Box<Schema>>> = BTreeMap::new();
    for nullary in ["plain", "path", "entry", "heading", "requirement", "added", "removed"] {
        variants.insert(nullary.to_string(), None);
    }
    variants.insert("mark".to_string(), Some(Box::new(mark_role())));
    let mut claim = BTreeMap::new();
    claim.insert(
        "role".to_string(),
        Schema::simple_enum(&["models", "implements", "tests", "drt", "proves", "pins"]),
    );
    variants.insert("claim".to_string(), Some(Box::new(Schema::Struct { fields: claim })));
    Schema::Enum { variants }
}

pub fn marks() -> Schema {
    let mut fields = BTreeMap::new();
    for mark in ["annotated", "judged", "tested", "proved", "structural"] {
        fields.insert(mark.to_string(), Schema::Bool);
    }
    Schema::Struct { fields }
}

pub fn mark_role() -> Schema {
    let mut fields = BTreeMap::new();
    fields.insert("mark".to_string(), Schema::simple_enum(&["annotated", "judged", "tested", "proved"]));
    fields.insert("held".to_string(), Schema::Bool);
    Schema::Struct { fields }
}

pub fn floors_met(counts: &[(&str, u64, u64)]) {
    let floors: Vec<Floor> = counts
        .iter()
        .map(|(s, at_least, _)| Floor { situation: (*s).into(), at_least: *at_least })
        .collect();
    let observed: Vec<Observed> = counts
        .iter()
        .map(|(s, _, reached)| Observed { situation: (*s).into(), reached: *reached })
        .collect();
    let reached = verdict(floors, observed, Vec::new());
    assert_eq!(reached, Verdict::Met, "the run did not reach its declared floor");
    assert_eq!(
        level(true, reached),
        Level::L3,
        "an agreeing run that met its floor is what L3 means"
    );
}

pub fn each_occurred(counts: &[(&str, u64)]) {
    let floors: Vec<(&str, u64, u64)> =
        counts.iter().map(|(s, reached)| (*s, 1, *reached)).collect();
    floors_met(&floors);
}

fn pending_dir() -> std::path::PathBuf {
    root().join(".tracelean").join("pending")
}

fn root() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(std::path::Path::parent)
        .expect("the project root is two above the crate")
        .to_path_buf()
}

#[derive(serde::Serialize, serde::Deserialize, Default)]
struct Half {
    #[serde(default)]
    agreed: std::collections::BTreeMap<String, Run>,
    covered: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    lines: Option<String>,
}

#[derive(serde::Serialize, serde::Deserialize)]
struct Run {
    cases: u64,
    stamp: String,
}

fn current_stamp(op: &str) -> String {
    let index = tracelean_core::trace::index::build(&root());
    tracelean_core::trace::earn::drt_stamp(&index, &binding_for(op).clauses())
}

fn half_path(op: &str) -> std::path::PathBuf {
    pending_dir().join(format!("{}.json", op.replace('/', "_")))
}

fn read_half(op: &str) -> Half {
    std::fs::read_to_string(half_path(op))
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or_default()
}

fn write_half(op: &str, half: &Half) {
    let path = half_path(op);
    std::fs::create_dir_all(path.parent().expect("a parent")).expect("the pending dir");
    std::fs::write(path, serde_json::to_string_pretty(half).expect("a half serialises"))
        .expect("the pending dir is writable");
}

/// @implements REQ-DRT.falsification_only
pub fn agreed(result: &tracelean_core::drt::run::DrtResult) {
    assert!(
        result.agreed(),
        "model and implementation disagree on {}.\n  input: {}\n  model: {:?}\n  impl:  {:?}",
        result.op,
        result.divergence.as_ref().map(|d| d.input.to_string()).unwrap_or_default(),
        result.divergence.as_ref().map(|d| &d.model),
        result.divergence.as_ref().map(|d| &d.implementation),
    );
    every_class_reached(result);
    let waived = binding_for(&result.op).waive.iter().any(|w| w.situations.iter().any(|s| s == "unreadable input"));
    assert!(
        result.unreadable == 0 || waived,
        "{} of {} cases of `{}` were input the implementation could not read: the generator draws a shape the \
         types do not have",
        result.unreadable,
        result.cases,
        result.op
    );
    let stamp = current_stamp(&result.op);
    let mut half = read_half(&result.op);
    half.agreed.retain(|_, run| run.stamp == stamp);
    half.agreed.insert(result.seed.to_string(), Run { cases: result.cases, stamp: stamp.clone() });
    if binding_for(&result.op).floors.is_empty() {
        half.covered = Some(stamp);
    }
    write_half(&result.op, &half);
    compose(&result.op);
    if std::env::var_os("TRACELEAN_DRT_LINES").is_some_and(|v| !v.is_empty()) {
        report_lines(&result.op);
    }
}

fn report_lines(op: &str) {
    use tracelean_core::drt::coverage::{floors_of, line_reach, unused_waivers, verdict, Observed, Verdict};
    let root = root();
    let base = root.join("target").join("tracelean-drt-lines");
    let binary = tracelean_core::drt::rust_runner::package_dir(&root.join("target").join("tracelean-drt-rust-lines"))
        .join("target/release/tracelean-drt-runner");
    let profiled = match tracelean_core::drt::lines_run::profiled_lines(&root, &binary, &base.join(op)) {
        Ok(profiled) => profiled,
        Err(why) => {
            eprintln!("  lines of {op} not measured: {why}");
            return;
        }
    };
    let binding = binding_for(op);
    let index = tracelean_core::trace::index::build(&root);
    let mut measured = Vec::new();
    for qualified in binding.clauses() {
        let (req_id, clause) = match qualified.split_once('.') {
            Some((req, clause)) => (req.to_string(), Some(clause.to_string())),
            None => (qualified.clone(), None),
        };
        for link in index.links.iter().filter(|l| {
            l.role == tracelean_core::trace::annotation::Role::Implements && l.req_id == req_id && l.clause == clause
        }) {
            let anchor = &link.anchor;
            let Some((_, hits)) = profiled.iter().find(|(file, _)| *file == anchor.file) else { continue };
            let Ok(text) = std::fs::read_to_string(root.join(&anchor.file)) else { continue };
            let item: Vec<(u32, String)> = text
                .lines()
                .enumerate()
                .map(|(n, line)| (n as u32 + 1, line.to_string()))
                .filter(|(n, _)| *n > anchor.start_line && *n <= anchor.end_line + 1)
                .collect();
            let first = item.first().map(|(_, line)| line.clone()).unwrap_or_default();
            let observed = line_reach(hits.clone(), item);
            let missed: Vec<String> = match verdict(floors_of(&observed), observed.clone(), binding.waive.clone()) {
                Verdict::Unmet { gaps } => gaps.iter().map(|g| g.situation().to_string()).collect(),
                _ => Vec::new(),
            };
            measured.push((qualified.clone(), anchor.file.clone(), anchor.start_line + 1, first, observed, missed));
        }
    }

    let entry = &binding.implementation.entry;
    let (entry_file, entry_name) = entry.split_once("::").unwrap_or((entry.as_str(), ""));
    let is_entry = |m: &&(String, String, u32, String, Vec<Observed>, Vec<String>)| {
        m.1 == entry_file
            && m.3.split(|c: char| !c.is_alphanumeric() && c != '_').any(|word| word == entry_name)
            && m.3.contains("fn ")
    };
    let mut bound: Vec<_> = measured.iter().filter(is_entry).collect();
    if bound.is_empty() {
        bound = measured
            .iter()
            .filter(|m| m.1 == entry_file && m.0 == op && m.4.iter().any(|o| o.reached > 0))
            .collect();
    }
    assert!(
        !bound.is_empty(),
        "`{op}`: its bound entry `{entry}` is no item claiming the clause, and nothing in its file does: \
         nothing to hold to every line"
    );
    for (_, _, _, _, _, missed) in &bound {
        assert!(missed.is_empty(), "`{op}`: lines of `{entry}` its cases never run, and no waiver says why: {missed:#?}");
    }
    let line_waivers: Vec<_> = binding
        .waive
        .iter()
        .cloned()
        .filter_map(|mut w| {
            w.situations.retain(|s| s.starts_with("line: "));
            (!w.situations.is_empty()).then_some(w)
        })
        .collect();
    let observed: Vec<Observed> = bound.iter().flat_map(|m| m.4.clone()).collect();
    let unused = unused_waivers(floors_of(&observed), observed.clone(), line_waivers);
    assert!(unused.is_empty(), "`{op}` waives lines of `{entry}` its cases run, or that are not in it: {unused:?}");

    let items: Vec<serde_json::Value> = measured
        .iter()
        .map(|(clause, file, from, _, observed, missed)| {
            serde_json::json!({ "clause": clause, "file": file, "from": from, "lines": observed.len(), "missed": missed })
        })
        .collect();
    let report = serde_json::json!({ "op": op, "items": items });
    let _ = std::fs::write(base.join(format!("{op}.json")), serde_json::to_string_pretty(&report).unwrap_or_default());

    let mut half = read_half(op);
    half.lines = Some(current_stamp(op));
    write_half(op, &half);
    compose(op);
}

/// @implements REQ-DRT-COVER.classes_reached
/// @implements REQ-DRT-COVER.waiver_unused_reported
fn every_class_reached(result: &tracelean_core::drt::run::DrtResult) {
    use tracelean_core::drt::coverage::{floors_of, unused_waivers};
    let binding = binding_for(&result.op);
    let waive: Vec<_> = binding
        .waive
        .iter()
        .cloned()
        .filter_map(|mut w| {
            w.situations.retain(|s| !s.starts_with("line: ") && s != "unreadable input");
            (!w.situations.is_empty()).then_some(w)
        })
        .collect();
    let floors = floors_of(&result.reached);
    let reached = verdict(floors.clone(), result.reached.clone(), waive.clone());
    if let Verdict::Unmet { gaps } = &reached {
        let names: Vec<&str> = gaps.iter().map(|gap| gap.situation()).collect();
        panic!(
            "`{}` agreed, but no case reached these classes of its arguments, so the run \
             is not L3: {names:?}. Reach them in the generator or waive them with a reason \
             in the binding's `waive`.",
            result.op
        );
    }
    let unused = unused_waivers(floors, result.reached.clone(), waive);
    assert!(unused.is_empty(), "`{}` waives what the run reached or never measured: {unused:?}", result.op);
}

/// @implements REQ-DRT-COVER.floor_stated
/// @implements REQ-DRT-COVER.law_coverage
/// @implements REQ-DRT-COVER.vacuous_named
pub fn covered(op: &str, counts: &[(&str, u64)]) {
    let binding = binding_for(op);
    assert!(
        !binding.floors.is_empty(),
        "`{op}` declares no floors to count against; its coverage is its classes, \
         recorded by `agreed` (REQ-DRT-COVER.floor_stated)"
    );
    for (situation, _) in counts {
        assert!(
            binding.floors.iter().any(|floor| floor.situation == *situation),
            "`{op}` does not declare the situation `{situation}`; the count has nowhere to go"
        );
    }
    let observed: Vec<Observed> = counts
        .iter()
        .map(|(situation, reached)| Observed {
            situation: (*situation).to_string(),
            reached: *reached,
        })
        .collect();
    let reached = verdict(binding.floors.clone(), observed, binding.waive.clone());
    assert_eq!(reached, Verdict::Met, "`{op}` did not reach its declared floor");

    let stamp = current_stamp(op);
    let mut half = read_half(op);
    half.agreed.retain(|_, run| run.stamp == stamp);
    half.covered = Some(stamp);
    write_half(op, &half);
    compose(op);
}

fn binding_for(op: &str) -> tracelean_core::drt::Binding {
    let bindings = tracelean_core::drt::config::read(&root()).expect("the binding file reads");
    bindings
        .into_iter()
        .find(|binding| binding.op() == op)
        .unwrap_or_else(|| panic!("no binding declares the op `{op}`"))
}

/// @implements REQ-EVID.mark_per_bond
/// @implements REQ-DRT-COVER.floor_unmet_is_not_pass
/// @implements REQ-DRT-BIND.binding_is_the_bond
fn compose(op: &str) {
    let half = read_half(op);
    let root = root();
    let binding = binding_for(op);
    let current = current_stamp(op);
    let stamps: Vec<String> = half.agreed.values().map(|run| run.stamp.clone()).collect();
    let lines_in = binding.implementation.language != "rust" || half.lines.as_deref() == Some(current.as_str());
    if !tracelean_core::trace::earn::halves_compose(
        &stamps,
        half.covered.as_deref().filter(|_| lines_in),
        &current,
        binding.implementations().len(),
    ) {
        return;
    }
    let mut runs: Vec<(u64, u64)> = half
        .agreed
        .iter()
        .filter(|(_, run)| run.stamp == current)
        .filter_map(|(seed, run)| seed.parse::<u64>().ok().map(|seed| (seed, run.cases)))
        .collect();
    runs.sort();
    let Some((seed, cases)) = runs.first().copied() else { return };
    let index = tracelean_core::trace::index::build(&root);
    let established = level(true, Verdict::Met);
    assert_eq!(established, Level::L3, "an agreeing run that met its floor is what L3 means");

    for qualified in binding.clauses() {
        let (req_id, clause) = match qualified.split_once('.') {
            Some((req, clause)) => (req.to_string(), Some(clause.to_string())),
            None => (qualified.clone(), None),
        };
        match tracelean_core::trace::earn::drt_record(
            &index,
            &req_id,
            clause.as_deref(),
            established,
            seed,
            cases,
            op,
        ) {
            Ok(record) => {
                tracelean_core::trace::store::write(&root, &record).expect("the store is writable");
            }
            Err(why) => eprintln!("  no record for {qualified}: {why:?}"),
        }
    }
}
