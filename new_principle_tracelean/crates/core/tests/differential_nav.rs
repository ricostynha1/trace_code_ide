mod harness;

use tracelean_core::drt::run::{run, RunOptions};
use tracelean_core::drt::schema::Schema;

fn strukt(fields: &[(&str, Schema)]) -> Schema {
    Schema::Struct { fields: fields.iter().map(|(k, v)| (k.to_string(), v.clone())).collect() }
}

fn text(max_len: usize, examples: &[&str]) -> Schema {
    Schema::Str { max_len: Some(max_len), examples: examples.iter().map(|s| s.to_string()).collect() }
}

fn line_input() -> Schema {
    strukt(&[(
        "line",
        text(
            4,
            &[
                "pub fn go(a: u8) {",
                "    pub(crate) fn go(&self) {",
                "let Kind::File { p } = x",
                "let kind: Kind = x;",
                "theorem open_ok : True",
                "@[simp] theorem t",
                "noncomputable def a",
                "x = open(1)",
                "fn",
                "macro_rules! m",
                "export function f()",
                "struct A<T>",
                "def a::b",
                "",
            ],
        ),
    )])
}

fn open_input() -> Schema {
    strukt(&[
        (
            "files",
            Schema::List {
                inner: Box::new(text(
                    4,
                    &["src/surface/screen.rs", "docs/screenshots.md", "README.md", "a/b_c.rs", "x.rs", "src/view.rs", "a-b/c.d"],
                )),
                max_len: Some(5),
            },
        ),
        ("query", text(3, &["scr", "vw", "LIB", "", " s", "sv", "zzz", "rs", "a/c"])),
        ("limit", Schema::Nat { max: Some(5), edges: vec![0] }),
    ])
}

fn workspace() -> Schema {
    Schema::List {
        inner: Box::new(Schema::Tuple {
            items: vec![
                text(1, &["a.rs", "b.lean", "c.ts"]),
                text(
                    0,
                    &[
                        "pub fn open(a: u8) {}\nfn other() { open(1) }\n",
                        "def canon (w : W) : W := w\ntheorem open_ok : True := trivial\n",
                        "export function open() {}\nconst x = open();\n",
                        "x...\n.. y\n",
                        "OPEN_ok\nopened\nreopen open\n",
                        "let Kind::File { p } = x\nlet kind: Kind = x;\n",
                        "",
                    ],
                ),
            ],
        }),
        max_len: Some(6),
    }
}

fn definitions_input() -> Schema {
    strukt(&[("files", workspace()), ("name", text(1, &["open", "Workspace.canon", "canon", "kind", "Kind", "other", "x"]))])
}

fn uses_input() -> Schema {
    strukt(&[
        ("files", workspace()),
        ("needle", text(1, &["open", "..", "OPEN", "x", "", "(", "open_ok"])),
        ("whole", Schema::Bool),
        ("limit", Schema::Nat { max: Some(6), edges: vec![0] }),
    ])
}

/// @drt REQ-NAV.definitions_found
/// @tests REQ-NAV.definitions_found
#[test]
#[ignore = "builds a Lean package and a Rust crate; run with --ignored"]
fn model_and_implementation_agree_on_where_a_name_is_declared() {
    let scratch = harness::scratch("declarations");
    let op = "REQ-NAV.definitions_found";
    let implementation =
        harness::rust_runner("REQ-NAV", "definitions_found", "crates/core/src/surface/definition.rs::declarations", &scratch);
    let model = harness::lean_runner("TraceLean.Nav", "TraceLean.Nav.declarations", op, &["files", "name"], &scratch);
    let result =
        run(op, &definitions_input(), &model, &implementation, RunOptions { seed: 83, cases: 4_000 })
            .expect("both runners answer");
    harness::agreed(&result);
    let _ = std::fs::remove_dir_all(&scratch);
}

/// @tests REQ-NAV.definitions_found
/// @tests REQ-DRT-COVER.law_coverage
#[test]
fn generation_reaches_names_declared_once_twice_and_never() {
    use tracelean_core::drt::gen;
    use tracelean_core::surface::definition::declarations;

    let mut rng = gen::Rng::new(83);
    let (mut once, mut several, mut dotted, mut never) = (0u64, 0u64, 0u64, 0u64);
    for _ in 0..4_000 {
        let v = gen::value(&definitions_input(), &mut rng);
        let files: Vec<(String, String)> = serde_json::from_value(v["files"].clone()).unwrap();
        let name = v["name"].as_str().unwrap().to_string();
        let found = declarations(files, &name);
        match found.len() {
            0 => never += 1,
            1 => once += 1,
            _ => several += 1,
        }
        if name.contains('.') && !found.is_empty() {
            dotted += 1;
        }
    }
    harness::covered(
        "REQ-NAV.definitions_found",
        &[("declared once", once), ("declared several times", several), ("by its last part", dotted), ("declared nowhere", never)],
    );
}

/// @drt REQ-NAV.uses_found
/// @tests REQ-NAV.uses_found
#[test]
#[ignore = "builds a Lean package and a Rust crate; run with --ignored"]
fn model_and_implementation_agree_on_where_a_name_is_used() {
    let scratch = harness::scratch("occurrences");
    let op = "REQ-NAV.uses_found";
    let implementation =
        harness::rust_runner("REQ-NAV", "uses_found", "crates/core/src/surface/definition.rs::occurrences", &scratch);
    let model = harness::lean_runner(
        "TraceLean.Nav",
        "TraceLean.Nav.occurrences",
        op,
        &["files", "needle", "whole", "limit"],
        &scratch,
    );
    let result = run(op, &uses_input(), &model, &implementation, RunOptions { seed: 89, cases: 4_000 })
        .expect("both runners answer");
    harness::agreed(&result);
    let _ = std::fs::remove_dir_all(&scratch);
}

/// @tests REQ-NAV.uses_found
/// @tests REQ-DRT-COVER.law_coverage
#[test]
fn generation_reaches_uses_searches_and_the_limit() {
    use tracelean_core::drt::gen;
    use tracelean_core::surface::definition::occurrences;

    let mut rng = gen::Rng::new(89);
    let (mut uses, mut searches, mut cut, mut none) = (0u64, 0u64, 0u64, 0u64);
    for _ in 0..4_000 {
        let v = gen::value(&uses_input(), &mut rng);
        let files: Vec<(String, String)> = serde_json::from_value(v["files"].clone()).unwrap();
        let needle = v["needle"].as_str().unwrap().to_string();
        let whole = v["whole"].as_bool().unwrap();
        let limit = v["limit"].as_u64().unwrap() as usize;
        let found = occurrences(files.clone(), &needle, whole, limit);
        let all = occurrences(files, &needle, whole, usize::MAX);
        match (found.is_empty(), whole) {
            (false, true) => uses += 1,
            (false, false) => searches += 1,
            (true, _) => none += 1,
        }
        if all.len() > found.len() {
            cut += 1;
        }
    }
    harness::covered(
        "REQ-NAV.uses_found",
        &[("a name's uses", uses), ("a search", searches), ("cut at the limit", cut), ("found nowhere", none)],
    );
}

fn reopen_input() -> Schema {
    let names = || Schema::List { inner: Box::new(text(0, &["a", "b", "c"])), max_len: Some(4) };
    strukt(&[
        ("files", names()),
        ("shown", Schema::Option { inner: Box::new(text(0, &["a", "b", "c"])) }),
        ("existing", names()),
    ])
}

/// @drt REQ-NAV.session_recalled
/// @tests REQ-NAV.session_recalled
#[test]
#[ignore = "builds a Lean package and a Rust crate; run with --ignored"]
fn model_and_implementation_agree_on_what_reopens() {
    let scratch = harness::scratch("reopened");
    let op = "REQ-NAV.session_recalled";
    let implementation =
        harness::rust_runner("REQ-NAV", "session_recalled", "crates/core/src/surface/reopen.rs::reopened", &scratch);
    let model =
        harness::lean_runner("TraceLean.Nav", "TraceLean.Nav.reopened", op, &["files", "shown", "existing"], &scratch);
    let result = run(op, &reopen_input(), &model, &implementation, RunOptions { seed: 79, cases: 1_000 })
        .expect("both runners answer");
    harness::agreed(&result);
    let _ = std::fs::remove_dir_all(&scratch);
}

/// @tests REQ-NAV.session_recalled
/// @tests REQ-DRT-COVER.law_coverage
#[test]
fn generation_reaches_removed_tabs_and_the_front_file() {
    use tracelean_core::drt::gen;
    use tracelean_core::surface::reopen::reopened;

    let mut rng = gen::Rng::new(79);
    let (mut removed, mut front, mut front_gone) = (0u64, 0u64, 0u64);
    for _ in 0..1_000 {
        let v = gen::value(&reopen_input(), &mut rng);
        let files: Vec<String> = serde_json::from_value(v["files"].clone()).unwrap();
        let shown: Option<String> = serde_json::from_value(v["shown"].clone()).unwrap();
        let existing: Vec<String> = serde_json::from_value(v["existing"].clone()).unwrap();
        let back = reopened(files.clone(), shown.clone(), existing.clone());
        assert!(back.iter().all(|p| existing.contains(p)));
        if files.iter().any(|f| !existing.contains(f)) {
            removed += 1;
        }
        match &shown {
            Some(s) if existing.contains(s) => {
                assert_eq!(back.last(), Some(s));
                front += 1
            }
            Some(_) => front_gone += 1,
            None => {}
        }
    }
    harness::covered(
        "REQ-NAV.session_recalled",
        &[("a tab removed since", removed), ("the front file kept", front), ("the front file removed", front_gone)],
    );
}

/// @drt REQ-NAV.declaration_line
/// @tests REQ-NAV.declaration_line
#[test]
#[ignore = "builds a Lean package and a Rust crate; run with --ignored"]
fn model_and_implementation_agree_on_what_a_line_declares() {
    let scratch = harness::scratch("declared-on");
    let op = "REQ-NAV.declaration_line";
    let implementation =
        harness::rust_runner("REQ-NAV", "declaration_line", "crates/core/src/surface/definition.rs::declared_on", &scratch);
    let model = harness::lean_runner("TraceLean.Nav", "TraceLean.Nav.declaredOn", op, &["line"], &scratch);
    let result = run(op, &line_input(), &model, &implementation, RunOptions { seed: 71, cases: 1_000 })
        .expect("both runners answer");
    harness::agreed(&result);
    let _ = std::fs::remove_dir_all(&scratch);
}

/// @tests REQ-NAV.declaration_line
/// @tests REQ-DRT-COVER.law_coverage
#[test]
fn generation_reaches_every_kind_of_line() {
    use tracelean_core::drt::gen;
    use tracelean_core::surface::definition::declared_on;

    let mut rng = gen::Rng::new(71);
    let (mut declared, mut modified, mut path, mut nothing) = (0u64, 0u64, 0u64, 0u64);
    for _ in 0..1_000 {
        let v = gen::value(&line_input(), &mut rng);
        let line = v["line"].as_str().unwrap().to_string();
        match declared_on(&line) {
            Some(_) if line.trim_start().starts_with(['p', 'n', '@', 'e']) => modified += 1,
            Some(_) => declared += 1,
            None if line.contains("::") => path += 1,
            None => nothing += 1,
        }
    }
    harness::covered(
        "REQ-NAV.declaration_line",
        &[("declares a name", declared), ("after modifiers", modified), ("a path", path), ("declares nothing", nothing)],
    );
}

/// @drt REQ-NAV.quick_open
/// @tests REQ-NAV.quick_open
#[test]
#[ignore = "builds a Lean package and a Rust crate; run with --ignored"]
fn model_and_implementation_agree_on_what_quick_open_lists() {
    let scratch = harness::scratch("quick-open");
    let op = "REQ-NAV.quick_open";
    let implementation =
        harness::rust_runner("REQ-NAV", "quick_open", "crates/core/src/surface/quick.rs::matches", &scratch);
    let model =
        harness::lean_runner("TraceLean.Nav", "TraceLean.Nav.quickOpen", op, &["files", "query", "limit"], &scratch);
    let result = run(op, &open_input(), &model, &implementation, RunOptions { seed: 73, cases: 1_000 })
        .expect("both runners answer");
    harness::agreed(&result);
    let _ = std::fs::remove_dir_all(&scratch);
}

/// @tests REQ-NAV.quick_open
/// @tests REQ-DRT-COVER.law_coverage
#[test]
fn generation_reaches_every_kind_of_query() {
    use tracelean_core::drt::gen;
    use tracelean_core::surface::quick::matches;

    let mut rng = gen::Rng::new(73);
    let (mut empty, mut none, mut cut, mut ranked) = (0u64, 0u64, 0u64, 0u64);
    for _ in 0..1_000 {
        let v = gen::value(&open_input(), &mut rng);
        let files: Vec<String> = serde_json::from_value(v["files"].clone()).unwrap();
        let query = v["query"].as_str().unwrap().to_string();
        let limit = v["limit"].as_u64().unwrap() as usize;
        let all = matches(files.clone(), &query, usize::MAX);
        let kept = matches(files.clone(), &query, limit);
        assert_eq!(kept, all.iter().take(limit).cloned().collect::<Vec<_>>(), "the limit only cuts");
        if query.trim().is_empty() {
            empty += 1;
        } else if all.is_empty() && !files.is_empty() {
            none += 1;
        } else if all.len() > 1 {
            ranked += 1;
        }
        if all.len() > kept.len() {
            cut += 1;
        }
    }
    harness::covered(
        "REQ-NAV.quick_open",
        &[("no letters", empty), ("nothing matches", none), ("several ranked", ranked), ("more than kept", cut)],
    );
}
