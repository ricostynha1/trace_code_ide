//! The keymap and staleness, checked against their models.
//!
//! Both are pure total functions over small data, which makes them cheap to
//! model and cheap to check — and both are places where being subtly wrong is
//! invisible until somebody is stuck.

mod harness;

use std::collections::BTreeMap;

use tracelean_core::drt::run::{run, RunOptions};
use tracelean_core::drt::schema::Schema;

mod support;

fn strukt(fields: &[(&str, Schema)]) -> Schema {
    Schema::Struct { fields: fields.iter().map(|(k, v)| (k.to_string(), v.clone())).collect() }
}

fn names(examples: &[&str]) -> Schema {
    Schema::Str {
        max_len: Some(6),
        examples: examples.iter().map(|s| s.to_string()).collect(),
    }
}

// ─── Keymap ──────────────────────────────────────────────────────────────────

fn binding() -> Schema {
    let mut variants: BTreeMap<String, Option<Box<Schema>>> = BTreeMap::new();
    variants.insert(
        "enter".into(),
        Some(Box::new(strukt(&[
            ("mode", names(&["Main", "Options", "File", "Ghost"])),
            ("description", names(&["go"])),
        ]))),
    );
    variants.insert(
        "dispatch".into(),
        Some(Box::new(strukt(&[
            ("action", names(&["undo", "save"])),
            ("description", names(&["do"])),
        ]))),
    );
    Schema::Enum { variants }
}

fn keymap_schema() -> Schema {
    let mode = strukt(&[
        (
            "parent",
            Schema::Option { inner: Box::new(names(&["Main", "Options", "File"])) },
        ),
        (
            "bindings",
            Schema::List {
                inner: Box::new(Schema::Tuple {
                    items: vec![names(&["u", "f", "o", "Escape", "C-."]), binding()],
                }),
                max_len: Some(3),
            },
        ),
    ]);
    strukt(&[
        ("root", names(&["Main"])),
        (
            "modes",
            Schema::List {
                inner: Box::new(Schema::Tuple {
                    items: vec![names(&["Main", "Options", "File"]), mode],
                }),
                max_len: Some(3),
            },
        ),
    ])
}

fn keymap_input() -> Schema {
    strukt(&[
        ("keymap", keymap_schema()),
        ("mode", names(&["Main", "Options", "File", "Nowhere"])),
        ("key", names(&["u", "f", "o", "Escape", "C-.", "q", "z"])),
    ])
}

/// No key in any mode is ever swallowed, and the model agrees about which.
///
/// @drt REQ-MYTH.totality
/// @tests REQ-MYTH.totality
/// @tests REQ-MYTH.outcomes_closed
#[test]
#[ignore = "builds a Lean package and a Rust crate; run with --ignored"]
fn model_and_implementation_agree_on_every_key() {
    let scratch = harness::scratch("keymap");
    let op = "REQ-MYTH.totality";
    let implementation = harness::rust_runner(
        "REQ-MYTH",
        "totality",
        "crates/core/src/surface/keymap.rs::step",
        &scratch,
    );
    let model = harness::lean_runner(
        "TraceLean.Keymap",
        "TraceLean.Keymap.step",
        op,
        &["keymap", "mode", "key"],
        &scratch,
    );

    let result = run(
        op,
        &keymap_input(),
        &model,
        &implementation,
        RunOptions { seed: 23, cases: 3_000, shrink_rounds: 100 },
    )
    .expect("both runners answer");

    support::agreed(&result);

    let _ = std::fs::remove_dir_all(&scratch);
}

/// All four outcomes must actually occur, or agreement says little.
///
/// @tests REQ-DRT-COVER.law_coverage
#[test]
fn generation_reaches_all_four_outcomes() {
    use tracelean_core::drt::gen;
    use tracelean_core::surface::keymap::{step, Keymap, Outcome};

    let schema = keymap_input();
    let mut rng = gen::Rng::new(23);
    let mut seen: std::collections::BTreeMap<&str, u64> = std::collections::BTreeMap::new();
    for _ in 0..3_000 {
        let v = gen::value(&schema, &mut rng);
        let keymap: Keymap = serde_json::from_value(v["keymap"].clone()).unwrap();
        let mode = v["mode"].as_str().unwrap_or_default().to_string();
        let key = v["key"].as_str().unwrap_or_default().to_string();
        *seen
            .entry(match step(keymap, mode, key) {
                Outcome::Enter { .. } => "enter",
                Outcome::Dispatch { .. } => "dispatch",
                Outcome::Leave { .. } => "leave",
                Outcome::PassThrough => "passThrough",
            })
            .or_default() += 1;
    }
    let counts: Vec<(&str, u64)> = ["enter", "dispatch", "leave", "passThrough"]
        .iter()
        .map(|name| (*name, seen.get(name).copied().unwrap_or(0)))
        .collect();
    support::covered("REQ-MYTH.totality", &counts);
}

// ─── Staleness ───────────────────────────────────────────────────────────────

fn staleness_input() -> Schema {
    let hash = names(&["h1", "h2", "h3"]);
    strukt(&[
        (
            "record",
            strukt(&[
                ("linkHash", names(&["link1", "link2"])),
                (
                    "inputs",
                    Schema::List {
                        inner: Box::new(Schema::Tuple {
                            items: vec![names(&["model", "code", "req"]), hash.clone()],
                        }),
                        max_len: Some(3),
                    },
                ),
            ]),
        ),
        (
            "liveLinkHashes",
            Schema::List { inner: Box::new(names(&["link1", "link2"])), max_len: Some(2) },
        ),
        (
            "current",
            Schema::List {
                inner: Box::new(Schema::Tuple {
                    items: vec![names(&["model", "code", "req"]), hash],
                }),
                max_len: Some(3),
            },
        ),
    ])
}

/// The soundness core: no evidence survives a change to any of its inputs.
///
/// @drt REQ-STALE.change_invalidates
/// @tests REQ-STALE.change_invalidates
#[test]
#[ignore = "builds a Lean package and a Rust crate; run with --ignored"]
fn model_and_implementation_agree_on_staleness() {
    let scratch = harness::scratch("staleness");
    let op = "REQ-STALE.change_invalidates";
    // The model spells the field `liveLinkHashes`; the implementation's
    // parameter is `live_link_hashes`. A binding renames, and only renames.
    let implementation = harness::rust_runner_with_params(
        "REQ-STALE",
        "change_invalidates",
        "crates/core/src/trace/record.rs::staleness_of",
        &[("liveLinkHashes", "live_link_hashes")],
        &scratch,
    );
    let model = harness::lean_runner(
        "TraceLean.Staleness",
        "TraceLean.Staleness.staleness",
        op,
        &["record", "liveLinkHashes", "current"],
        &scratch,
    );

    let result = run(
        op,
        &staleness_input(),
        &model,
        &implementation,
        RunOptions { seed: 29, cases: 3_000, shrink_rounds: 100 },
    )
    .expect("both runners answer");

    support::agreed(&result);

    let _ = std::fs::remove_dir_all(&scratch);
}

/// Whether `--drt` runs a clause again: held runs of this op or another,
/// agreed or not, still valid or stale, and asked again or not.
///
/// @drt REQ-STALE.agreed_not_rerun
/// @tests REQ-STALE.agreed_not_rerun
#[test]
#[ignore = "builds a Lean package and a Rust crate; run with --ignored"]
fn model_and_implementation_agree_on_running_a_clause_again() {
    let scratch = harness::scratch("rerun");
    let op = "REQ-STALE.agreed_not_rerun";
    let implementation =
        harness::rust_runner("REQ-STALE", "agreed_not_rerun", "crates/core/src/drt/rerun.rs::rerun", &scratch);
    let model = harness::lean_runner("TraceLean.Staleness", "TraceLean.Staleness.rerun", op, &["held", "op", "live", "again"], &scratch);
    let Schema::Struct { fields: parts } = staleness_input() else { unreachable!() };
    let run_held = strukt(&[
        ("op", names(&["REQ-A.x", "REQ-A.y"])),
        ("agreed", Schema::Bool),
        ("record", parts["record"].clone()),
        ("current", parts["current"].clone()),
    ]);
    let schema = strukt(&[
        ("held", Schema::List { inner: Box::new(run_held), max_len: Some(2) }),
        ("op", names(&["REQ-A.x"])),
        ("live", parts["liveLinkHashes"].clone()),
        ("again", Schema::Bool),
    ]);
    let result = run(op, &schema, &model, &implementation, RunOptions { seed: 31, cases: 3_000, shrink_rounds: 100 })
        .expect("both runners answer");
    support::agreed(&result);
    let _ = std::fs::remove_dir_all(&scratch);
}

/// Both reasons for staleness, and the valid case, must all be reached.
///
/// @tests REQ-DRT-COVER.law_coverage
#[test]
fn generation_reaches_valid_retargeted_and_changed() {
    use tracelean_core::drt::gen;
    use tracelean_core::trace::record::{staleness_of, Staleness, StalenessInput};

    let schema = staleness_input();
    let mut rng = gen::Rng::new(29);
    let mut seen: std::collections::BTreeMap<&str, u64> = std::collections::BTreeMap::new();
    for _ in 0..3_000 {
        let v = gen::value(&schema, &mut rng);
        let record: StalenessInput = serde_json::from_value(v["record"].clone()).unwrap();
        let live: Vec<String> = serde_json::from_value(v["liveLinkHashes"].clone()).unwrap();
        let current: Vec<(String, String)> =
            serde_json::from_value(v["current"].clone()).unwrap();
        *seen
            .entry(match staleness_of(record, live, current) {
                None => "valid",
                Some(Staleness::LinkRetargeted) => "retargeted",
                Some(Staleness::InputChanged { .. }) => "changed",
            })
            .or_default() += 1;
    }
    let counts: Vec<(&str, u64)> = ["valid", "retargeted", "changed"]
        .iter()
        .map(|name| (*name, seen.get(name).copied().unwrap_or(0)))
        .collect();
    support::covered("REQ-STALE.change_invalidates", &counts);
}

// ─── Position encoding ───────────────────────────────────────────────────────

fn encoding_input() -> Schema {
    let mut variants: BTreeMap<String, Option<Box<Schema>>> = BTreeMap::new();
    for name in ["utf8", "utf16", "utf32"] {
        variants.insert(name.to_string(), None);
    }
    strukt(&[
        (
            "lineText",
            Schema::Str {
                max_len: Some(6),
                examples: vec![
                    "".into(),
                    "plain".into(),
                    // Non-ASCII is the whole point: the encodings agree on
                    // everything else, so a run of only ASCII proves nothing.
                    "héllo".into(),
                    "日本語".into(),
                    "😀x".into(),
                    "a😀b".into(),
                ],
            },
        ),
        ("character", Schema::Nat { max: Some(8), edges: vec![0, 1, 2] }),
        ("encoding", Schema::Enum { variants }),
    ])
}

/// @drt REQ-LSP.column_to_offset
/// @tests REQ-LSP.column_to_offset
#[test]
#[ignore = "builds a Lean package and a Rust crate; run with --ignored"]
fn model_and_implementation_agree_on_position_encoding() {
    let scratch = harness::scratch("lsp");
    let op = "REQ-LSP.column_to_offset";
    let implementation = harness::rust_runner_with_params(
        "REQ-LSP",
        "column_to_offset",
        "crates/core/src/surface/lsp.rs::to_byte_offset",
        &[("lineText", "line_text")],
        &scratch,
    );
    let model = harness::lean_runner(
        "TraceLean.Lsp",
        "TraceLean.Lsp.toByteOffset",
        op,
        &["lineText", "character", "encoding"],
        &scratch,
    );

    let result = run(
        op,
        &encoding_input(),
        &model,
        &implementation,
        RunOptions { seed: 31, cases: 2_000, shrink_rounds: 100 },
    )
    .expect("both runners answer");

    support::agreed(&result);

    let _ = std::fs::remove_dir_all(&scratch);
}

/// The encodings only differ on non-ASCII text, so a run that never generated
/// any would agree about nothing worth agreeing about.
///
/// @tests REQ-DRT-COVER.law_coverage
#[test]
fn generation_reaches_positions_where_the_encodings_differ() {
    use tracelean_core::drt::gen;
    use tracelean_core::surface::lsp::{to_byte_offset, Encoding};

    let schema = encoding_input();
    let mut rng = gen::Rng::new(31);
    let (mut differing, mut inside) = (0, 0);
    for _ in 0..2_000 {
        let v = gen::value(&schema, &mut rng);
        let line = v["lineText"].as_str().unwrap_or_default().to_string();
        let character = v["character"].as_u64().unwrap_or(0) as u32;
        let a = to_byte_offset(line.clone(), character, Encoding::Utf8);
        let b = to_byte_offset(line.clone(), character, Encoding::Utf16);
        if a != b {
            differing += 1;
        }
        if to_byte_offset(line, character, Encoding::Utf16).is_none() {
            inside += 1;
        }
    }
    support::covered(
        "REQ-LSP.column_to_offset",
        &[
            ("the encodings disagreed", differing),
            ("a position inside a character or past the end", inside),
        ],
    );
}

/// The same lines, read the other way: a byte offset in, a column out.
fn offset_input() -> Schema {
    let Schema::Struct { mut fields } = encoding_input() else { unreachable!() };
    fields.remove("character");
    fields.insert("offset".to_string(), Schema::Nat { max: Some(10), edges: vec![0, 1, 2, 3] });
    Schema::Struct { fields }
}

/// @drt REQ-LSP.offset_to_column
/// @tests REQ-LSP.offset_to_column
#[test]
#[ignore = "builds a Lean package and a Rust crate; run with --ignored"]
fn model_and_implementation_agree_on_columns_for_offsets() {
    let scratch = harness::scratch("lsp-column");
    let op = "REQ-LSP.offset_to_column";
    let implementation = harness::rust_runner_with_params(
        "REQ-LSP",
        "offset_to_column",
        "crates/core/src/surface/lsp.rs::to_character",
        &[("lineText", "line_text")],
        &scratch,
    );
    let model = harness::lean_runner(
        "TraceLean.Lsp",
        "TraceLean.Lsp.toCharacter",
        op,
        &["lineText", "offset", "encoding"],
        &scratch,
    );

    let result = run(
        op,
        &offset_input(),
        &model,
        &implementation,
        RunOptions { seed: 32, cases: 2_000, shrink_rounds: 100 },
    )
    .expect("both runners answer");

    support::agreed(&result);

    let _ = std::fs::remove_dir_all(&scratch);
}

/// An offset inside a character is the case a careless conversion rounds, and
/// one past the end the case it clamps; both must be reached, and wherever a
/// column comes back, converting it back must return the offset.
///
/// @tests REQ-LSP.offset_to_column
/// @tests REQ-DRT-COVER.law_coverage
#[test]
fn generation_reaches_offsets_inside_characters_and_they_round_trip() {
    use tracelean_core::drt::gen;
    use tracelean_core::surface::lsp::{to_byte_offset, to_character, Encoding};

    let schema = offset_input();
    let mut rng = gen::Rng::new(32);
    let (mut inside, mut past, mut answered) = (0, 0, 0);
    for _ in 0..2_000 {
        let v = gen::value(&schema, &mut rng);
        let line = v["lineText"].as_str().unwrap_or_default().to_string();
        let offset = v["offset"].as_u64().unwrap_or(0) as usize;
        let encoding: Encoding = serde_json::from_value(v["encoding"].clone()).unwrap();
        match to_character(line.clone(), offset, encoding) {
            Some(column) => {
                answered += 1;
                assert_eq!(
                    to_byte_offset(line.clone(), column as u32, encoding),
                    Some(offset),
                    "{line:?} at {offset} in {encoding:?} does not round-trip"
                );
            }
            None if offset > line.len() => past += 1,
            None => inside += 1,
        }
    }
    support::covered(
        "REQ-LSP.offset_to_column",
        &[
            ("an offset inside a character", inside),
            ("an offset past the end", past),
            ("an offset with a column", answered),
        ],
    );
}


// ─── Lowering a server's edits ───────────────────────────────────────────────

fn edit() -> Schema {
    strukt(&[
        ("start", Schema::Nat { max: Some(9), edges: vec![0] }),
        ("end", Schema::Nat { max: Some(9), edges: vec![0] }),
        ("text", Schema::Str { max_len: Some(2), examples: vec!["".into(), "X".into()] }),
    ])
}

fn lower_input() -> Schema {
    strukt(&[
        ("file", Schema::Str { max_len: None, examples: vec!["a.rs".into()] }),
        // Short, and deliberately including a multi-byte character: an offset
        // that splits it is a position the file does not have, and the two
        // sides have to refuse it for the same reason.
        (
            "content",
            Schema::Str {
                max_len: Some(4),
                examples: vec!["".into(), "abcd".into(), "aéb".into(), "日本".into()],
            },
        ),
        ("edits", Schema::List { inner: Box::new(edit()), max_len: Some(3) }),
    ])
}

/// Edits applied in the wrong order corrupt a file quietly, and the wrong
/// implementation passes every single-edit test.
///
/// @drt REQ-LSP.edits_ordered
/// @tests REQ-LSP.edits_ordered
/// @tests REQ-LSP.overlap_refused
#[test]
#[ignore = "builds a Lean package and a Rust crate; run with --ignored"]
fn model_and_implementation_agree_on_lowering_edits() {
    let scratch = harness::scratch("lower");
    let op = "REQ-LSP.edits_ordered";
    let implementation = harness::rust_runner(
        "REQ-LSP",
        "edits_ordered",
        "crates/core/src/surface/lsp.rs::lower_outcome",
        &scratch,
    );
    let model = harness::lean_runner(
        "TraceLean.Lsp",
        "TraceLean.Lsp.lower",
        op,
        &["file", "content", "edits"],
        &scratch,
    );

    let result = run(
        op,
        &lower_input(),
        &model,
        &implementation,
        RunOptions { seed: 53, cases: 2_000, shrink_rounds: 100 },
    )
    .expect("both runners answer");

    support::agreed(&result);

    let _ = std::fs::remove_dir_all(&scratch);
}

/// Every refusal the requirement names must occur, and so must a success with
/// more than one edit — the case the ordering property is about.
///
/// @tests REQ-DRT-COVER.law_coverage
#[test]
fn generation_reaches_every_lowering_outcome() {
    use tracelean_core::drt::gen;
    use tracelean_core::surface::lsp::{lower, Edit, LowerError};

    let schema = lower_input();
    let mut rng = gen::Rng::new(53);
    let mut seen: std::collections::BTreeMap<&str, u64> = std::collections::BTreeMap::new();
    let mut multi = 0;
    for _ in 0..2_000 {
        let v = gen::value(&schema, &mut rng);
        let edits: Vec<Edit> = serde_json::from_value(v["edits"].clone()).unwrap();
        let n = edits.len();
        match lower(
            v["file"].as_str().unwrap().to_string(),
            v["content"].as_str().unwrap().to_string(),
            edits,
        ) {
            Ok(commands) => {
                *seen.entry("ok").or_default() += 1;
                if n > 1 && !commands.is_empty() {
                    multi += 1;
                }
            }
            Err(LowerError::Overlap { .. }) => {
                *seen.entry("overlap").or_default() += 1;
            }
            Err(LowerError::OutOfRange { .. }) => {
                *seen.entry("outOfRange").or_default() += 1;
            }
            Err(LowerError::Inverted { .. }) => {
                *seen.entry("inverted").or_default() += 1;
            }
        }
    }
    let mut counts: Vec<(&str, u64)> = ["ok", "overlap", "outOfRange", "inverted"]
        .iter()
        .map(|name| (*name, seen.get(name).copied().unwrap_or(0)))
        .collect();
    counts.push(("more than one edit lowered successfully", multi));
    support::covered("REQ-LSP.edits_ordered", &counts);
}

/// A sweep must account for every record it was given: nothing rewritten,
/// nothing dropped.
///
/// The single-record law says *whether* a record is stale. This says what the
/// scanner then does with a whole set of them, which is where omission would
/// hide — a stale record that vanished would leave its clause looking untested
/// rather than looking like a claim that has expired.
///
/// @drt REQ-STALE.scanner_never_writes
/// @tests REQ-STALE.scanner_never_writes
/// @tests REQ-STALE.stale_is_visible
#[test]
#[ignore = "builds a Lean package and a Rust crate; run with --ignored"]
fn model_and_implementation_agree_on_a_whole_sweep() {
    let scratch = harness::scratch("sweep");
    let op = "REQ-STALE.scanner_never_writes";
    let implementation = harness::rust_runner_with_params(
        "REQ-STALE",
        "scanner_never_writes",
        "crates/core/src/trace/record.rs::sweep",
        &[("liveLinkHashes", "live_link_hashes")],
        &scratch,
    );
    let model = harness::lean_runner(
        "TraceLean.Staleness",
        "TraceLean.Staleness.sweep",
        op,
        &["records", "liveLinkHashes", "current"],
        &scratch,
    );

    let Schema::Struct { fields } = staleness_input() else { panic!("a struct") };
    let schema = strukt(&[
        (
            "records",
            Schema::List { inner: Box::new(fields["record"].clone()), max_len: Some(4) },
        ),
        ("liveLinkHashes", fields["liveLinkHashes"].clone()),
        ("current", fields["current"].clone()),
    ]);

    let result = run(
        op,
        &schema,
        &model,
        &implementation,
        RunOptions { seed: 31, cases: 2_000, shrink_rounds: 100 },
    )
    .expect("both runners answer");

    support::agreed(&result);

    let _ = std::fs::remove_dir_all(&scratch);
}

/// Nothing comes back that was not produced.
///
/// A stale record is not repaired, re-judged or forgiven. Stated here as the
/// property of `revalidated` that matters: its answer is always a subset of
/// what the backend produced, and an empty production revalidates nothing
/// however stale the records are.
///
/// @drt REQ-STALE.no_silent_revalidation
/// @tests REQ-STALE.no_silent_revalidation
#[test]
fn a_stale_record_never_revives_itself() {
    use tracelean_core::trace::record::{revalidated, StalenessInput};

    let stale: Vec<StalenessInput> = ["link1", "link2"]
        .iter()
        .map(|h| StalenessInput { link_hash: (*h).to_string(), inputs: vec![] })
        .collect();

    assert!(
        revalidated(stale.clone(), vec![]).is_empty(),
        "a record came back with nothing produced"
    );

    let produced = vec![
        StalenessInput { link_hash: "link1".into(), inputs: vec![("m".into(), "h9".into())] },
        StalenessInput { link_hash: "link9".into(), inputs: vec![] },
    ];
    let back = revalidated(stale.clone(), produced.clone());
    assert_eq!(back.len(), 1, "exactly the produced record for a stale key comes back");
    assert_eq!(back[0], produced[0], "the record that came back is the produced one");
    assert!(
        !stale.contains(&back[0]),
        "the record that came back is the stale one, rewritten"
    );
}

/// Which records replace stale ones, checked against the model.
///
/// @tests REQ-STALE.no_silent_revalidation
#[test]
#[ignore = "builds a Lean package and a Rust crate; run with --ignored"]
fn model_and_implementation_agree_on_what_comes_back() {
    let scratch = harness::scratch("revalidated");
    let op = "REQ-STALE.no_silent_revalidation";
    let implementation = harness::rust_runner(
        "REQ-STALE",
        "no_silent_revalidation",
        "crates/core/src/trace/record.rs::revalidated",
        &scratch,
    );
    let model = harness::lean_runner(
        "TraceLean.Staleness",
        "TraceLean.Staleness.revalidated",
        op,
        &["stale", "produced"],
        &scratch,
    );

    let Schema::Struct { fields } = staleness_input() else { panic!("a struct") };
    let record = fields["record"].clone();
    let schema = strukt(&[
        ("stale", Schema::List { inner: Box::new(record.clone()), max_len: Some(3) }),
        ("produced", Schema::List { inner: Box::new(record), max_len: Some(3) }),
    ]);

    let result = run(
        op,
        &schema,
        &model,
        &implementation,
        RunOptions { seed: 37, cases: 2_000, shrink_rounds: 100 },
    )
    .expect("both runners answer");

    support::agreed(&result);

    let _ = std::fs::remove_dir_all(&scratch);
}

// ─── Which server, and what it declared ──────────────────────────────────────

fn server_state() -> Schema {
    let mut variants: BTreeMap<String, Option<Box<Schema>>> = BTreeMap::new();
    variants.insert("notStarted".into(), None);
    variants.insert("starting".into(), None);
    variants.insert(
        "running".into(),
        Some(Box::new(strukt(&[("encoding", {
            let mut e: BTreeMap<String, Option<Box<Schema>>> = BTreeMap::new();
            for name in ["utf8", "utf16", "utf32"] {
                e.insert(name.to_string(), None);
            }
            Schema::Enum { variants: e }
        })]))),
    );
    variants.insert(
        "failed".into(),
        Some(Box::new(strukt(&[(
            "reason",
            Schema::Str { max_len: Some(1), examples: vec!["no binary".into(), "".into()] },
        )]))),
    );
    Schema::Enum { variants }
}

/// The absence of a server is named, never rendered as an empty result.
///
/// @drt REQ-LSP.absent_server_named
/// @tests REQ-LSP.absent_server_named
#[test]
#[ignore = "builds a Lean package and a Rust crate; run with --ignored"]
fn model_and_implementation_agree_on_what_to_show_without_a_server() {
    let scratch = harness::scratch("lsp-present");
    let op = "REQ-LSP.absent_server_named";
    let implementation = harness::rust_runner(
        "REQ-LSP",
        "absent_server_named",
        "crates/core/src/surface/lsp.rs::present_text",
        &scratch,
    );
    let model = harness::lean_runner(
        "TraceLean.Lsp",
        "TraceLean.Lsp.presentText",
        op,
        &["state", "answer"],
        &scratch,
    );

    let schema = strukt(&[
        ("state", server_state()),
        (
            "answer",
            Schema::Option {
                inner: Box::new(Schema::Str {
                    max_len: Some(1),
                    examples: vec!["hover text".into(), "".into()],
                }),
            },
        ),
    ]);

    let result = run(
        op,
        &schema,
        &model,
        &implementation,
        RunOptions { seed: 67, cases: 2_000, shrink_rounds: 100 },
    )
    .expect("both runners answer");

    support::agreed(&result);

    let _ = std::fs::remove_dir_all(&scratch);
}

/// One server per language, and the encoding read off the running one.
///
/// @drt REQ-LSP.registry_per_language
/// @tests REQ-LSP.registry_per_language
/// @tests REQ-LSP.encoding_declared
#[test]
#[ignore = "builds a Lean package and a Rust crate; run with --ignored"]
fn model_and_implementation_agree_on_which_encoding_a_language_uses() {
    let scratch = harness::scratch("lsp-registry");
    let op = "REQ-LSP.registry_per_language";
    let implementation = harness::rust_runner(
        "REQ-LSP",
        "registry_per_language",
        "crates/core/src/surface/lsp.rs::encoding_for",
        &scratch,
    );
    let model = harness::lean_runner(
        "TraceLean.Lsp",
        "TraceLean.Lsp.encodingFor",
        op,
        &["registry", "language"],
        &scratch,
    );

    let language =
        Schema::Str { max_len: Some(0), examples: vec!["rust".into(), "lean".into(), "".into()] };
    let schema = strukt(&[
        (
            "registry",
            Schema::List {
                inner: Box::new(Schema::Tuple {
                    items: vec![language.clone(), server_state()],
                }),
                max_len: Some(3),
            },
        ),
        ("language", language),
    ]);

    let result = run(
        op,
        &schema,
        &model,
        &implementation,
        RunOptions { seed: 71, cases: 2_000, shrink_rounds: 100 },
    )
    .expect("both runners answer");

    support::agreed(&result);

    let _ = std::fs::remove_dir_all(&scratch);
}

/// Every server state, and both the registered and unregistered case, must
/// occur — a run that only ever asked about a running server would say nothing
/// about the clause that exists for the others.
///
/// @tests REQ-DRT-COVER.law_coverage
#[test]
fn generation_reaches_every_server_state_and_both_lookups() {
    use tracelean_core::drt::gen;
    use tracelean_core::surface::lsp::{encoding_for, Display, ServerState};

    let language =
        Schema::Str { max_len: Some(0), examples: vec!["rust".into(), "lean".into(), "".into()] };
    let schema = strukt(&[
        (
            "registry",
            Schema::List {
                inner: Box::new(Schema::Tuple {
                    items: vec![language.clone(), server_state()],
                }),
                max_len: Some(3),
            },
        ),
        ("language", language),
    ]);

    let mut rng = gen::Rng::new(71);
    let (mut answered, mut unavailable) = (0, 0);
    let mut states = std::collections::BTreeSet::new();
    for _ in 0..2_000 {
        let value = gen::value(&schema, &mut rng);
        let registry: Vec<(String, ServerState)> =
            serde_json::from_value(value["registry"].clone()).unwrap();
        let language: String = serde_json::from_value(value["language"].clone()).unwrap();
        for (_, state) in &registry {
            states.insert(match state {
                ServerState::NotStarted => "notStarted",
                ServerState::Starting => "starting",
                ServerState::Running { .. } => "running",
                ServerState::Failed { .. } => "failed",
            });
        }
        match encoding_for(registry, language) {
            Display::Result { .. } => answered += 1,
            Display::Unavailable { .. } => unavailable += 1,
        }
    }
    assert_eq!(states.len(), 4, "not every server state was generated");
    support::covered(
        "REQ-LSP.registry_per_language",
        &[
            ("a lookup finding a running server", answered),
            ("a lookup finding none", unavailable),
        ],
    );
}
