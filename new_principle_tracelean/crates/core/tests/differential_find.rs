//! Finding and asking, checked against their models: a text with `*` and `?`,
//! the lines that hold a match, what a key does to a typed answer, what a
//! colour setting accepts, and what the proof panel says of the goals.

mod harness;
mod support;

use tracelean_core::drt::run::{run, RunOptions};
use tracelean_core::drt::schema::Schema;

fn strukt(fields: &[(&str, Schema)]) -> Schema {
    Schema::Struct { fields: fields.iter().map(|(k, v)| (k.to_string(), v.clone())).collect() }
}

fn text(max_len: usize, examples: &[&str]) -> Schema {
    Schema::Str { max_len: Some(max_len), examples: examples.iter().map(|s| s.to_string()).collect() }
}

fn glob_input() -> Schema {
    strukt(&[
        (
            "text",
            text(
                0,
                &[
                    "REQ-ACT.one_path\nREQ-NAV.find_by_glob\n",
                    "cat cot cut\nCAT cOt",
                    "λ x\nλ y",
                    "abc",
                    "",
                    "a\nb\n\nab",
                    "draft one\napproved two\ndraft three",
                    "aaa\nabab",
                ],
            ),
        ),
        ("pattern", text(0, &["req-*.one", "c?t", "*", "**a", "?y", "a*b", "", "x", "a**b", "CAT", "é", "dra*", "a", "ab"])),
    ])
}

/// A text with `*` and `?`, checked against the model.
///
/// @drt REQ-NAV.find_by_glob
/// @tests REQ-NAV.find_by_glob
#[test]
#[ignore = "builds a Lean package and a Rust crate; run with --ignored"]
fn model_and_implementation_agree_on_where_a_pattern_matches() {
    let scratch = harness::scratch("glob");
    let op = "REQ-NAV.find_by_glob";
    let implementation =
        harness::rust_runner("REQ-NAV", "find_by_glob", "crates/core/src/surface/find.rs::glob_matches", &scratch);
    let model = harness::lean_runner("TraceLean.Find", "TraceLean.Find.globMatches", op, &["text", "pattern"], &scratch);
    let result = run(op, &glob_input(), &model, &implementation, RunOptions { seed: 97, cases: 4_000 })
        .expect("both runners answer");
    support::agreed(&result);
    let _ = std::fs::remove_dir_all(&scratch);
}

/// A match, none, several, a star, a question mark, an empty pattern, and a
/// text of more than one line.
///
/// @tests REQ-NAV.find_by_glob
/// @tests REQ-DRT-COVER.law_coverage
#[test]
fn generation_reaches_matches_stars_and_lines() {
    use tracelean_core::drt::gen;
    use tracelean_core::surface::find::glob_matches;

    let mut rng = gen::Rng::new(97);
    let (mut found, mut none, mut several, mut star, mut question, mut empty, mut lines) =
        (0u64, 0u64, 0u64, 0u64, 0u64, 0u64, 0u64);
    for _ in 0..4_000 {
        let v = gen::value(&glob_input(), &mut rng);
        let (text, pattern) = (v["text"].as_str().unwrap().to_string(), v["pattern"].as_str().unwrap().to_string());
        let matches = glob_matches(text.clone(), pattern.clone());
        match matches.len() {
            0 => none += 1,
            1 => found += 1,
            _ => {
                found += 1;
                several += 1
            }
        }
        if pattern.contains('*') && !matches.is_empty() {
            star += 1;
        }
        if pattern.contains('?') && !matches.is_empty() {
            question += 1;
        }
        if pattern.is_empty() {
            empty += 1;
        }
        if text.contains('\n') && !matches.is_empty() {
            lines += 1;
        }
    }
    support::covered(
        "REQ-NAV.find_by_glob",
        &[
            ("a match", found),
            ("no match", none),
            ("several matches", several),
            ("a star that matched", star),
            ("a question mark that matched", question),
            ("an empty pattern", empty),
            ("a match in a text of several lines", lines),
        ],
    );
}

/// The lines that hold a match, checked against the model.
///
/// @drt REQ-NAV.find_lists_lines
/// @tests REQ-NAV.find_lists_lines
#[test]
#[ignore = "builds a Lean package and a Rust crate; run with --ignored"]
fn model_and_implementation_agree_on_which_lines_match() {
    let scratch = harness::scratch("matching-lines");
    let op = "REQ-NAV.find_lists_lines";
    let implementation =
        harness::rust_runner("REQ-NAV", "find_lists_lines", "crates/core/src/surface/find.rs::matching_lines", &scratch);
    let model =
        harness::lean_runner("TraceLean.Find", "TraceLean.Find.matchingLines", op, &["text", "pattern"], &scratch);
    let result = run(op, &glob_input(), &model, &implementation, RunOptions { seed: 101, cases: 4_000 })
        .expect("both runners answer");
    support::agreed(&result);
    let _ = std::fs::remove_dir_all(&scratch);
}

/// Some lines, none, and every line.
///
/// @tests REQ-NAV.find_lists_lines
/// @tests REQ-DRT-COVER.law_coverage
#[test]
fn generation_reaches_some_none_and_every_line() {
    use tracelean_core::drt::gen;
    use tracelean_core::surface::find::matching_lines;

    let mut rng = gen::Rng::new(101);
    let (mut some, mut none, mut every) = (0u64, 0u64, 0u64);
    for _ in 0..4_000 {
        let v = gen::value(&glob_input(), &mut rng);
        let (text, pattern) = (v["text"].as_str().unwrap().to_string(), v["pattern"].as_str().unwrap().to_string());
        let all = text.split('\n').count();
        let listed = matching_lines(text, pattern).len();
        match listed {
            0 => none += 1,
            n if n == all => every += 1,
            _ => some += 1,
        }
    }
    support::covered(
        "REQ-NAV.find_lists_lines",
        &[("some lines match", some), ("no line matches", none), ("every line matches", every)],
    );
}

fn ask_input() -> Schema {
    strukt(&[
        ("answer", text(0, &["", "ab", " ab ", "  ", "new.rs", "é", "a b"])),
        (
            "key",
            text(0, &["a", "Z", "Space", "Backspace", "Enter", "Escape", "Left", "Up", "é", "Tab", "F1", "ab", ""]),
        ),
    ])
}

/// What a key does to a typed answer, checked against the model.
///
/// @drt REQ-NAV.name_is_asked_for
/// @tests REQ-NAV.name_is_asked_for
#[test]
#[ignore = "builds a Lean package and a Rust crate; run with --ignored"]
fn model_and_implementation_agree_on_what_a_key_does_to_an_answer() {
    let scratch = harness::scratch("ask");
    let op = "REQ-NAV.name_is_asked_for";
    let implementation =
        harness::rust_runner("REQ-NAV", "name_is_asked_for", "crates/core/src/surface/ask.rs::ask_step", &scratch);
    let model = harness::lean_runner("TraceLean.Ask", "TraceLean.Ask.askStep", op, &["answer", "key"], &scratch);
    let result = run(op, &ask_input(), &model, &implementation, RunOptions { seed: 103, cases: 3_000 })
        .expect("both runners answer");
    support::agreed(&result);
    let _ = std::fs::remove_dir_all(&scratch);
}

/// Each way a key can go: text, a space, Backspace, Enter with and without an
/// answer, Escape, and a key that is none of them.
///
/// @tests REQ-NAV.name_is_asked_for
/// @tests REQ-DRT-COVER.law_coverage
#[test]
fn generation_reaches_every_way_a_key_goes() {
    use tracelean_core::drt::gen;
    use tracelean_core::surface::ask::ask_step;

    let mut rng = gen::Rng::new(103);
    let (mut typed, mut spaced, mut back, mut done, mut empty, mut escaped, mut other) =
        (0u64, 0u64, 0u64, 0u64, 0u64, 0u64, 0u64);
    for _ in 0..3_000 {
        let v = gen::value(&ask_input(), &mut rng);
        let (answer, key) = (v["answer"].as_str().unwrap().to_string(), v["key"].as_str().unwrap().to_string());
        let (outcome, now) = ask_step(answer.clone(), key.clone());
        match (outcome.as_str(), key.as_str()) {
            ("cancelled", "Escape") => escaped += 1,
            ("cancelled", _) => empty += 1,
            ("done", _) => done += 1,
            (_, "Space") => spaced += 1,
            (_, "Backspace") => back += 1,
            (_, k) if k.chars().count() == 1 => typed += 1,
            _ => {
                assert_eq!(now, answer);
                other += 1
            }
        }
    }
    support::covered(
        "REQ-NAV.name_is_asked_for",
        &[
            ("a character typed", typed),
            ("a space typed", spaced),
            ("backspace", back),
            ("enter with an answer", done),
            ("enter with none", empty),
            ("escape", escaped),
            ("a key that is not text", other),
        ],
    );
}

fn setting_input() -> Schema {
    strukt(&[(
        "value",
        text(
            0,
            &[
                "#fff", "#ffff", "#1e90ff", "#1e90ff80", "#ABCdef", "", "#", "fff", "#ff", "#fffff", "red", "#1e90fg",
                "#éee", "##fff", "#1e90ff8",
            ],
        ),
    )])
}

/// What a colour setting accepts, checked against the model.
///
/// @drt REQ-LOOK.setting_edited_in_place
/// @tests REQ-LOOK.setting_edited_in_place
#[test]
#[ignore = "builds a Lean package and a Rust crate; run with --ignored"]
fn model_and_implementation_agree_on_what_a_colour_is() {
    let scratch = harness::scratch("setting");
    let op = "REQ-LOOK.setting_edited_in_place";
    let implementation =
        harness::rust_runner("REQ-LOOK", "setting_edited_in_place", "crates/core/src/surface/setting.rs::is_colour", &scratch);
    let model = harness::lean_runner("TraceLean.Setting", "TraceLean.Setting.isColour", op, &["value"], &scratch);
    let result = run(op, &setting_input(), &model, &implementation, RunOptions { seed: 107, cases: 2_000 })
        .expect("both runners answer");
    support::agreed(&result);
    let _ = std::fs::remove_dir_all(&scratch);
}

/// A colour and a value that is none.
///
/// @tests REQ-LOOK.setting_edited_in_place
/// @tests REQ-DRT-COVER.law_coverage
#[test]
fn generation_reaches_colours_and_values_that_are_none() {
    use tracelean_core::drt::gen;
    use tracelean_core::surface::setting::is_colour;

    let mut rng = gen::Rng::new(107);
    let (mut colour, mut not) = (0u64, 0u64);
    for _ in 0..2_000 {
        let v = gen::value(&setting_input(), &mut rng);
        if is_colour(v["value"].as_str().unwrap().to_string()) {
            colour += 1;
        } else {
            not += 1;
        }
    }
    support::covered("REQ-LOOK.setting_edited_in_place", &[("a colour", colour), ("not a colour", not)]);
}

fn goals_input() -> Schema {
    strukt(&[
        ("answered", Schema::Bool),
        (
            "goals",
            Schema::Option {
                inner: Box::new(Schema::List { inner: Box::new(text(0, &["x : Nat\n⊢ x = x", "⊢ True", "a"])), max_len: Some(3) }),
            },
        ),
        ("rendered", Schema::Option { inner: Box::new(text(0, &["rendered goal", ""])) }),
    ])
}

/// What the proof panel says of the goals, checked against the model.
///
/// @drt REQ-LSP.proof_state_shown
/// @tests REQ-LSP.proof_state_shown
#[test]
#[ignore = "builds a Lean package and a Rust crate; run with --ignored"]
fn model_and_implementation_agree_on_what_the_panel_says() {
    let scratch = harness::scratch("goals");
    let op = "REQ-LSP.proof_state_shown";
    let implementation =
        harness::rust_runner("REQ-LSP", "proof_state_shown", "crates/core/src/lean_server.rs::goals_shown", &scratch);
    let model = harness::lean_runner(
        "TraceLean.ProofState",
        "TraceLean.ProofState.goalsShown",
        op,
        &["answered", "goals", "rendered"],
        &scratch,
    );
    let result = run(op, &goals_input(), &model, &implementation, RunOptions { seed: 109, cases: 2_000 })
        .expect("both runners answer");
    support::agreed(&result);
    let _ = std::fs::remove_dir_all(&scratch);
}

/// No result, no list of goals, an empty list, several goals.
///
/// @tests REQ-LSP.proof_state_shown
/// @tests REQ-DRT-COVER.law_coverage
#[test]
fn generation_reaches_each_thing_a_server_can_answer() {
    use tracelean_core::drt::gen;
    use tracelean_core::lean_server::goals_shown;

    let mut rng = gen::Rng::new(109);
    let (mut no_result, mut no_list, mut empty, mut several, mut rendered) = (0u64, 0u64, 0u64, 0u64, 0u64);
    for _ in 0..2_000 {
        let v = gen::value(&goals_input(), &mut rng);
        let answered = v["answered"].as_bool().unwrap();
        let goals: Option<Vec<String>> = serde_json::from_value(v["goals"].clone()).unwrap();
        let shown: Option<String> = serde_json::from_value(v["rendered"].clone()).unwrap();
        let said = goals_shown(answered, goals.clone(), shown.clone());
        match (answered, &goals) {
            (false, _) => no_result += 1,
            (true, None) => {
                no_list += 1;
                if shown.is_some() {
                    rendered += 1;
                }
            }
            (true, Some(g)) if g.is_empty() => {
                assert_eq!(said, "no goals");
                empty += 1
            }
            (true, Some(g)) if g.len() > 1 => several += 1,
            _ => {}
        }
    }
    support::covered(
        "REQ-LSP.proof_state_shown",
        &[
            ("no result", no_result),
            ("a result with no list of goals", no_list),
            ("an empty list of goals", empty),
            ("several goals", several),
            ("the server's own rendering", rendered),
        ],
    );
}
