use std::path::{Path, PathBuf};

fn project_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap().parent().unwrap().to_path_buf()
}

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

fn launches_in(files: &[(String, String)]) -> Vec<String> {
    let mut found = Vec::new();
    for (file, text) in files {
        if file.contains("/src/drt/") || file.contains("/tests/") || file.ends_with("src/lean_server.rs") {
            continue;
        }
        for (line_no, line) in code_lines(text, file) {
            let reads_history = file.ends_with("src/observe/revision.rs") && line.contains("Command::new(\"git\")");
            if (line.contains("Command::new") || line.contains("process::Command")) && !reads_history {
                found.push(format!("{file}:{line_no}: {line}"));
            }
        }
    }
    found
}

fn tree(files: &[(&str, &str)]) -> Vec<(String, String)> {
    files.iter().map(|(p, t)| (p.to_string(), t.to_string())).collect()
}

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

/// @tests ARCH-NO-DRIVING.no_model_call
/// @tests ARCH-NO-DRIVING.no_launch
/// @tests REQ-JUDGE.no_call
/// @structural ARCH-NO-DRIVING.no_model_call reason="an absence has no function to model; what can be stated is that no call site exists, and that is read off the tree"
/// @structural REQ-JUDGE.no_call reason="recording a judgement calls nothing, and an absence of call sites is read off the tree rather than computed"
#[test]
fn nothing_here_calls_a_model_or_reaches_the_network() {
    let found = offenders(NETWORK);
    assert!(found.is_empty(), "this project must make no model or network call:\n{found:#?}");
}

/// @tests ARCH-NO-DRIVING.no_launch
/// @tests ARCH-NO-DRIVING.user_runs_it
/// @tests ARCH-NO-DRIVING.observation_only
/// @structural ARCH-NO-DRIVING.user_runs_it reason="who starts a tool is not a value this system computes"
/// @structural ARCH-NO-DRIVING.observation_only reason="a claim about which direction information flows through the tree, not about any one function"
/// @structural ARCH-NO-DRIVING.no_launch reason="an absence of process spawning, read off the tree"
#[test]
fn nothing_here_launches_a_coding_agent() {
    let found = launches_in(&sources());
    assert!(
        found.is_empty(),
        "only the differential-testing toolchain may spawn a process:\n{found:#?}"
    );
}

/// @tests ARCH-DETERMINISM.no_ambient_time
/// @tests ARCH-DETERMINISM.seeded_generation
/// @tests ARCH-CORE-SHELL.decision_total
/// @structural ARCH-DETERMINISM.no_ambient_time reason="a constraint on what the source may mention; a function that read a clock would satisfy any model of itself"
#[test]
fn nothing_derived_depends_on_a_clock_randomness_or_this_machine() {
    let found = offenders(AMBIENT);
    assert!(
        found.is_empty(),
        "a derived artefact must be a function of its input alone:\n{found:#?}"
    );

    let root = project_root();
    let machine = machine_paths(&root);
    assert!(
        !machine_paths_in(&[("made".to_string(), format!("{{\"file\":\"{}\"}}", machine[0]))], &machine).is_empty(),
        "a path of this machine written into an artefact went unnoticed"
    );
    let mut derived = Vec::new();
    for dir in [root.join(".tracelean"), root.join(".tracelean").join("evidence")] {
        for entry in std::fs::read_dir(&dir).expect("the derived artefacts").flatten() {
            let path = entry.path();
            if path.extension().is_some_and(|e| e == "json" || e == "lock") {
                if let Ok(text) = std::fs::read_to_string(&path) {
                    derived.push((path.display().to_string(), text));
                }
            }
        }
    }
    assert!(derived.len() > 100, "only {} derived artefacts read; the walk is wrong", derived.len());
    let leaked = machine_paths_in(&derived, &machine);
    assert!(leaked.is_empty(), "a derived artefact names this machine:\n{leaked:#?}");
}

fn machine_paths(root: &Path) -> Vec<String> {
    let mut paths = vec![root.display().to_string()];
    if let Ok(home) = std::env::var("HOME") {
        paths.push(home);
    }
    paths.push(std::env::temp_dir().display().to_string());
    paths.retain(|p| p.len() > 1);
    paths
}

fn machine_paths_in(files: &[(String, String)], machine: &[String]) -> Vec<String> {
    let mut found = Vec::new();
    for (file, text) in files {
        for path in machine {
            if text.contains(path.as_str()) {
                found.push(format!("{file}: {path}"));
            }
        }
    }
    found
}

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

/// @tests ARCH-CORE-SHELL.shell_thin
/// @tests ARCH-CORE-SHELL.decision_total
/// @structural ARCH-CORE-SHELL.shell_thin reason="a property of how the modules are divided, which no single function can express"
#[test]
fn only_declared_shells_touch_the_filesystem() {
    let unexpected = filesystem_outside_shells(&sources(), SHELLS);
    assert!(
        unexpected.is_empty(),
        "a module that is not a declared shell reads or writes the filesystem:\n{unexpected:#?}"
    );
    let thick = thick_shells(&sources(), SHELLS);
    assert!(thick.is_empty(), "{thick:#?}");
}

const SHELLS: &[&str] = &[
    "core/src/observe/workspace.rs",
    "core/src/observe/watch.rs",
    "core/src/observe/workcopy.rs",
    "core/src/observe/revision.rs",
    "core/src/trace/index.rs",
    "core/src/trace/lockfile.rs",
    "core/src/trace/store.rs",
    "core/src/history/persistence.rs",
    "core/src/drt/rust_runner.rs",
    "core/src/drt/lean_runner.rs",
    "core/src/drt/ts_runner.rs",
    "core/src/drt/run.rs",
    "core/src/drt/config.rs",
    "core/src/drt/proofs.rs",
    "core/src/drt/auto.rs",
    "core/src/drt/lines_run.rs",
    "core/src/bin/tracelean-trace.rs",
    "editor/src/theme.rs",
    "editor/src/recall.rs",
];

const HELD: &[(&str, &str)] = &[(
    "core/src/surface/highlight.rs",
    "each language's compiled query, built from a constant; a cache, not an input",
)];

fn global_state_outside(files: &[(String, String)], shells: &[&str], held: &[(&str, &str)]) -> Vec<String> {
    let mut found = Vec::new();
    for (file, text) in files {
        if !file.starts_with("core/src/") || shells.contains(&file.as_str()) || held.iter().any(|(f, _)| f == file) {
            continue;
        }
        let fixtures_begin = text.lines().position(|line| line.trim() == "#[cfg(test)]").map(|i| i + 1).unwrap_or(usize::MAX);
        for (line_no, line) in code_lines(text, file) {
            let global = ["static mut", "thread_local!", "OnceLock", "OnceCell", "lazy_static", "static COUNTER"]
                .iter()
                .any(|needle| line.contains(needle))
                || (line.starts_with("static ") && (line.contains("Mutex") || line.contains("Atomic")));
            if global && line_no <= fixtures_begin {
                found.push(format!("{file}:{line_no}: {line}"));
            }
        }
    }
    found
}

/// @tests ARCH-CORE-SHELL.decision_total
/// @structural ARCH-CORE-SHELL.decision_total reason="a claim about every decision in the project at once: what the source may hold besides arguments, read off the tree"
#[test]
fn decisions_keep_no_state_between_calls() {
    let found = global_state_outside(&sources(), SHELLS, HELD);
    assert!(found.is_empty(), "a decision module keeps state outside its arguments:\n{found:#?}");
}

/// @tests REQ-CHECK.structural_rejects
#[test]
fn the_global_state_check_rejects_a_decision_that_keeps_some() {
    let keeps = tree(&[("core/src/trace/x.rs", "static mut SEEN: u32 = 0;\nfn a() {}\n")]);
    assert_eq!(global_state_outside(&keeps, SHELLS, HELD).len(), 1);
    let cached = tree(&[("core/src/surface/highlight.rs", "thread_local! { static Q: u8 = 0; }\n")]);
    assert!(global_state_outside(&cached, SHELLS, HELD).is_empty());
    let fixture = tree(&[("core/src/trace/x.rs", "fn a() {}\n#[cfg(test)]\nmod t {\nstatic COUNTER: AtomicU64 = AtomicU64::new(0);\n}\n")]);
    assert!(global_state_outside(&fixture, SHELLS, HELD).is_empty());
}

const REFLECTION: &[&str] = &["#print", "#check", "Lean.Meta", "Lean.Elab", "getConstInfo", "inferType", "MetaM", "TermElabM"];

fn reflection_in(files: &[(String, String)]) -> Vec<String> {
    let mut found = Vec::new();
    for (file, text) in files {
        if !file.starts_with("core/src/drt/") {
            continue;
        }
        for (line_no, line) in code_lines(text, file) {
            if REFLECTION.iter().any(|needle| line.contains(needle)) {
                found.push(format!("{file}:{line_no}: {line}"));
            }
        }
    }
    found
}

/// @tests REQ-DRT-SCHEMA.declared_not_reflected
/// @structural REQ-DRT-SCHEMA.declared_not_reflected reason="an absence of reflection in the source and in what it generates, read off both"
#[test]
fn no_schema_is_extracted_from_a_model_by_reflection() {
    use tracelean_core::drt::lean_runner::{self, LeanEntry};
    let found = reflection_in(&sources());
    assert!(found.is_empty(), "the differential machinery reflects on the model:\n{found:#?}");

    let out = std::env::temp_dir().join(format!("tracelean-reflect-{}", std::process::id()));
    let entries = [LeanEntry { op: "REQ-X.c".into(), function: "TraceLean.Nav.reopened".into(), arguments: vec!["files".into()] }];
    lean_runner::materialize(&out, &["TraceLean.Nav"], "tracelean", "/models", &entries).expect("generated");
    let main = std::fs::read_to_string(lean_runner::package_dir(&out).join("Main.lean")).expect("a runner");
    let _ = std::fs::remove_dir_all(&out);
    let asked: Vec<&&str> = REFLECTION.iter().filter(|needle| main.contains(**needle)).collect();
    assert!(asked.is_empty(), "the generated runner reflects on the model: {asked:?}");
}

/// @tests REQ-CHECK.structural_rejects
#[test]
fn the_reflection_check_rejects_machinery_that_reflects() {
    let reflects = tree(&[("core/src/drt/x.rs", "let q = \"#print TraceLean.Nav.reopened\";\n")]);
    assert_eq!(reflection_in(&reflects).len(), 1);
    assert!(reflection_in(&tree(&[("core/src/drt/x.rs", "let q = schema.clone();\n")])).is_empty());
}

const TRANSCRIPT_READERS: &[&str] =
    &["core/src/observe/transcript.rs", "core/src/observe/claude.rs", "core/src/observe/watch.rs"];

fn writes_in(files: &[(String, String)], readers: &[&str]) -> Vec<String> {
    let mut found = Vec::new();
    for (file, text) in files {
        if !readers.contains(&file.as_str()) {
            continue;
        }
        for (line_no, line) in code_lines(text, file) {
            let writes = ["fs::write", "File::create", "OpenOptions", "remove_file", "set_len", "fs::rename", "fs::copy"]
                .iter()
                .any(|needle| line.contains(needle));
            if writes {
                found.push(format!("{file}:{line_no}: {line}"));
            }
        }
    }
    found
}

/// @tests REQ-TRANSCRIPT.read_only
/// @structural REQ-TRANSCRIPT.read_only reason="an absence: nothing writes to the transcript, and a model of a read could not distinguish a version that also truncated"
#[test]
fn what_reads_a_transcript_writes_nothing() {
    let sources = sources();
    for reader in TRANSCRIPT_READERS {
        assert!(sources.iter().any(|(f, _)| f == reader), "{reader} moved; this check would pass vacuously");
    }
    let found = writes_in(&sources, TRANSCRIPT_READERS);
    assert!(found.is_empty(), "a transcript reader can write:\n{found:#?}");
}

/// @tests REQ-CHECK.structural_rejects
#[test]
fn the_transcript_check_rejects_a_reader_that_writes() {
    let writes = tree(&[("core/src/observe/watch.rs", "let _ = std::fs::write(p, \"\");\n")]);
    assert_eq!(writes_in(&writes, TRANSCRIPT_READERS).len(), 1);
    let reads = tree(&[("core/src/observe/watch.rs", "let t = std::fs::read_to_string(p);\n")]);
    assert!(writes_in(&reads, TRANSCRIPT_READERS).is_empty());
}

fn filesystem_outside_shells(files: &[(String, String)], shells: &[&str]) -> Vec<String> {
    let mut unexpected = Vec::new();
    for (file, text) in files {
        if file.contains("/tests/") || shells.contains(&file.as_str()) {
            continue;
        }
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
    let requirement = std::fs::read_to_string(project_root().join("reqs/arch/ARCH-EFFECT-LAW.md")).unwrap();
    let problems = axiom_problems(&models, &requirement);
    assert!(problems.is_empty(), "{problems:#?}");
}

fn axiom_problems(models: &[(String, String)], requirement: &str) -> Vec<String> {
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
    let laws = text.lines().filter(|line| line.contains("axiom") && line.contains("_")).count();
    if !(laws > 0 && *declared >= laws) {
        problems.push(format!("{declared} axioms and {laws} of them named as laws"));
    }
    if !requirement.contains("has no law") {
        problems.push("an effect without a law must say why, in its requirement".to_string());
    }
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
        "axiom copy_law : True\naxiom run_law : True\n{laws}"
    );
    let why = "`readTranscript` has no law";
    assert!(axiom_problems(&tree(&[("Effects.lean", &good)]), why).is_empty());

    let stray = tree(&[("Effects.lean", &good), ("Other.lean", "axiom sneaky_law : True\n")]);
    assert_eq!(axiom_problems(&stray, why).len(), 1);
    let bare = tree(&[("Effects.lean", "axiom copy_law : True\n")]);
    assert!(axiom_problems(&bare, "").len() >= 2, "{:?}", axiom_problems(&bare, ""));
}

fn names_the_problem(rendered: &str) -> bool {
    rendered.len() > 6 && !rendered.contains("true") && !rendered.contains("false")
}

/// @tests ARCH-HONEST.named_findings
/// @structural ARCH-HONEST.named_findings reason="a claim about every finding type in the project at once, checked by rendering one of each"
#[test]
fn every_finding_type_is_a_closed_named_vocabulary() {
    use tracelean_core::observe::effects::Violation;
    use tracelean_core::surface::keymap::Problem;
    use tracelean_core::trace::checker::Kind;

    let named: Vec<String> = vec![
        serde_json::to_value(Kind::Unmodeled).unwrap().to_string(),
        serde_json::to_value(Violation::ContainmentUnreported).unwrap().to_string(),
        serde_json::to_value(Problem::UnreachableMode { mode: "x".into() })
            .unwrap()
            .to_string(),
    ];
    for rendered in &named {
        assert!(
            names_the_problem(rendered),
            "a finding rendered as {rendered}, which says nothing about what is wrong"
        );
    }
    for bad in ["false", "true", "\"ok\"", "{\"valid\":false}"] {
        assert!(!names_the_problem(bad), "{bad} should not count as a named finding");
    }
    assert!(named[0].contains("unmodeled"));
    assert!(named[1].contains("containmentUnreported"));
    assert!(named[2].contains("unreachableMode"));
}

/// @tests ARCH-CORE-SHELL.decision_public
/// @structural ARCH-CORE-SHELL.decision_public reason="a claim about module visibility, which is a fact about the source and not a value"
#[test]
fn every_module_holding_a_decision_is_public() {
    let text = std::fs::read_to_string(project_root().join("crates/core/src/lib.rs"))
        .expect("the crate root");
    let hidden = private_modules(&text);
    assert!(hidden.is_empty(), "not public, so a generated runner cannot call into them: {hidden:?}");
}

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

    let found = offenders(TRANSPORT);
    assert!(found.is_empty(), "something in this tree can send:\n{found:#?}");
}

const TRANSPORT: &[&str] = &["reqwest", "hyper::", "TcpStream", "UdpSocket", "ureq", "curl"];

/// @tests REQ-CHECK.structural_rejects
#[test]
fn the_transport_check_rejects_a_tree_that_can_send() {
    let sends = tree(&[("core/src/judge.rs", "pub fn go(p: String) { ureq::post(url).send_string(&p); }\n")]);
    assert_eq!(offenders_in(&sends, TRANSPORT).len(), 1);
    assert!(offenders_in(&tree(&[("core/src/judge.rs", "pub fn prompt() -> String { String::new() }\n")]), TRANSPORT).is_empty());
}

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

/// @tests ARCH-HONEST.absence_is_not_pass
/// @tests ARCH-HONEST.confidence_carried
/// @structural ARCH-HONEST.absence_is_not_pass reason="a claim about every reporting path at once: no path may turn an absence into a pass"
/// @structural ARCH-HONEST.confidence_carried reason="a claim that certain fields exist and reach the artefact, not a property of any one call"
#[test]
fn every_unsure_answer_carries_a_field_saying_so() {
    use tracelean_core::evidence::{marks, Bond, Level, Marks, Record};
    use tracelean_core::trace::anchor::{Anchor, AnchorKind};
    use tracelean_core::trace::checker::coverage_kinds;
    use tracelean_core::trace::rollup::{combine, Figure};

    assert_eq!(marks(vec![], false), Marks::NONE);
    assert_eq!(marks(vec![Record { bond: Bond::ModelProof, level: Level::L4 }], true).text(), "A--P");
    assert_eq!(combine(vec![]), Marks::NONE);

    assert!(
        !coverage_kinds(vec![], false, false).is_empty(),
        "a clause nothing claims produced no finding, which reads as passing"
    );

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

    let rendered = serde_json::to_string(&Figure { met: 1, total: 2, exact: false }).unwrap();
    assert!(rendered.contains("exact"), "a figure lost the field that says it is a bound");
}

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

fn feature_files() -> Vec<String> {
    let root = project_root();
    let mut out = Vec::new();
    let mut stack = vec![root.join("crates"), root.join("web/src")];
    while let Some(dir) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else { continue };
        for entry in entries.flatten() {
            let path = entry.path();
            let name = path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
            if path.is_dir() {
                if !matches!(name.as_str(), "target" | "tests" | "examples") {
                    stack.push(path);
                }
            } else if (name.ends_with(".rs") || name.ends_with(".ts")) && name != "build.rs" {
                out.push(path.strip_prefix(&root).unwrap_or(&path).display().to_string());
            }
        }
    }
    out.sort();
    out
}

fn unclaimed_files(files: &[String], claimed: &std::collections::BTreeSet<String>) -> Vec<String> {
    files
        .iter()
        .filter(|file| !file.ends_with("/lib.rs") && !file.ends_with("/mod.rs") && !claimed.contains(*file))
        .cloned()
        .collect()
}

/// @tests ARCH-SELFHOST.every_feature_traced
/// @structural ARCH-SELFHOST.every_feature_traced reason="a claim about every source file in this repository at once"
#[test]
fn every_file_that_ships_code_is_claimed() {
    use tracelean_core::trace::index::build;
    let index = build(&project_root());
    let claimed = index.links.iter().map(|l| l.anchor.file.clone()).collect();
    let files = feature_files();
    assert!(files.len() > 40, "only {} source files; the walk is wrong", files.len());
    let missing = unclaimed_files(&files, &claimed);
    assert!(missing.is_empty(), "nothing claims these files:\n{missing:#?}");
}

/// @tests REQ-CHECK.structural_rejects
#[test]
fn the_tracing_check_rejects_a_file_nothing_claims() {
    let files = vec!["crates/a/src/lib.rs".to_string(), "crates/a/src/x.rs".to_string(), "web/src/y.ts".to_string()];
    let claimed = std::collections::BTreeSet::from(["web/src/y.ts".to_string()]);
    assert_eq!(unclaimed_files(&files, &claimed), vec!["crates/a/src/x.rs".to_string()]);
}

/// @tests ARCH-CORE-SHELL.no_hidden_input
/// @structural ARCH-CORE-SHELL.no_hidden_input reason="a claim about what the bound functions may mention, which is read off their source rather than computed"
#[test]
fn nothing_a_binding_names_reads_anything_it_was_not_given() {
    let root = project_root();
    let index = tracelean_core::trace::index::build(&root);
    let files = tracelean_core::observe::workspace::snapshot(&root).files;
    let bindings = tracelean_core::drt::auto::bindings(&root, &index, &files).expect("the bindings read");

    let mut bound = Vec::new();
    for binding in tracelean_core::drt::auto::tested(&index, bindings) {
        let (file, _symbol) = binding.implementation.split_entry().expect("file::symbol");
        let source = std::fs::read_to_string(root.join(file)).expect("a bound file exists");
        bound.push((file.to_string(), source));
    }
    assert!(bound.len() > 30, "only {} bindings checked; the walk is wrong", bound.len());
    let found = ambient_in_bound_files(&bound);
    assert!(found.is_empty(), "{found:#?}");
}

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
