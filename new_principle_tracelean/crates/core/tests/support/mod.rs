//! Shared by the differential suites: judging a run against declared floors.
//!
//! Every suite used to end in a row of `assert!(count > n)` lines. Those are
//! coverage floors written by hand, and writing them by hand meant each suite
//! reported a failure its own way and none of them could tell a situation the
//! generator never reached from one it reached too rarely. Routing them through
//! `drt::coverage` makes the floors the same kind of object the bindings
//! declare, and makes the vacuous case a named verdict rather than a sentence
//! in an assertion message.

#![allow(dead_code)]

use std::collections::BTreeMap;

use tracelean_core::drt::coverage::{level, verdict, Floor, Observed, Verdict};
use tracelean_core::drt::schema::Schema;
use tracelean_core::evidence::Level;

/// The schema of `surface::view::Role`, in one place.
///
/// Three suites generate roles and the enum is no longer flat — `level` carries
/// its grade — so describing it three times would mean three chances to forget
/// a grade and one suite quietly never generating an L4.
pub fn role() -> Schema {
    let mut variants: BTreeMap<String, Option<Box<Schema>>> = BTreeMap::new();
    for nullary in ["plain", "path", "entry", "heading", "requirement", "added", "removed"] {
        variants.insert(nullary.to_string(), None);
    }
    let mut fields = BTreeMap::new();
    fields.insert("grade".to_string(), Schema::simple_enum(&["L1", "L2", "L3", "L4"]));
    variants.insert("level".to_string(), Some(Box::new(Schema::Struct { fields })));
    let mut claim = BTreeMap::new();
    claim.insert(
        "role".to_string(),
        Schema::simple_enum(&["models", "implements", "tests", "drt", "proves", "pins"]),
    );
    variants.insert("claim".to_string(), Some(Box::new(Schema::Struct { fields: claim })));
    Schema::Enum { variants }
}

/// Judge a run: each entry is a situation, the floor it must reach, and how
/// often it was reached.
///
/// Panics with the verdict, so a suite that falls short says which situation
/// and by how much, and a suite whose generator cannot produce a situation at
/// all says that instead.
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

/// The same, for situations that need only occur: a floor of one.
///
/// The verdict distinguishes them from situations that occurred and fell short,
/// which is exactly what a set-membership assertion could not.
pub fn each_occurred(counts: &[(&str, u64)]) {
    let floors: Vec<(&str, u64, u64)> =
        counts.iter().map(|(s, reached)| (*s, 1, *reached)).collect();
    floors_met(&floors);
}

// ─── Earning L3 ──────────────────────────────────────────────────────────────
//
// L3 needs two facts that this project's suites establish in two places: the
// runs agreed (the differential test) and the generator reached the situations
// the binding declared (the generation test). Neither alone is L3, and neither
// test can see the other's result.
//
// So each reports its half, and whichever arrives second composes the record.
// The halves are files rather than memory because the two tests are usually two
// processes. Composing is order-independent: there is no "first" test.

/// Where a half-finished claim waits for its other half.
fn pending_dir() -> std::path::PathBuf {
    root().join(".tracelean").join("pending")
}

/// The project root, worked out here rather than borrowed from `harness`,
/// because not every suite that judges coverage also generates runners.
fn root() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(std::path::Path::parent)
        .expect("the project root is two above the crate")
        .to_path_buf()
}

/// What has been established about a binding so far.
///
/// `agreed` is keyed by seed rather than a flag, because a clause may have more
/// than one implementation: `also_implemented_by` binds a second frontend to the
/// same model, and both are run against it. Recording L3 after one of them
/// agreed would be recording that the clause is checked when half of it is.
///
/// Each half carries the stamp (`earn::drt_stamp`) of the inputs current when
/// it was established, and composes only with halves of the same, current
/// stamp: an "agreed" half left from before a change is about other code.
/// A file in an older shape reads as nothing established.
#[derive(serde::Serialize, serde::Deserialize, Default)]
struct Half {
    /// Seed -> the agreeing run. One entry per agreeing run.
    #[serde(default)]
    agreed: std::collections::BTreeMap<String, Run>,
    /// The stamp under which the generator reached the situations the binding
    /// declared.
    covered: Option<String>,
}

#[derive(serde::Serialize, serde::Deserialize)]
struct Run {
    cases: u64,
    stamp: String,
}

/// The stamp of the inputs an op's records would rest on now.
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

/// Report that a run agreed, and record if coverage has already been judged.
///
/// Takes the result rather than the facts, so a suite cannot report a run other
/// than the one it did: which op, which seed, how many cases and whether
/// anything diverged all come from the run itself.
///
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
    let stamp = current_stamp(&result.op);
    let mut half = read_half(&result.op);
    half.agreed.retain(|_, run| run.stamp == stamp);
    half.agreed.insert(result.seed.to_string(), Run { cases: result.cases, stamp });
    write_half(&result.op, &half);
    compose(&result.op);
}

/// Every class of the arguments was reached by some case, or is waived with a
/// reason in the binding (action plan §10): an agreeing run that never asked a
/// question has not answered it. A waiver that excuses nothing is reported too,
/// so waivers cannot pile up.
///
/// @implements REQ-DRT-COVER.classes_reached
/// @implements REQ-DRT-COVER.waiver_unused_reported
fn every_class_reached(result: &tracelean_core::drt::run::DrtResult) {
    use tracelean_core::drt::coverage::{floors_of, unused_waivers};
    let binding = binding_for(&result.op);
    let floors = floors_of(&result.reached);
    let reached = verdict(floors.clone(), result.reached.clone(), binding.waive.clone());
    if let Verdict::Unmet { gaps } = &reached {
        let names: Vec<&str> = gaps.iter().map(|gap| gap.situation()).collect();
        panic!(
            "`{}` agreed, but no case reached these classes of its arguments, so the run \
             is not L3: {names:?}. Reach them in the generator or waive them with a reason \
             in the binding's `waive`.",
            result.op
        );
    }
    let unused = unused_waivers(floors, result.reached.clone(), binding.waive.clone());
    assert!(unused.is_empty(), "`{}` waives what the run reached or never measured: {unused:?}", result.op);
}

/// Judge a generator against the floors the binding declared, and record if the
/// runs have already agreed.
///
/// The floors come from `.tracelean/drt.json` and the counts from the caller: a
/// situation is a predicate over generated values that no JSON can express, so
/// the binding names it and the suite reports how often the name was reached.
/// A situation the binding never declared is a counting error, not extra
/// credit, and is refused.
///
/// @implements REQ-DRT-COVER.floor_stated
/// @implements REQ-DRT-COVER.law_coverage
/// @implements REQ-DRT-COVER.vacuous_named
pub fn covered(op: &str, counts: &[(&str, u64)]) {
    let binding = binding_for(op);
    assert!(
        !binding.floors.is_empty(),
        "`{op}` declares no coverage floors, so no run of it can reach L3 \
         (REQ-DRT-COVER.floor_stated)"
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

/// The binding an op belongs to.
fn binding_for(op: &str) -> tracelean_core::drt::Binding {
    let bindings = tracelean_core::drt::config::read(&root()).expect("the binding file reads");
    bindings
        .into_iter()
        .find(|binding| binding.op() == op)
        .unwrap_or_else(|| panic!("no binding declares the op `{op}`"))
}

/// Write the record, once both halves are in.
///
/// The level is `coverage::level`'s and never a suite's: agreement without a met
/// floor is not L3, and a met floor without agreement is not L3 either. One
/// record per clause the binding says this call establishes — `also_checks` is a
/// claim the binding made, and the binding-shape tests hold it to it.
///
/// @implements REQ-EVID.ladder
/// @implements REQ-DRT-COVER.floor_unmet_is_not_pass
/// @implements REQ-DRT-BIND.binding_is_the_bond
fn compose(op: &str) {
    let half = read_half(op);
    let root = root();
    let binding = binding_for(op);
    // Every implementation the binding declares, not merely one of them, and
    // both halves established against the inputs as they are now.
    let current = current_stamp(op);
    let stamps: Vec<String> = half.agreed.values().map(|run| run.stamp.clone()).collect();
    if !tracelean_core::trace::earn::halves_compose(
        &stamps,
        half.covered.as_deref(),
        &current,
        binding.implementations().len(),
    ) {
        return;
    }
    // The run the record names, chosen by lowest seed so that the bytes do not
    // depend on which suite finished first. The others are why the record
    // exists at all — it is written only once every implementation agreed —
    // but a `Detail::Drt` names one reproducible run, and this is that one.
    let mut runs: Vec<(u64, u64)> = half
        .agreed
        .iter()
        .filter(|(_, run)| run.stamp == current)
        .filter_map(|(seed, run)| seed.parse::<u64>().ok().map(|seed| (seed, run.cases)))
        .collect();
    // By the seed as a number, not as the string it is stored under: `"103"`
    // sorts before `"29"` and the lowest seed should mean the lowest seed.
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
            // A clause the binding claims but nothing annotates is a finding the
            // checker already reports; a record with no input to go stale
            // against would be worse than no record.
            Err(why) => eprintln!("  no record for {qualified}: {why:?}"),
        }
    }
}
