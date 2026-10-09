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
                // A person, a delegate, and a blank name.
                ("judgedBy", text(&["ana", "sam", "rev", " ", ""])),
                ("delegatedBy", Schema::Option { inner: Box::new(text(&["ana", "bob"])) }),
                ("note", Schema::Option { inner: Box::new(text(&["unclear"])) }),
                ("requirementHash", hash()),
                ("modelHash", hash()),
            ]),
        ),
        ("judges", judges_schema()),
        ("linkHash", text(&["link1", "link2"])),
    ])
}

/// A project's judges file, or none: people who delegate to `rev` or `sam`.
fn judges_schema() -> Schema {
    let names = |names: &[&str]| Schema::Str {
        max_len: Some(3),
        examples: names.iter().map(|s| s.to_string()).collect(),
    };
    let person = strukt(&[
        ("name", names(&["ana", "bob"])),
        ("delegatesTo", Schema::List { inner: Box::new(names(&["rev", "sam"])), max_len: Some(2) }),
    ]);
    Schema::Option {
        inner: Box::new(strukt(&[("people", Schema::List { inner: Box::new(person), max_len: Some(2) })])),
    }
}

/// @drt REQ-JUDGE.caps_at_judgement
/// @tests REQ-JUDGE.caps_at_judgement
/// @tests REQ-JUDGE.proposal_not_mutation
/// @tests REQ-JUDGE.human_decides
/// @tests REQ-JUDGE.drift_recorded
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
        &["material", "judgement", "judges", "linkHash"],
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

/// Every outcome occurs — including a delegate accepted and refused — and the
/// recorded ones never exceed `L2`, drift and `unmodelable` sitting at `L1`:
/// the property stated directly against the implementation, not only through
/// the model.
///
/// @tests REQ-JUDGE.caps_at_judgement
/// @tests REQ-JUDGE.drift_recorded
/// @tests REQ-JUDGE.human_decides
/// @tests REQ-DRT-COVER.law_coverage
#[test]
fn generation_reaches_every_verdict_and_none_exceeds_l2() {
    use tracelean_core::drt::gen;
    use tracelean_core::evidence::Level;
    use tracelean_core::judge::{record, Judgement, Judges, Material, Outcome};
    use tracelean_core::trace::record::Detail;

    let schema = input_schema();
    let mut rng = gen::Rng::new(17);
    let mut seen: std::collections::BTreeMap<&str, u64> = std::collections::BTreeMap::new();
    for _ in 0..2_000 {
        let v = gen::value(&schema, &mut rng);
        let material: Material = serde_json::from_value(v["material"].clone()).unwrap();
        let judgement: Judgement = serde_json::from_value(v["judgement"].clone()).unwrap();
        let judges: Option<Judges> = serde_json::from_value(v["judges"].clone()).unwrap();
        let link = v["linkHash"].as_str().unwrap().to_string();
        let evidence = match record(material, judgement, judges, link) {
            Outcome::Recorded { evidence } => {
                *seen.entry("recorded").or_default() += 1;
                assert_eq!(evidence.level, Level::L2, "a judgement recorded above L2");
                assert_eq!(evidence.effective_level(), Level::L2);
                evidence
            }
            Outcome::Drifted { evidence } => {
                *seen.entry("drifted").or_default() += 1;
                assert_eq!(evidence.level, Level::L1, "a drift raised something");
                evidence
            }
            Outcome::Proposed { evidence, .. } => {
                *seen.entry("proposed").or_default() += 1;
                assert_eq!(evidence.level, Level::L1, "an unmodelable clause raised something");
                evidence
            }
            Outcome::Refused { .. } => {
                *seen.entry("refused").or_default() += 1;
                continue;
            }
        };
        if let Detail::Judge { delegated_by: Some(_), .. } = &evidence.detail {
            *seen.entry("delegated").or_default() += 1;
        }
    }
    let counts: Vec<(&str, u64)> = ["recorded", "drifted", "proposed", "refused", "delegated"]
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
            ("delegatedBy", Schema::Option { inner: Box::new(text(&["bo"])) }),
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

/// A judgement as the requirement view shows it.
fn judged_input() -> Schema {
    let mut verdicts = BTreeMap::new();
    verdicts.insert("L1".to_string(), None);
    verdicts.insert("L2".to_string(), None);
    strukt(&[(
        "judged",
        strukt(&[
            ("verdict", text(&["agrees", "drift", "unmodelable"])),
            ("judgedBy", text(&["ana", "claude-review"])),
            ("delegatedBy", Schema::Option { inner: Box::new(text(&["ricostynha"])) }),
            ("note", Schema::Option { inner: Box::new(text(&["it rounds"])) }),
            ("level", Schema::Enum { variants: verdicts }),
        ]),
    )])
}

/// How a judgement reads, checked against the model: a delegated `L2` must
/// never read as a person's own, nor a drift as an agreement.
///
/// @drt REQ-JUDGE.judgement_shown
/// @tests REQ-JUDGE.judgement_shown
#[test]
#[ignore = "builds a Lean package and a Rust crate; run with --ignored"]
fn model_and_implementation_agree_on_how_a_judgement_reads() {
    let scratch = harness::scratch("judged");
    let op = "REQ-JUDGE.judgement_shown";
    let implementation = harness::rust_runner(
        "REQ-JUDGE",
        "judgement_shown",
        "crates/core/src/surface/requirement_view.rs::judged_text",
        &scratch,
    );
    let model =
        harness::lean_runner("TraceLean.Judge", "TraceLean.Judge.judgedText", op, &["judged"], &scratch);
    let result = run(
        op,
        &judged_input(),
        &model,
        &implementation,
        RunOptions { seed: 23, cases: 1_000, shrink_rounds: 100 },
    )
    .expect("both runners answer");
    support::agreed(&result);
    let _ = std::fs::remove_dir_all(&scratch);
}

/// Each kind of judgement is generated, so the agreement above is about all
/// of them.
///
/// @tests REQ-JUDGE.judgement_shown
/// @tests REQ-DRT-COVER.law_coverage
#[test]
fn generation_reaches_every_kind_of_judgement_shown() {
    use tracelean_core::drt::gen;
    use tracelean_core::surface::requirement_view::{judged_text, Judged};

    let mut rng = gen::Rng::new(23);
    let (mut agreed, mut drift, mut delegated) = (0u64, 0u64, 0u64);
    for _ in 0..1_000 {
        let v = gen::value(&judged_input(), &mut rng);
        let judged: Judged = serde_json::from_value(v["judged"].clone()).unwrap();
        let said = judged_text(judged.clone());
        if judged.verdict == "agrees" {
            agreed += 1;
        } else if judged.verdict == "drift" || judged.verdict == "unmodelable" {
            assert!(said.starts_with(&format!("judged: {}", judged.verdict)), "{said}");
            drift += 1;
        }
        if let Some(person) = &judged.delegated_by {
            assert!(said.contains(&format!("(delegated by {person})")), "{said}");
            delegated += 1;
        }
    }
    support::covered(
        "REQ-JUDGE.judgement_shown",
        &[
            ("an agreement", agreed),
            ("a drift or unmodelable verdict", drift),
            ("a delegated judgement", delegated),
        ],
    );
}
