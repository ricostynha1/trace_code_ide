//! What a judgement is allowed to write, checked against the model.
//!
//! This is the bond with no oracle: a person reads a clause and a model and
//! says whether they agree. The danger is not that they are wrong — they are
//! allowed to be, and the level says so — but that the recording step quietly
//! promotes their reading above what a reading can support. That is a single
//! wrong constant, invisible in review, and it would make every stronger
//! guarantee in the system decorative.

mod harness;

use std::collections::BTreeMap;

use tracelean_core::drt::run::{run, RunOptions};
use tracelean_core::drt::schema::Schema;

mod support;

fn strukt(fields: &[(&str, Schema)]) -> Schema {
    Schema::Struct { fields: fields.iter().map(|(k, v)| (k.to_string(), v.clone())).collect() }
}

fn text(examples: &[&str]) -> Schema {
    Schema::Str {
        max_len: Some(3),
        examples: examples.iter().map(|s| s.to_string()).collect(),
    }
}

fn hash() -> Schema {
    text(&["h1", "h2"])
}

fn verdict() -> Schema {
    let mut variants: BTreeMap<String, Option<Box<Schema>>> = BTreeMap::new();
    variants.insert("agrees".into(), None);
    variants.insert("drift".into(), None);
    variants.insert("unmodelable".into(), None);
    Schema::Enum { variants }
}

fn input_schema() -> Schema {
    strukt(&[
        (
            "material",
            strukt(&[
                ("reqId", text(&["REQ-EVID", "REQ-CMD"])),
                ("clause", Schema::Option { inner: Box::new(text(&["ladder", "weakest_link"])) }),
                ("clauseText", text(&["levels are ordered"])),
                ("modelSource", text(&["def assurance"])),
                ("requirementHash", hash()),
                ("modelHash", hash()),
                ("divergence", Schema::Option { inner: Box::new(text(&["case 7"])) }),
            ]),
        ),
        (
            "judgement",
            strukt(&[
                ("verdict", verdict()),
                ("judgedBy", text(&["ana", "sam"])),
                ("note", Schema::Option { inner: Box::new(text(&["unclear"])) }),
                ("requirementHash", hash()),
                ("modelHash", hash()),
            ]),
        ),
        ("linkHash", text(&["link1", "link2"])),
    ])
}

/// @drt REQ-JUDGE.caps_at_judgement
/// @tests REQ-JUDGE.caps_at_judgement
/// @tests REQ-JUDGE.proposal_not_mutation
#[test]
#[ignore = "builds a Lean package and a Rust crate; run with --ignored"]
fn model_and_implementation_agree_on_what_a_judgement_records() {
    let scratch = harness::scratch("judge");
    let op = "REQ-JUDGE.caps_at_judgement";
    let implementation = harness::rust_runner_with_params(
        "REQ-JUDGE",
        "caps_at_judgement",
        "crates/core/src/judge.rs::record",
        &[("linkHash", "link_hash")],
        &scratch,
    );
    let model = harness::lean_runner(
        "TraceLean.Judge",
        "TraceLean.Judge.record",
        op,
        &["material", "judgement", "linkHash"],
        &scratch,
    );

    let result = run(
        op,
        &input_schema(),
        &model,
        &implementation,
        RunOptions { seed: 17, cases: 2_000, shrink_rounds: 100 },
    )
    .expect("both runners answer");

    support::agreed(&result);

    let _ = std::fs::remove_dir_all(&scratch);
}

/// All three verdicts occur, and the recorded ones never exceed `L2` — the
/// property stated directly against the implementation, not only through the
/// model.
///
/// @tests REQ-JUDGE.caps_at_judgement
/// @tests REQ-DRT-COVER.law_coverage
#[test]
fn generation_reaches_every_verdict_and_none_exceeds_l2() {
    use tracelean_core::drt::gen;
    use tracelean_core::evidence::Level;
    use tracelean_core::judge::{record, Judgement, Material, Outcome};

    let schema = input_schema();
    let mut rng = gen::Rng::new(17);
    let mut seen: std::collections::BTreeMap<&str, u64> = std::collections::BTreeMap::new();
    for _ in 0..2_000 {
        let v = gen::value(&schema, &mut rng);
        let material: Material = serde_json::from_value(v["material"].clone()).unwrap();
        let judgement: Judgement = serde_json::from_value(v["judgement"].clone()).unwrap();
        let link = v["linkHash"].as_str().unwrap().to_string();
        match record(material, judgement, link) {
            Outcome::Recorded { evidence } => {
                *seen.entry("recorded").or_default() += 1;
                assert_eq!(evidence.level, Level::L2, "a judgement recorded above L2");
                assert_eq!(evidence.effective_level(), Level::L2);
            }
            Outcome::Drifted => {
                *seen.entry("drifted").or_default() += 1;
            }
            Outcome::Proposed { .. } => {
                *seen.entry("proposed").or_default() += 1;
            }
        }
    }
    let counts: Vec<(&str, u64)> = ["recorded", "drifted", "proposed"]
        .iter()
        .map(|name| (*name, seen.get(name).copied().unwrap_or(0)))
        .collect();
    support::covered("REQ-JUDGE.caps_at_judgement", &counts);
}

/// When a judgement stops applying.
///
/// A judgement is about a specific pair of texts. If either moves, nobody has
/// judged the pair that now exists — and the dangerous implementation is the
/// one that checks only the requirement, because a model can be rewritten under
/// a judgement without the text changing at all.
///
/// @drt REQ-JUDGE.invalidated_by_change
/// @tests REQ-JUDGE.invalidated_by_change
#[test]
#[ignore = "builds a Lean package and a Rust crate; run with --ignored"]
fn model_and_implementation_agree_on_when_a_judgement_lapses() {
    let scratch = harness::scratch("still-applies");
    let op = "REQ-JUDGE.invalidated_by_change";
    let implementation = harness::rust_runner(
        "REQ-JUDGE",
        "invalidated_by_change",
        "crates/core/src/judge.rs::still_applies_to",
        &scratch,
    );
    let model = harness::lean_runner(
        "TraceLean.Judge",
        "TraceLean.Judge.stillApplies",
        op,
        &["judgement", "material"],
        &scratch,
    );

    let Schema::Struct { fields } = input_schema() else { unreachable!() };
    let schema = Schema::Struct {
        fields: fields
            .into_iter()
            .filter(|(name, _)| name != "linkHash")
            .collect(),
    };

    let result = run(
        op,
        &schema,
        &model,
        &implementation,
        RunOptions { seed: 18, cases: 2_000, shrink_rounds: 100 },
    )
    .expect("both runners answer");

    support::agreed(&result);

    let _ = std::fs::remove_dir_all(&scratch);
}

/// One detail of each kind, which is the whole input to a ceiling.
fn ceiling_input() -> Schema {
    let mut variants: BTreeMap<String, Option<Box<Schema>>> = BTreeMap::new();
    variants.insert(
        "judge".into(),
        Some(Box::new(strukt(&[
            ("verdict", text(&["agrees"])),
            ("judgedBy", text(&["ana"])),
            ("promptVersion", text(&["1"])),
        ]))),
    );
    variants.insert(
        "drt".into(),
        Some(Box::new(strukt(&[
            ("seed", Schema::Nat { max: Some(99), edges: vec![0] }),
            ("cases", Schema::Nat { max: Some(99), edges: vec![0] }),
            ("op", text(&["REQ-X.c"])),
        ]))),
    );
    variants.insert(
        "proof".into(),
        Some(Box::new(strukt(&[
            ("theoremName", text(&["t"])),
            ("toolchain", text(&["4.12.0"])),
        ]))),
    );
    strukt(&[("detail", Schema::Enum { variants })])
}

/// The ceiling per backend, on its own.
///
/// @drt REQ-EVID.judgement_caps
/// @tests REQ-EVID.judgement_caps
#[test]
#[ignore = "builds a Lean package and a Rust crate; run with --ignored"]
fn model_and_implementation_agree_on_what_each_backend_can_establish() {
    let scratch = harness::scratch("ceiling");
    let op = "REQ-EVID.judgement_caps";
    let implementation = harness::rust_runner(
        "REQ-EVID",
        "judgement_caps",
        "crates/core/src/trace/record.rs::detail_ceiling",
        &scratch,
    );
    let model = harness::lean_runner(
        "TraceLean.Judge",
        "TraceLean.Judge.Detail.ceiling",
        op,
        &["detail"],
        &scratch,
    );

    let result = run(
        op,
        &ceiling_input(),
        &model,
        &implementation,
        RunOptions { seed: 19, cases: 1_000, shrink_rounds: 100 },
    )
    .expect("both runners answer");

    support::agreed(&result);

    let _ = std::fs::remove_dir_all(&scratch);
}

/// The prompt a person carries, checked against the model.
///
/// The prompt is the one place a model's opinion enters this system at all, and
/// a prompt that quietly stopped presenting the divergence would make every
/// judgement after it worth less without anything reporting a change.
///
/// @drt REQ-JUDGE.prompt_exported
/// @tests REQ-JUDGE.prompt_exported
/// @tests REQ-JUDGE.divergence_presented
/// @tests REQ-JUDGE.advice_is_not_evidence
#[test]
#[ignore = "builds a Lean package and a Rust crate; run with --ignored"]
fn model_and_implementation_agree_on_the_prompt() {
    let scratch = harness::scratch("prompt");
    let op = "REQ-JUDGE.prompt_exported";
    let implementation = harness::rust_runner(
        "REQ-JUDGE",
        "prompt_exported",
        "crates/core/src/judge.rs::prompt",
        &scratch,
    );
    let model =
        harness::lean_runner("TraceLean.Judge", "TraceLean.Judge.prompt", op, &["material"], &scratch);

    let Schema::Struct { fields } = input_schema() else { panic!("a struct") };
    let schema = strukt(&[("material", fields["material"].clone())]);

    let result = run(
        op,
        &schema,
        &model,
        &implementation,
        RunOptions { seed: 59, cases: 2_000, shrink_rounds: 100 },
    )
    .expect("both runners answer");

    support::agreed(&result);

    let _ = std::fs::remove_dir_all(&scratch);
}

/// A known divergence reaches the prompt, and half the generated prompts have
/// one — otherwise the agreement above would be about the other branch only.
///
/// @tests REQ-JUDGE.divergence_presented
/// @tests REQ-DRT-COVER.law_coverage
#[test]
fn generation_reaches_prompts_with_and_without_a_divergence() {
    use tracelean_core::drt::gen;
    use tracelean_core::judge::{prompt, Material};

    let Schema::Struct { fields } = input_schema() else { panic!("a struct") };
    let schema = strukt(&[("material", fields["material"].clone())]);
    let mut rng = gen::Rng::new(59);
    let (mut with, mut without) = (0, 0);
    for _ in 0..2_000 {
        let value = gen::value(&schema, &mut rng);
        let material: Material = serde_json::from_value(value["material"].clone()).unwrap();
        let known = material.divergence.clone();
        let text = prompt(material);
        assert!(
            text.contains("This is advice"),
            "a prompt did not say that what comes back is advice"
        );
        match known {
            Some(divergence) if !divergence.is_empty() => {
                assert!(
                    text.contains(&divergence),
                    "a known divergence did not reach the prompt"
                );
                with += 1;
            }
            _ => without += 1,
        }
    }
    support::covered(
        "REQ-JUDGE.prompt_exported",
        &[("a prompt carrying a divergence", with), ("a prompt carrying none", without)],
    );
}

/// Each backend's ceiling, asked of each backend.
///
/// The ceiling is one `match` with three arms, and a run that reached only the
/// proof arm would agree about a function it never asked the interesting
/// question. The interesting arm is `judge`: it is the most fallible method and
/// the one whose ceiling being wrong would let the least reliable bond produce
/// the most confident output.
///
/// @tests REQ-EVID.judgement_caps
/// @tests REQ-DRT-COVER.law_coverage
#[test]
fn generation_reaches_every_backend_and_none_exceeds_its_method() {
    use tracelean_core::drt::gen;
    use tracelean_core::evidence::Level;
    use tracelean_core::trace::record::{detail_ceiling, Detail};

    let schema = ceiling_input();
    let mut rng = gen::Rng::new(19);
    let (mut judged, mut tested, mut proved) = (0u64, 0u64, 0u64);
    for _ in 0..1_000 {
        let v = gen::value(&schema, &mut rng);
        let detail: Detail = serde_json::from_value(v["detail"].clone()).unwrap();
        let ceiling = detail_ceiling(detail.clone());
        match detail {
            Detail::Judge { .. } => {
                assert_eq!(ceiling, Level::L2, "a reading established more than a reading can");
                judged += 1;
            }
            Detail::Drt { .. } => {
                assert_eq!(ceiling, Level::L3, "a differential run is not a proof");
                tested += 1;
            }
            Detail::Proof { .. } => {
                assert_eq!(ceiling, Level::L4, "the kernel's word is the top of the ladder");
                proved += 1;
            }
        }
    }
    support::covered(
        "REQ-EVID.judgement_caps",
        &[
            ("a judgement's ceiling", judged),
            ("a differential run's ceiling", tested),
            ("a proof's ceiling", proved),
        ],
    );
}
