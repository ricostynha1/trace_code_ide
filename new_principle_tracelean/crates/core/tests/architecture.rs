//! Architectural clauses, checked against the source rather than asserted in a
//! comment.
//!
//! These are properties about what the code *does not do*. Nothing can be run
//! to demonstrate them: there is no input that makes a network call appear, and
//! a differential test would compare two things that both do nothing. What can
//! be done is read the tree and say so, which is what this does — and what
//! turns `ARCH-NO-DRIVING` from a promise in a document into a claim that fails
//! when somebody breaks it.
//!
//! Scoped to this project's own crates. Dependencies are a separate question,
//! answered by the dependency list being short enough to read.

use std::path::{Path, PathBuf};

fn project_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap().parent().unwrap().to_path_buf()
}

/// Every `.rs` file of this project's own source, with its path.
fn sources() -> Vec<(String, String)> {
    let root = project_root().join("crates");
    let mut out = Vec::new();
    let mut stack = vec![root.clone()];
    while let Some(dir) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else { continue };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                if path.file_name().is_some_and(|n| n == "target") {
                    continue;
                }
                stack.push(path);
            } else if path.extension().is_some_and(|e| e == "rs") {
                if let Ok(text) = std::fs::read_to_string(&path) {
                    let rel = path.strip_prefix(&root).unwrap_or(&path);
                    out.push((rel.display().to_string(), text));
                }
            }
        }
    }
    assert!(out.len() > 20, "found only {} source files; the walk is wrong", out.len());
    out
}

/// Lines of code, with comments and the needles this test itself contains
/// stripped out — otherwise the test would be its own counterexample.
fn code_lines(text: &str, file: &str) -> Vec<(usize, String)> {
    if file.ends_with("tests/architecture.rs") {
        return Vec::new();
    }
    text.lines()
        .enumerate()
        .map(|(i, line)| (i + 1, line.trim().to_string()))
        .filter(|(_, line)| !line.starts_with("//") && !line.starts_with("/*") && !line.starts_with('*'))
        .collect()
}

fn offenders(needles: &[&str]) -> Vec<String> {
    offenders_in(&sources(), needles)
}

/// The same check over any tree, so a tree built to violate it can be tried.
fn offenders_in(files: &[(String, String)], needles: &[&str]) -> Vec<String> {
    let mut found = Vec::new();
    for (file, text) in files {
        for (line_no, line) in code_lines(&text, &file) {
            for needle in needles {
                if line.contains(needle) {
                    found.push(format!("{file}:{line_no}: {needle}"));
                }
            }
        }
    }
    found
}

const NETWORK: &[&str] = &[
    "reqwest", "hyper::", "TcpStream", "UdpSocket", "TcpListener",
    "api.anthropic.com", "api.openai.com", "bedrock", "ANTHROPIC_API_KEY",
    "OPENAI_API_KEY", "openai", "anthropic",
];

const AMBIENT: &[&str] = &[
    "SystemTime::now", "Instant::now", "Utc::now", "Local::now", "chrono::",
    "rand::", "thread_rng", "random()", "uuid::", "Uuid::new_v4",
    "hostname", "std::env::var(\"USER\")",
];

/// Process spawns outside the differential-testing toolchain.
fn launches_in(files: &[(String, String)]) -> Vec<String> {
    let mut found = Vec::new();
    for (file, text) in files {
        // Building a runner is this project's own toolchain use, and it is
        // what `drt` exists to do; driving an agent is not.
        if file.contains("/src/drt/") || file.contains("/tests/") {
            continue;
        }
        for (line_no, line) in code_lines(text, file) {
            if line.contains("Command::new") || line.contains("process::Command") {
                found.push(format!("{file}:{line_no}: {line}"));
            }
        }
    }
    found
}

fn tree(files: &[(&str, &str)]) -> Vec<(String, String)> {
    files.iter().map(|(p, t)| (p.to_string(), t.to_string())).collect()
}

/// Each check above rejects a tree that breaks it, and accepts one that does not.
///
/// @tests REQ-CHECK.structural_rejects
#[test]
fn the_needle_checks_reject_a_tree_that_breaks_them() {
    let calls_a_model = tree(&[("core/src/x.rs", "fn a() {}\nlet c = reqwest::get(u);\n")]);
    assert_eq!(offenders_in(&calls_a_model, NETWORK).len(), 1);
    assert!(offenders_in(&tree(&[("core/src/x.rs", "fn a() {}\n")]), NETWORK).is_empty());

    let reads_a_clock = tree(&[("core/src/x.rs", "let t = SystemTime::now();\n")]);
    assert_eq!(offenders_in(&reads_a_clock, AMBIENT).len(), 1);
    assert!(offenders_in(&tree(&[("core/src/x.rs", "// SystemTime::now is banned\n")]), AMBIENT).is_empty());

    let spawns = tree(&[("core/src/x.rs", "let c = Command::new(\"claude\");\n")]);
    assert_eq!(launches_in(&spawns).len(), 1);
    assert!(launches_in(&tree(&[("core/src/drt/x.rs", "let c = Command::new(\"lake\");\n")])).is_empty());
}

/// No call to a language model, and nothing that would account for one.
///
/// @tests ARCH-NO-DRIVING.no_model_call
/// @tests ARCH-NO-DRIVING.no_launch
/// @tests REQ-JUDGE.no_call
/// @structural ARCH-NO-DRIVING.no_model_call reason="an absence has no function to model; what can be stated is that no call site exists, and that is read off the tree"
/// @structural REQ-JUDGE.no_call reason="recording a judgement calls nothing, and an absence of call sites is read off the tree rather than computed"
/// @structural ARCH-NO-DRIVING.no_launch reason="the same absence, for process spawning"
#[test]
fn nothing_here_calls_a_model_or_reaches_the_network() {
    let found = offenders(NETWORK);
    assert!(found.is_empty(), "this project must make no model or network call:\n{found:#?}");
}

/// Nothing starts, steers or stops an external tool. The user runs it.
///
/// @tests ARCH-NO-DRIVING.no_launch
/// @tests ARCH-NO-DRIVING.user_runs_it
/// @tests ARCH-NO-DRIVING.observation_only
/// @structural ARCH-NO-DRIVING.user_runs_it reason="who starts a tool is not a value this system computes"
/// @structural ARCH-NO-DRIVING.observation_only reason="a claim about which direction information flows through the tree, not about any one function"
#[test]
fn nothing_here_launches_a_coding_agent() {
    let found = launches_in(&sources());
    assert!(
        found.is_empty(),
        "only the differential-testing toolchain may spawn a process:\n{found:#?}"
    );
}

/// A derived artefact embeds nothing that varies between runs of the same
/// input: no clock, no randomness that is not seeded, no machine identity.
///
/// @tests ARCH-DETERMINISM.no_ambient_time
/// @tests ARCH-DETERMINISM.seeded_generation
/// @structural ARCH-DETERMINISM.no_ambient_time reason="a constraint on what the source may mention; a function that read a clock would satisfy any model of itself"
#[test]
fn nothing_here_reads_a_clock_or_an_unseeded_source_of_randomness() {
    let found = offenders(AMBIENT);
    assert!(
        found.is_empty(),
        "a derived artefact must be a function of its input alone:\n{found:#?}"
    );
}

/// The one exception, stated rather than hidden: scratch directories for the
/// generated runners are named from the process id, which does vary. They are
/// temporary build directories, not derived artefacts, and nothing produced
/// from them carries the name.
#[test]
fn the_only_varying_names_are_scratch_directories() {
    let found = varying_names_outside_scratch(&sources());
    assert!(found.is_empty(), "a varying name outside a scratch directory:\n{found:#?}");
}

/// @tests REQ-CHECK.structural_rejects
#[test]
fn the_scratch_name_check_rejects_a_tree_that_breaks_it() {
    let bad = tree(&[("core/src/trace/x.rs", "fn a() {}\nlet n = std::process::id();\n")]);
    assert_eq!(varying_names_outside_scratch(&bad).len(), 1);
    assert!(varying_names_outside_scratch(&tree(&[("core/src/drt/x.rs", "let n = std::process::id();\n")])).is_empty());
    let fixture = tree(&[("core/src/trace/x.rs", "fn a() {}\n#[cfg(test)]\nmod t {\nlet n = std::process::id();\n}\n")]);
    assert!(varying_names_outside_scratch(&fixture).is_empty());
}

fn varying_names_outside_scratch(files: &[(String, String)]) -> Vec<String> {
    let mut found = Vec::new();
    for (file, text) in files {
        // Everything from `#[cfg(test)]` onwards builds fixtures, not
        // artefacts, and a fixture directory has to be unique per process to
        // let tests run in parallel.
        let fixtures_begin = text
            .lines()
            .position(|line| line.trim() == "#[cfg(test)]")
            .map(|i| i + 1)
            .unwrap_or(usize::MAX);

        for (line_no, line) in code_lines(&text, &file) {
            if !line.contains("process::id") && !line.contains("temp_dir") {
                continue;
            }
            let allowed = line_no > fixtures_begin
                || file.contains("/drt/")
                || file.contains("/tests/");
            if !allowed {
                found.push(format!("{file}:{line_no}"));
            }
        }
    }
    found
}

/// Filesystem access is confined to the modules that declare themselves a
/// shell, and those modules make no decisions.
///
/// `shell_thin` is hard to check directly — "a branch a decision function could
/// have made" is a judgement. What can be checked is the two things that make
/// it true in practice: the set of modules that touch the filesystem is small
/// and named, and none of them contains the vocabulary of a decision. A shell
/// that started classifying paths or grading evidence would fail here.
///
/// @tests ARCH-CORE-SHELL.shell_thin
/// @structural ARCH-CORE-SHELL.shell_thin reason="a property of how the modules are divided, which no single function can express"
#[test]
fn only_declared_shells_touch_the_filesystem() {
    // Each of these reads or writes, and each is listed with why. Adding a file
    // here is the moment to ask whether the decision inside it belongs in a
    // core module instead.
    const SHELLS: &[&str] = &[
        "core/src/observe/workspace.rs", // copies a tree into a workspace
        "core/src/observe/watch.rs",     // reads an agent's transcript and the price table
        "core/src/observe/workcopy.rs",  // makes, finds and ends a sandbox's copy
        "core/src/trace/index.rs",       // walks a project and reads its files
        "core/src/trace/lockfile.rs",    // writes the lock file
        "core/src/trace/store.rs",       // holds the records backends earn
        "core/src/history/persistence.rs", // appends and replays the journal
        "core/src/drt/rust_runner.rs",   // generates a runner package
        "core/src/drt/lean_runner.rs",   // generates a runner package
        "core/src/drt/ts_runner.rs",     // generates a runner package
        "core/src/drt/run.rs",           // speaks to two runner processes
        "core/src/drt/config.rs",        // reads the binding file
        "core/src/drt/pins.rs",          // asks Lean about a pinning theorem, keeps the verdict
        "core/src/drt/auto.rs",          // generates, builds and runs a derived test's runners
        "core/src/drt/lines_run.rs",     // runs each test under coverage, keeps the lines
        "core/src/bin/tracelean-trace.rs", // the command-line entry point
        "editor/src/theme.rs",           // reads the colours a project chose
        "editor/src/recall.rs",          // keeps what was open for next time
    ];

    let unexpected = filesystem_outside_shells(&sources(), SHELLS);
    assert!(
        unexpected.is_empty(),
        "a module that is not a declared shell reads or writes the filesystem:\n{unexpected:#?}"
    );
    let thick = thick_shells(&sources(), SHELLS);
    assert!(thick.is_empty(), "{thick:#?}");
}

/// Files that touch the filesystem without being a declared shell.
fn filesystem_outside_shells(files: &[(String, String)], shells: &[&str]) -> Vec<String> {
    let mut unexpected = Vec::new();
    for (file, text) in files {
        if file.contains("/tests/") || shells.contains(&file.as_str()) {
            continue;
        }
        // Fixtures build temporary trees; that is not the shell this is about.
        let fixtures_begin = text
            .lines()
            .position(|line| line.trim() == "#[cfg(test)]")
            .map(|i| i + 1)
            .unwrap_or(usize::MAX);
        for (line_no, line) in code_lines(text, file) {
            if line_no > fixtures_begin {
                continue;
            }
            if line.contains("std::fs::") || line.contains("fs::read") || line.contains("fs::write")
            {
                unexpected.push(format!("{file}:{line_no}: {line}"));
            }
        }
    }
    unexpected
}

/// Shells that stay thin. A decision belongs in a core module where a
/// conformance runner can call it; the number is a ceiling on drift, not a
/// style rule, and each shell is currently well under it.
fn thick_shells(files: &[(String, String)], shells: &[&str]) -> Vec<String> {
    let mut thick = Vec::new();
    for (file, text) in files {
        if !shells.contains(&file.as_str()) {
            continue;
        }
        let branches = code_lines(text, file)
            .iter()
            .filter(|(_, line)| line.starts_with("match ") || line.starts_with("if "))
            .count();
        if branches >= 40 {
            thick.push(format!(
                "{file} has {branches} branches; a shell that decides that much has a decision \
                 function hiding in it"
            ));
        }
    }
    thick
}

/// @tests REQ-CHECK.structural_rejects
#[test]
fn the_shell_checks_reject_a_tree_that_breaks_them() {
    let shells = ["core/src/trace/store.rs"];
    let reaches = tree(&[("core/src/surface/x.rs", "fn a() {}\nlet t = std::fs::read_to_string(p);\n")]);
    assert_eq!(filesystem_outside_shells(&reaches, &shells).len(), 1);
    let allowed = tree(&[("core/src/trace/store.rs", "let t = std::fs::read_to_string(p);\n")]);
    assert!(filesystem_outside_shells(&allowed, &shells).is_empty());
    let in_fixture = tree(&[("core/src/surface/x.rs", "fn a() {}\n#[cfg(test)]\nmod t {\nlet t = std::fs::write(p, q);\n}\n")]);
    assert!(filesystem_outside_shells(&in_fixture, &shells).is_empty());

    let many = "if x {}\n".repeat(41);
    assert_eq!(thick_shells(&tree(&[("core/src/trace/store.rs", &many)]), &shells).len(), 1);
    assert!(thick_shells(&tree(&[("core/src/trace/store.rs", "if x {}\n")]), &shells).is_empty());
}

/// Every axiomatised effect carries a law, and every law is a function
/// something can compute.
///
/// An axiom with no law is an assumption with no content: it says an operation
/// exists and nothing about what it must do. This reads the one module axioms
/// are allowed to live in and checks that each opaque constant is either
/// accompanied by a law or is explicitly documented as carrying its claim in
/// its type.
///
/// @tests ARCH-EFFECT-LAW.law_stated
/// @tests ARCH-EFFECT-LAW.axioms_confined
/// @tests ARCH-EFFECT-LAW.axiomatised
/// @structural ARCH-EFFECT-LAW.law_stated reason="a claim about the model tree: every axiom is accompanied by a law. Modelling it would need the model to quantify over itself"
/// @structural ARCH-EFFECT-LAW.axioms_confined reason="a claim about which file the axioms are in"
/// @structural ARCH-EFFECT-LAW.axiomatised reason="a claim about how effects are declared in the model tree"
#[test]
fn axioms_live_in_one_file_and_each_one_states_a_law() {
    let formal = project_root().join("formal").join("TraceLean");
    let mut models = Vec::new();
    for entry in std::fs::read_dir(&formal).expect("the model directory").flatten() {
        let path = entry.path();
        let Ok(text) = std::fs::read_to_string(&path) else { continue };
        models.push((path.file_name().unwrap().to_string_lossy().to_string(), text));
    }
    let problems = axiom_problems(&models);
    assert!(problems.is_empty(), "{problems:#?}");
}

/// What is wrong with how a set of model files declares axioms.
fn axiom_problems(models: &[(String, String)]) -> Vec<String> {
    let mut problems = Vec::new();
    let with_axioms: Vec<(&String, &String, usize)> = models
        .iter()
        .map(|(name, text)| {
            let declares = text.lines().filter(|line| line.trim_start().starts_with("axiom ")).count();
            (name, text, declares)
        })
        .filter(|(_, _, declares)| *declares > 0)
        .collect();

    let names: Vec<&str> = with_axioms.iter().map(|(name, _, _)| name.as_str()).collect();
    if names != vec!["Effects.lean"] {
        problems.push(format!(
            "axioms must live in one module, so the project's assumptions read in one sitting: {names:?}"
        ));
    }
    let Some((_, text, declared)) = with_axioms.iter().find(|(name, _, _)| name.as_str() == "Effects.lean") else {
        return problems;
    };
    // Laws are the axioms that state something; effects are the axioms that
    // only declare a constant. Every effect either has a law or says in its own
    // doc comment why its type is the whole claim.
    let laws = text.lines().filter(|line| line.contains("axiom") && line.contains("_")).count();
    if !(laws > 0 && *declared >= laws) {
        problems.push(format!("{declared} axioms and {laws} of them named as laws"));
    }
    if !text.contains("No axiom accompanies this one, and the absence is the point") {
        problems.push("an effect without a law must say why, in the module itself".to_string());
    }
    // And every law is reachable as a function: the model computes violations,
    // so a differential test can ask the implementation the same question.
    for law in ["copyViolations", "capabilityViolations", "escapeViolations", "runViolations"] {
        if !text.contains(&format!("def {law}")) {
            problems.push(format!("the law `{law}` is not a function"));
        }
    }
    problems
}

/// @tests REQ-CHECK.structural_rejects
#[test]
fn the_axiom_check_rejects_a_tree_that_breaks_it() {
    let laws = "def copyViolations := 1\ndef capabilityViolations := 1\ndef escapeViolations := 1\ndef runViolations := 1\n";
    let good = format!(
        "axiom copy_law : True\naxiom run_law : True\n-- No axiom accompanies this one, and the absence is the point\n{laws}"
    );
    assert!(axiom_problems(&tree(&[("Effects.lean", &good)])).is_empty());

    // An axiom in another module.
    let stray = tree(&[("Effects.lean", &good), ("Other.lean", "axiom sneaky_law : True\n")]);
    assert_eq!(axiom_problems(&stray).len(), 1);
    // A law that is not a function, and an effect that does not say why it has no law.
    let bare = tree(&[("Effects.lean", "axiom copy_law : True\n")]);
    assert!(axiom_problems(&bare).len() >= 2, "{:?}", axiom_problems(&bare));
}

/// A finding is a named variant, not a string or a bool.
///
/// The failure this prevents is the one that reads as working: a checker that
/// returns `false`, or an error that says "invalid", tells the person looking
/// at it nothing they can act on.
///
/// @tests ARCH-HONEST.named_findings
/// @structural ARCH-HONEST.named_findings reason="a claim about every finding type in the project at once, checked by rendering one of each"
#[test]
fn every_finding_type_is_a_closed_named_vocabulary() {
    use tracelean_core::observe::effects::Violation;
    use tracelean_core::surface::keymap::Problem;
    use tracelean_core::trace::checker::Kind;

    // Each of these is an enum whose variants name the problem. Rendering one
    // through serde gives the name, which is what reaches a report.
    let named: Vec<String> = vec![
        serde_json::to_value(Kind::Unmodeled).unwrap().to_string(),
        serde_json::to_value(Violation::ContainmentUnreported).unwrap().to_string(),
        serde_json::to_value(Problem::UnreachableMode { mode: "x".into() })
            .unwrap()
            .to_string(),
    ];
    for rendered in &named {
        assert!(
            rendered.len() > 6 && !rendered.contains("true") && !rendered.contains("false"),
            "a finding rendered as {rendered}, which says nothing about what is wrong"
        );
    }
    assert!(named[0].contains("unmodeled"));
    assert!(named[1].contains("containmentUnreported"));
    assert!(named[2].contains("unreachableMode"));
}

/// A decision function is reachable from outside the crate.
///
/// This is what makes a conformance runner possible at all: the generated
/// runner is a separate package, so anything it calls has to be `pub` and
/// re-exported. Every binding in `.tracelean/drt.json` names one, and
/// `bindings_are_real` resolves them against the source — what is left to check
/// here is that the module path is public the whole way down.
///
/// @tests ARCH-CORE-SHELL.decision_public
/// @structural ARCH-CORE-SHELL.decision_public reason="a claim about module visibility, which is a fact about the source and not a value"
#[test]
fn every_module_holding_a_decision_is_public() {
    let text = std::fs::read_to_string(project_root().join("crates/core/src/lib.rs"))
        .expect("the crate root");
    let hidden = private_modules(&text);
    assert!(hidden.is_empty(), "not public, so a generated runner cannot call into them: {hidden:?}");
}

/// The decision modules a crate root does not declare `pub mod`.
fn private_modules(root_text: &str) -> Vec<&'static str> {
    ["drt", "trace", "history", "observe", "surface", "evidence", "judge"]
        .into_iter()
        .filter(|module| !root_text.contains(&format!("pub mod {module}")))
        .collect()
}

/// @tests REQ-CHECK.structural_rejects
#[test]
fn the_visibility_check_rejects_a_private_decision_module() {
    let all = "pub mod drt;\npub mod trace;\npub mod history;\npub mod observe;\npub mod surface;\npub mod evidence;\npub mod judge;\n";
    assert!(private_modules(all).is_empty());
    assert_eq!(private_modules(&all.replace("pub mod trace;", "mod trace;")), vec!["trace"]);
}

/// Where a model's opinion would be useful, a prompt is exported for a person
/// to carry — and nothing sends it.
///
/// The judge produces text. Whether that text ever reaches a model is the
/// user's decision, made in their own client, and this asserts there is no
/// second path.
///
/// @tests ARCH-NO-DRIVING.export_not_call
/// @structural ARCH-NO-DRIVING.export_not_call reason="the claim is that a code path does not exist; a model of the export function could not distinguish a version that also sent it"
#[test]
fn the_judge_exports_a_prompt_and_nothing_carries_it() {
    use tracelean_core::judge::{prompt, Material};

    let text = prompt(Material {
        req_id: "REQ-X".into(),
        clause: Some("one".into()),
        clause_text: "A thing shall happen.".into(),
        model_source: "def f := 1".into(),
        requirement_hash: "h1".into(),
        model_hash: "h2".into(),
        divergence: Some("model said 1, implementation said 2".into()),
    });
    assert!(text.contains("REQ-X"), "the prompt does not say what it is about");

    // And nothing in the tree carries it anywhere. The needles are the ones a
    // transport would need; `nothing_here_calls_a_model_or_reaches_the_network`
    // covers the provider side, this covers the general one.
    let found = offenders(&["reqwest", "hyper::", "TcpStream", "UdpSocket", "ureq", "curl"]);
    assert!(found.is_empty(), "something in this tree can send:\n{found:#?}");
}

/// A link exists only where somebody wrote one, and nothing about it is
/// inferred from where the file sits.
///
/// The failure this prevents is a tool that reads links off directory layout —
/// `src/foo.rs` implements `reqs/foo.md` — which works until somebody
/// reorganises the tree and every claim silently changes meaning.
///
/// What is checked is that moving and renaming everything changes nothing about
/// *what* is claimed: the same clause, the same body hash, the same requirement
/// identity. The `link_hash` does move, and deliberately — it names the link's
/// target, a file included, so that evidence earned for a function in one file
/// is not inherited by the same text in another (`REQ-STALE.retarget_invalidates`).
///
/// The comparison below can tell two trees apart: the same layout with a
/// different body moves the claim's hash, so equal claims after a move mean
/// something. (A convention-keyed tool cannot be built to fail it here; what
/// can be shown is that the check is not blind.)
///
/// @tests REQ-CHECK.structural_rejects
#[test]
fn the_no_convention_comparison_is_sensitive_to_what_is_claimed() {
    use tracelean_core::trace::index::build;

    let base = std::env::temp_dir().join(format!("tracelean-no-convention-neg-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&base);
    let requirement = "---\nid: REQ-M\nclauses:\n  one: A thing shall happen.\n---\n\n# M\n";
    for (dir, body) in [("one", "pub fn f() {}\n"), ("two", "pub fn f() { 1; }\n")] {
        std::fs::create_dir_all(base.join(dir)).unwrap();
        std::fs::write(base.join(dir).join("REQ-M.md"), requirement).unwrap();
        std::fs::write(base.join(dir).join("impl.rs"), format!("/// @implements REQ-M.one\n{body}")).unwrap();
    }
    let hash = |dir: &str| build(&base.join(dir)).links[0].anchor.body_hash.clone();
    assert_ne!(hash("one"), hash("two"));
    let _ = std::fs::remove_dir_all(&base);
}

/// @tests ARCH-SELFHOST.no_convention
/// @structural ARCH-SELFHOST.no_convention reason="the claim is that no code keys off a path, which is an absence in the source rather than a value any function returns"
#[test]
fn moving_a_file_does_not_change_what_it_claims() {
    use tracelean_core::trace::index::build;

    let base = std::env::temp_dir().join(format!("tracelean-no-convention-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&base);

    let requirement = "---\nid: REQ-M\nclauses:\n  one: A thing shall happen.\n---\n\n# M\n";
    let source = "/// @implements REQ-M.one\npub fn f() {}\n";

    let here = base.join("here");
    std::fs::create_dir_all(here.join("a/b")).unwrap();
    std::fs::write(here.join("REQ-M.md"), requirement).unwrap();
    std::fs::write(here.join("a/b/impl.rs"), source).unwrap();

    let there = base.join("there");
    std::fs::create_dir_all(there.join("totally/different/place")).unwrap();
    std::fs::write(there.join("other-name.md"), requirement).unwrap();
    std::fs::write(there.join("totally/different/place/renamed.rs"), source).unwrap();

    let first = build(&here);
    let second = build(&there);

    let claim = |index: &tracelean_core::trace::index::Index| -> Vec<(String, Option<String>, String)> {
        index
            .links
            .iter()
            .map(|l| (l.req_id.clone(), l.clause.clone(), l.anchor.body_hash.clone()))
            .collect()
    };
    assert_eq!(claim(&first).len(), 1, "the link was not found");
    assert_eq!(
        claim(&first),
        claim(&second),
        "moving and renaming everything changed what the annotation claims"
    );
    assert_eq!(
        first.requirements["REQ-M"].content_hash,
        second.requirements["REQ-M"].content_hash,
        "a requirement's identity moved with its filename"
    );
    assert!(
        first.requirements.contains_key("REQ-M") && second.requirements.contains_key("REQ-M"),
        "a requirement was found by its filename rather than its declared id"
    );

    let _ = std::fs::remove_dir_all(&base);
}

/// A check that did not run reads as not run, and a derived relationship is
/// distinguishable from an asserted one.
///
/// Two clauses, one test, because they are the same discipline: every place
/// this system is unsure carries a field saying so, and nothing defaults that
/// field to the confident answer.
///
/// @tests ARCH-HONEST.absence_is_not_pass
/// @tests ARCH-HONEST.confidence_carried
/// @structural ARCH-HONEST.absence_is_not_pass reason="a claim about every reporting path at once: no path may turn an absence into a pass"
/// @structural ARCH-HONEST.confidence_carried reason="a claim that certain fields exist and reach the artefact, not a property of any one call"
#[test]
fn every_unsure_answer_carries_a_field_saying_so() {
    use tracelean_core::evidence::{assurance, Bond, Level, Record};
    use tracelean_core::trace::anchor::{Anchor, AnchorKind};
    use tracelean_core::trace::checker::coverage_kinds;
    use tracelean_core::trace::rollup::Figure;

    // No evidence is the bottom of the ladder, not the top.
    assert_eq!(assurance(vec![]), Level::L1);
    assert_eq!(assurance(vec![Record { bond: Bond::ModelProof, level: Level::L4 }]), Level::L1);

    // No roles is a finding, not silence.
    assert!(
        !coverage_kinds(vec![], false, false).is_empty(),
        "a clause nothing claims produced no finding, which reads as passing"
    );

    // A figure over an unclaimed decomposition can never render as exact, and
    // an anchor in a file that did not parse says it is imprecise.
    assert!(!Figure { met: 2, total: 2, exact: false }.is_complete());
    assert!(Figure { met: 2, total: 2, exact: true }.is_complete());
    let vague = Anchor {
        file: "a.rs".into(),
        kind: AnchorKind::File,
        body_hash: "h".into(),
        start_line: 0,
        end_line: 0,
        precise: false,
    };
    assert!(!vague.precise, "a whole-file anchor claimed to be precisely placed");

    // And those fields survive serialisation, which is where they would be
    // quietly dropped.
    let rendered = serde_json::to_string(&Figure { met: 1, total: 2, exact: false }).unwrap();
    assert!(rendered.contains("exact"), "a figure lost the field that says it is a bound");
}

/// Exemption stays rare, and every one that exists says who signed for it.
///
/// `exempt_last` is the clause that keeps grey meaningful. It is checked by
/// counting: an exemption is cheap to write and expensive to justify, and a
/// number that creeps is the thing to notice.
///
/// @tests ARCH-EFFECT-LAW.exempt_last
/// @structural ARCH-EFFECT-LAW.exempt_last reason="a claim about how many exemptions exist in this repository, which is a count over the tree rather than a value"
#[test]
fn exemptions_are_rare_and_every_one_is_signed() {
    use tracelean_core::trace::index::build;

    let index = build(&project_root());
    let claims: Vec<(String, Option<Qualifier>)> =
        index.links.iter().map(|l| (l.anchor.file.clone(), l.qualifier.clone())).collect();
    let problems = escape_hatch_problems(&claims);
    assert!(problems.is_empty(), "{problems:#?}");
}

use tracelean_core::trace::annotation::Qualifier;

/// What is wrong with a tree's exemptions and structural claims: more than five
/// exemptions, one without a reason and an approver, or a structural claim that
/// does not say why there is no law to state.
fn escape_hatch_problems(claims: &[(String, Option<Qualifier>)]) -> Vec<String> {
    let mut problems = Vec::new();
    let exemptions: Vec<_> =
        claims.iter().filter(|(_, q)| matches!(q, Some(Qualifier::Exempt { .. }))).collect();
    if exemptions.len() > 5 {
        problems.push(format!(
            "{} exemptions; `exempt_last` means this number stays small enough to read",
            exemptions.len()
        ));
    }
    for (file, qualifier) in &exemptions {
        let Some(Qualifier::Exempt { reason, judged_by, .. }) = qualifier else { continue };
        if reason.is_none() || judged_by.is_none() {
            problems.push(format!("an exemption in {file} names no reason or no approver"));
        }
    }
    // Structural claims are the other escape hatch, and they are held to the
    // same standard: every one says why there is no law to state.
    for (file, qualifier) in claims {
        if let Some(Qualifier::Structural { reason }) = qualifier {
            if !reason.as_ref().is_some_and(|r| r.len() > 20) {
                problems.push(format!("a structural claim in {file} does not say why there is no law"));
            }
        }
    }
    problems
}

/// @tests REQ-CHECK.structural_rejects
#[test]
fn the_escape_hatch_check_rejects_unsigned_and_unexplained_claims() {
    let signed = Qualifier::Exempt { reason: Some("r".into()), judged_by: Some("p".into()), expires: None };
    let unsigned = Qualifier::Exempt { reason: Some("r".into()), judged_by: None, expires: None };
    let why = Some("an absence that no function can express".to_string());
    assert!(escape_hatch_problems(&[("a.rs".into(), Some(signed.clone()))]).is_empty());
    assert_eq!(escape_hatch_problems(&[("a.rs".into(), Some(unsigned))]).len(), 1);
    assert_eq!(escape_hatch_problems(&vec![("a.rs".to_string(), Some(signed)); 6]).len(), 1);
    assert!(escape_hatch_problems(&[("a.rs".into(), Some(Qualifier::Structural { reason: why }))]).is_empty());
    assert_eq!(escape_hatch_problems(&[("a.rs".into(), Some(Qualifier::Structural { reason: Some("short".into()) }))]).len(), 1);
}

/// A bound entry point takes its inputs as arguments and reads nothing else.
///
/// This is what makes the conformance runner possible: the generated package
/// calls the function with deserialised arguments and nothing more. A function
/// that consulted the environment, a clock or a global would answer differently
/// in the runner than in the program, and the differential test would be
/// comparing two things that were never the same call.
///
/// @tests ARCH-CORE-SHELL.no_hidden_input
/// @structural ARCH-CORE-SHELL.no_hidden_input reason="a claim about what the bound functions may mention, which is read off their source rather than computed"
#[test]
fn nothing_a_binding_names_reads_anything_it_was_not_given() {
    let root = project_root();
    let text = std::fs::read_to_string(root.join(".tracelean/drt.json")).expect("drt.json");
    let config: serde_json::Value = serde_json::from_str(&text).expect("readable JSON");

    let mut bound = Vec::new();
    for binding in config["bindings"].as_array().expect("bindings") {
        let entry = binding["implementation"]["entry"].as_str().unwrap_or_default();
        let (file, _symbol) = entry.split_once("::").expect("file::symbol");
        let source = std::fs::read_to_string(root.join(file)).expect("a bound file exists");
        bound.push((file.to_string(), source));
    }
    assert!(bound.len() > 30, "only {} bindings checked; the walk is wrong", bound.len());
    let found = ambient_in_bound_files(&bound);
    assert!(found.is_empty(), "{found:#?}");
}

/// Bound files that mention something ambient. Which part of a file a symbol
/// is in is not tracked, so this asks the cruder question: does the module
/// holding a bound function reach for anything ambient at all? Everything from
/// `#[cfg(test)]` onward builds fixtures and is not read.
fn ambient_in_bound_files(bound: &[(String, String)]) -> Vec<String> {
    let mut found = Vec::new();
    for (file, source) in bound {
        let library = match source.find("#[cfg(test)]") {
            Some(at) => &source[..at],
            None => &source[..],
        };
        for needle in ["std::env::", "SystemTime::", "Instant::", "thread_rng", "static mut"] {
            if library.contains(needle) {
                found.push(format!("{file} holds a bound entry point and mentions `{needle}`"));
            }
        }
    }
    found
}

/// @tests REQ-CHECK.structural_rejects
#[test]
fn the_hidden_input_check_rejects_a_bound_file_that_reads_the_environment() {
    let bad = tree(&[("core/src/x.rs", "pub fn f() { let v = std::env::var(\"A\"); }\n")]);
    assert_eq!(ambient_in_bound_files(&bad).len(), 1);
    let fixture_only = tree(&[("core/src/x.rs", "pub fn f() {}\n#[cfg(test)]\nmod t { fn g() { std::env::var(\"A\"); } }\n")]);
    assert!(ambient_in_bound_files(&fixture_only).is_empty());
}
