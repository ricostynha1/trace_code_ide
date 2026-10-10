//! Run this project's own trace kernel over a tree.
//!
//! Stage 2 of the bootstrap: what `tools/stage0-trace` does with the old
//! kernel, this does with the ported one, so the two can be compared.

use std::path::PathBuf;

use tracelean_core::trace;

fn state_name(state: &trace::doclink::State) -> &'static str {
    match state {
        trace::doclink::State::Current => "current",
        trace::doclink::State::InReview { .. } => "in-review",
        trace::doclink::State::Dangling => "dangling",
    }
}

/// The value after a flag, if it was given one.
fn value_after(args: &[String], flag: &str) -> Option<String> {
    args.iter().position(|a| a == flag).and_then(|at| args.get(at + 1)).cloned()
}

/// The lines a declaration spans, as the judge should read them.
///
/// Inclusive of the last line, and one past it when the anchor's end is the
/// declaration's own closing line: a model shown with its final line cut off is
/// a model nobody can judge.
fn read_lines(path: &std::path::Path, start: u32, end: u32) -> String {
    let Ok(text) = std::fs::read_to_string(path) else { return String::new() };
    text.lines()
        .skip(start as usize)
        .take((end.saturating_sub(start) + 1) as usize)
        .collect::<Vec<_>>()
        .join("\n")
}

fn main() {
    // The first argument that is not a flag, nor a flag's value, is the root.
    let mut positional = Vec::new();
    let mut rest = std::env::args().skip(1);
    while let Some(arg) = rest.next() {
        if matches!(arg.as_str(), "--show" | "--context" | "--parts" | "--today") {
            let _ = rest.next();
        } else if !arg.starts_with("--") {
            positional.push(arg);
        }
    }
    let root = positional.first().map(PathBuf::from).unwrap_or_else(|| PathBuf::from("."));
    let root = root.canonicalize().unwrap_or(root);
    let index = trace::index::build(&root);

    // `--context REQ-X[.clause] [--parts code,tests,…|all]` prints what an
    // agent needs to change it — the editor's context page, as Markdown, for
    // an agent to gather itself.
    //
    // @implements REQ-CONTEXT.from_the_shell
    let args: Vec<String> = std::env::args().collect();
    if let Some(target) = value_after(&args, "--context") {
        let files = tracelean_core::observe::workspace::snapshot(&root).files;
        let parts = value_after(&args, "--parts");
        match tracelean_core::surface::context::for_the_shell(&target, parts.as_deref(), &index, &files) {
            Ok(text) => print!("{text}"),
            Err(why) => {
                eprintln!("{why}");
                std::process::exit(2);
            }
        }
        return;
    }

    // `--hashes` prints what every target currently hashes to, which is what a
    // document records when somebody confirms it against what it describes.
    if std::env::args().any(|a| a == "--hashes") {
        for (target, hash) in trace::doclink::current_hashes(&index) {
            println!("{target}\t{hash}");
        }
        return;
    }

    // `--judge REQ.clause` puts the clause and the model in front of a person.
    //
    // Without `--verdict` it prints the judging prompt and stops, which is
    // `prompt_exported`: the person carries it to whatever tool they like. With
    // `--verdict` it records *their* decision. Nothing here calls anything.
    //
    // @implements REQ-JUDGE.human_decides
    // @implements REQ-JUDGE.prompt_exported
    // @implements REQ-JUDGE.no_call
    if let Some(at) = std::env::args().position(|a| a == "--judge") {
        let args: Vec<String> = std::env::args().collect();
        let Some(target) = args.get(at + 1) else {
            eprintln!("--judge needs a clause, as REQ-X.clause");
            return;
        };
        let (req_id, clause) = match target.split_once('.') {
            Some((req, clause)) => (req.to_string(), Some(clause.to_string())),
            None => (target.clone(), None),
        };

        let Some(_) = trace::material::model_of(&index, &req_id, clause.as_deref()) else {
            eprintln!("nothing models {target}, so there is nothing to judge it against");
            return;
        };
        // The model and the specification, and every model while a clause
        // still has several (ADR-0014).
        let read = |role| -> Vec<(trace::material::ModelAt, String)> {
            trace::material::declarations_of(&index, &req_id, clause.as_deref(), role)
                .into_iter()
                .map(|at| {
                    let text = read_lines(&root.join(&at.file), at.start_line, at.end_line);
                    (at, text)
                })
                .collect()
        };
        let source = trace::material::shown_source(
            &read(trace::annotation::Role::Models),
            &read(trace::annotation::Role::Specifies),
        );
        let assembled =
            trace::material::assemble(&index, &req_id, clause.as_deref(), source, None);
        let (material, model_at) = match assembled {
            Ok(pair) => pair,
            Err(why) => {
                eprintln!("cannot assemble the material: {why:?}");
                return;
            }
        };

        let verdict = value_after(&args, "--verdict");
        let Some(verdict) = verdict else {
            print!("{}", tracelean_core::judge::prompt(material));
            return;
        };
        let Some(by) = value_after(&args, "--by") else {
            eprintln!("--verdict needs --by <name>: a judgement is attributable or it is not one");
            return;
        };
        let verdict = match verdict.as_str() {
            "agrees" => tracelean_core::judge::Verdict::Agrees,
            "drift" => tracelean_core::judge::Verdict::Drift,
            "unmodelable" => tracelean_core::judge::Verdict::Unmodelable,
            other => {
                eprintln!("`{other}` is not a verdict: agrees, drift or unmodelable");
                return;
            }
        };
        let judgement = tracelean_core::judge::Judgement {
            verdict,
            judged_by: by,
            delegated_by: value_after(&args, "--delegated-by"),
            note: value_after(&args, "--note"),
            requirement_hash: material.requirement_hash.clone(),
            model_hash: material.model_hash.clone(),
        };
        // Who may judge: `.tracelean/judges.json`. Without one, a name is taken
        // at its word, and the person recording is told so.
        let judges = match std::fs::read_to_string(root.join(".tracelean").join("judges.json")) {
            Ok(text) => match tracelean_core::judge::parse_judges(&text) {
                Ok(judges) => Some(judges),
                Err(why) => {
                    eprintln!("{why}");
                    std::process::exit(2);
                }
            },
            Err(_) => {
                println!("no .tracelean/judges.json: this verdict is attributed to a name, not to a listed person");
                None
            }
        };
        use tracelean_core::judge::Outcome;
        // Every verdict goes in the clause's one judgement slot, so a drift
        // replaces an agreement recorded before, in the store and — once the
        // lock is collected again below — in the lock.
        let (evidence, said) = match tracelean_core::judge::record(material, judgement, judges, model_at.link_hash) {
            Outcome::Refused { reason } => {
                eprintln!("refused: {reason}");
                std::process::exit(2);
            }
            Outcome::Recorded { evidence } => (evidence, format!("{target}: agrees, recorded at L2")),
            Outcome::Drifted { evidence } => {
                (evidence, format!("{target}: drift recorded at L1; the clause and the model differ"))
            }
            Outcome::Proposed { evidence, proposal } => (
                evidence,
                format!(
                    "{target}: unmodelable, recorded at L1, and a proposal which nothing applies for you:\n  {}",
                    proposal.suggestion
                ),
            ),
        };
        match trace::store::write(&root, &evidence) {
            Ok(path) => println!("{said}: {}", path.display()),
            Err(error) => {
                eprintln!("cannot write the record: {error}");
                std::process::exit(1);
            }
        }
        let held = trace::lockfile::read(&root).map(|l| l.evidence).unwrap_or_default();
        let collected = trace::lockfile::collected(&index, held, trace::store::read_all(&root));
        if let Err(error) = trace::lockfile::write(&root, &collected.lockfile) {
            eprintln!("cannot write the lock: {error}");
        }
        return;
    }

    // `--drt-due` says, for each differential suite, whether a clause it claims
    // has no current agreed run (`due`) or none does (`current`): what
    // `tools/differential-all.sh` runs, and what it skips.
    if std::env::args().any(|a| a == "--drt-due") {
        for (file, due) in tracelean_core::drt::auto::suites_due(&root, &index) {
            match due.is_empty() {
                true => println!("current {file}"),
                false => println!("due     {file}  {}", due.join(" ")),
            }
        }
        return;
    }

    // `--drt` tests every clause a Lean function models and a Rust function
    // implements against each other, without a binding, records L3 where it
    // was earned, and refreshes the lock so the editor sees it.
    if std::env::args().any(|a| a == "--drt") {
        use tracelean_core::drt::auto::{run_all, Outcome};
        let files = tracelean_core::observe::workspace::snapshot(&root).files;
        let again = std::env::args().any(|a| a == "--again");
        let outcomes = run_all(&root, &index, &files, again);
        if outcomes.is_empty() {
            println!("nothing to test: no clause has both a Lean `def` modelling it and a Rust `fn` implementing it, unbound");
        }
        for (candidate, outcome) in &outcomes {
            let op = &candidate.op;
            match outcome {
                Outcome::Agreed { cases } => println!("agreed     {op}  {cases} cases, every class reached: L3"),
                Outcome::Cached => println!("cached     {op}  agreed before, and nothing it ran on changed (--again re-runs)"),
                Outcome::Uncovered { missing } => println!("uncovered  {op}  never generated: {}", missing.join(", ")),
                Outcome::Diverged { input, model, implementation } => {
                    println!("DIVERGED   {op}\n    input {input}\n    model {model}\n    code  {implementation}")
                }
                Outcome::Mismatch(problems) => {
                    println!("mismatch   {op}  ({} against {})", candidate.model.1, candidate.implementation.1);
                    for problem in problems {
                        println!("    {problem}");
                    }
                }
                Outcome::Failed(why) => {
                    println!("failed     {op}");
                    for line in why.lines().take(15) {
                        println!("    {line}");
                    }
                }
            }
        }
        let index = trace::index::build(&root);
        let held = trace::lockfile::read(&root).map(|l| l.evidence).unwrap_or_default();
        let collected = trace::lockfile::collected(&index, held, trace::store::read_all(&root));
        if let Err(error) = trace::lockfile::write(&root, &collected.lockfile) {
            eprintln!("cannot write the lock: {error}");
        }
        return;
    }

    // `--stale` lists everything that must be approved, tested, proved or
    // measured again because something it rests on changed — evidence, pinning
    // verdicts, documents, coverage — with what to run for each. Exit status 1
    // when anything is listed, so a script or an agent can check it.
    //
    // @implements REQ-STALE.listed_for_a_script
    if std::env::args().any(|a| a == "--stale") {
        use trace::record::Staleness;
        let files = tracelean_core::observe::workspace::snapshot(&root).files;
        let mut found = 0;
        let held = trace::lockfile::read(&root).map(|l| l.evidence).unwrap_or_default();
        for record in trace::earn::merge(held, trace::store::read_all(&root)) {
            let Some(why) = trace::earn::why_stale(&index, &record) else { continue };
            found += 1;
            let name = match &record.key.clause {
                Some(clause) => format!("{}.{clause}", record.key.req_id),
                None => record.key.req_id.clone(),
            };
            let changed = trace::earn::changed_inputs(&index, &record);
            let why = match why {
                Staleness::InputChanged { .. } => format!("changed: {}", changed.join(", ")),
                Staleness::LinkRetargeted if changed.is_empty() => {
                    "its annotation now points elsewhere".to_string()
                }
                Staleness::LinkRetargeted => {
                    format!("its annotation now points elsewhere; changed: {}", changed.join(", "))
                }
            };
            let (what, redo) = match record.key.bond {
                tracelean_core::evidence::Bond::RequirementModel => {
                    ("judgement", format!("tracelean-trace . --judge {name} --verdict agrees|drift|unmodelable --by <who>"))
                }
                tracelean_core::evidence::Bond::ModelImpl => ("differential test", "tracelean-trace . --drt (or its DRT suite)".to_string()),
                tracelean_core::evidence::Bond::ModelProof => ("proof", "rebuild the Lean and earn the proof again".to_string()),
            };
            println!("{what:<18} {name}: {why}\n    redo: {redo}");
        }
        for plan in trace::pinning::plans(&index, &files) {
            let Some(theorem) = &plan.theorem else { continue };
            let record = tracelean_core::drt::pins::read_record(&root, &plan.req_id, plan.clause.as_deref());
            if record.as_ref().is_some_and(|r| r.pinned && (&r.theorem_name != theorem || r.key != plan.key)) {
                found += 1;
                let name = format!("{}{}", plan.req_id, plan.clause.as_deref().map(|c| format!(".{c}")).unwrap_or_default());
                println!("pinning            {name}: the specification, model or theorem changed\n    redo: tracelean-trace . --pins");
            }
        }
        let hashes = trace::doclink::pairs(&trace::doclink::current_hashes(&index));
        for link in &index.doc_links {
            let state = trace::doclink::state(link.clone(), hashes.clone());
            if !matches!(state, trace::doclink::State::Current) {
                found += 1;
                println!(
                    "document           {} -> {}: {}\n    redo: review it, then record the hashes from tracelean-trace . --hashes",
                    link.file,
                    link.target,
                    state_name(&state)
                );
            }
        }
        let coverage = tracelean_core::drt::lines_run::read(&root);
        for (path, (hash, _)) in &coverage.files {
            if files.get(path).map(|t| trace::hash::text(t)) != Some(hash.clone()) {
                found += 1;
                println!("coverage           {path}: changed since it was measured\n    redo: tracelean-trace . --coverage");
            }
        }
        println!("{found} to redo");
        std::process::exit(if found > 0 { 1 } else { 0 });
    }

    // `--coverage` runs each Rust test alone under coverage and keeps, per
    // file, which tests ran each line and how often.
    // Not again while every Rust source hashes as it did (`--again` does).
    if std::env::args().any(|a| a == "--coverage") {
        use tracelean_core::drt::lines_run;
        let files = tracelean_core::observe::workspace::snapshot(&root).files;
        let sources = tracelean_core::trace::lines::sources_hash(&files);
        let held = lines_run::read(&root);
        if !tracelean_core::trace::lines::retake(&held, &files, std::env::args().any(|a| a == "--again")) {
            println!("cached: no Rust source changed since it was measured ({} files; --again re-measures)", held.files.len());
            return;
        }
        match lines_run::measure(&root, &files) {
            Ok((mut coverage, tests)) => {
                coverage.sources = sources;
                for (path, (_, lines)) in &coverage.files {
                    let reached = lines.iter().filter(|l| l.hits > 0).count();
                    println!("{reached:>5}/{:<5} {path}", lines.len());
                }
                println!("{tests} tests, {} files", coverage.files.len());
                if let Err(error) = lines_run::write(&root, &coverage) {
                    eprintln!("cannot keep the coverage: {error}");
                }
            }
            Err(why) => {
                eprintln!("{why}");
                std::process::exit(2);
            }
        }
        return;
    }

    // `--pins` asks Lean whether each `@pins` theorem pins its clause, keeps the
    // verdict under `.tracelean/pins`, and says what an unpinned clause owes.
    if std::env::args().any(|a| a == "--pins") {
        use trace::pinning::{plans, statement};
        let files = tracelean_core::observe::workspace::snapshot(&root).files;
        for plan in plans(&index, &files) {
            let name = match &plan.clause {
                Some(clause) => format!("{}.{clause}", plan.req_id),
                None => plan.req_id.clone(),
            };
            match tracelean_core::drt::pins::check(&root, &plan) {
                Some(record) if record.pinned => println!("pinned     {name}  {}", record.theorem_name),
                Some(record) => {
                    println!("attempted  {name}  {}: Lean did not accept it", record.theorem_name);
                    for line in record.said.lines().take(12) {
                        println!("    {line}");
                    }
                }
                None => match (&plan.spec, &plan.model) {
                    (Some(spec), Some(model)) => println!(
                        "open       {name}  owes, annotated @pins {name}:\n    theorem … : {}",
                        statement(spec.clone(), model.clone(), plan.inputs)
                    ),
                    _ => println!("open       {name}  {}", trace::pinning::OPEN_WITHOUT_BOTH),
                },
            }
        }
        return;
    }

    // `--strength` lists what each modelled declaration owes, and where it
    // stands. Nothing here confirms a pinning proof: only a build can say the
    // kernel accepted it, and this command builds nothing.
    if std::env::args().any(|a| a == "--strength") {
        let obligations = trace::strength::obligations(&index);
        let mut counts: std::collections::BTreeMap<&str, usize> = Default::default();
        for obligation in &obligations {
            *counts.entry(obligation.strength.as_str()).or_default() += 1;
        }
        println!("== strength ({}) ==", obligations.len());
        for (state, n) in &counts {
            println!("  {state:<18} {n}");
        }
        for obligation in &obligations {
            if !matches!(obligation.strength, trace::strength::Strength::Open) {
                println!(
                    "  {:<10} {}  ({} theorem(s))",
                    obligation.strength.as_str(),
                    obligation.symbol,
                    obligation.theorems.len()
                );
            }
        }
        // `--strength <symbol>` prints the obligation somebody would have to
        // prove, which is the whole of what this system does about L4 strength.
        if let Some(at) = std::env::args().position(|a| a == "--strength") {
            if let Some(wanted) = std::env::args().nth(at + 1) {
                if let Some(obligation) = obligations.iter().find(|o| o.symbol.contains(&wanted)) {
                    println!("\n{}", obligation.lean_source());
                }
            }
        }
        return;
    }

    // `--lock` writes the committed index, carrying through whatever evidence
    // the existing lock records. Nothing here earns evidence: rendering is a
    // function of the tree, and a backend is the only thing that may add a
    // record.
    if std::env::args().any(|a| a == "--lock") {
        let held = trace::lockfile::read(&root).map(|l| l.evidence).unwrap_or_default();
        let earned = trace::store::read_all(&root);
        let collected = trace::lockfile::collected(&index, held, earned);
        let rendered = collected.lockfile;
        match trace::lockfile::write(&root, &rendered) {
            Ok(path) => {
                println!(
                    "{}\n  {} requirements, {} links, {} evidence records",
                    path.display(),
                    rendered.requirements.len(),
                    rendered.links.len(),
                    rendered.evidence.len()
                );
                for record in &collected.dropped {
                    // Named, because a record that went stale and a record that
                    // was never earned look the same in a shorter lock file.
                    println!(
                        "  stale: {}{} {:?}",
                        record.key.req_id,
                        record.key.clause.as_deref().map(|c| format!(".{c}")).unwrap_or_default(),
                        record.key.bond
                    );
                }
            }
            Err(error) => eprintln!("cannot write the lock: {error}"),
        }
        return;
    }

    // `--unparsed <file>` prints the regions of one file the grammar could not
    // read, with the lines around them. `Imprecise` says a claim is capped; this
    // says where to look to lift the cap.
    if let Some(at) = std::env::args().position(|a| a == "--unparsed") {
        let Some(name) = std::env::args().nth(at + 1) else {
            eprintln!("--unparsed needs a file");
            return;
        };
        let path = root.join(&name);
        let Ok(content) = std::fs::read_to_string(&path) else {
            eprintln!("cannot read {}", path.display());
            return;
        };
        let lang = path
            .extension()
            .and_then(|e| e.to_str())
            .and_then(trace::anchor::Lang::from_extension);
        let scan = trace::anchor::scan(&content, lang);
        for (start, end) in &scan.error_ranges {
            let line = content[..*start].lines().count();
            let text = content[*start..*end].lines().next().unwrap_or_default();
            println!("{name}:{line}  {text}");
        }
        // A declaration can fail to parse inside a comment or a literal, which
        // the ranges above do not reach. Naming the declaration is what the
        // person fixing it needs either way.
        for decl in scan.declarations.iter().filter(|d| !d.precise) {
            println!("{name}:{}  declaration `{}` did not parse", decl.start_line + 1, decl.symbol_path);
        }
        if scan.error_ranges.is_empty() && scan.declarations.iter().all(|d| d.precise) {
            println!("{name}: parses cleanly");
        }
        return;
    }

    println!("== requirements ({}) ==", index.requirements.len());
    for (id, req) in &index.requirements {
        println!(
            "  {id:<20} {:<9} {:<9} clauses={:<3} refines={:?}",
            req.status.as_str(),
            req.decomposition.as_str(),
            req.clauses.len(),
            req.refines
        );
    }

    println!("\n== links ({}) ==", index.links.len());
    for link in &index.links {
        println!(
            "  @{:<11} {}{} -> {}",
            link.role.as_str(),
            link.req_id,
            link.clause.as_deref().map(|c| format!(".{c}")).unwrap_or_default(),
            link.anchor.ident()
        );
    }

    println!("\n== problems ({}) ==", index.problems.len());
    for p in &index.problems {
        println!("  {}:{} {}", p.file, p.line, p.message);
    }

    let hashes = trace::doclink::pairs(&trace::doclink::current_hashes(&index));
    let mut doc_states: std::collections::BTreeMap<String, usize> = Default::default();
    let mut in_review = Vec::new();
    for link in &index.doc_links {
        let state = trace::doclink::state(link.clone(), hashes.clone());
        *doc_states.entry(state_name(&state).to_string()).or_default() += 1;
        if !matches!(state, trace::doclink::State::Current) {
            in_review.push((link.file.clone(), link.target.clone(), state));
        }
    }
    println!("\n== documents ({}) ==", index.doc_links.len());
    for (state, n) in &doc_states {
        println!("  {state:<10} {n}");
    }
    for (file, target, state) in &in_review {
        println!("  {file} -> {target}: {}", state_name(state));
    }

    // Exemptions are judged expired only against a date given here (`--today
    // 2026-10-11`, as a CI job would pass it): the check reads no clock.
    let policy = trace::checker::Policy { today: value_after(&args, "--today"), ..Default::default() };
    let findings = trace::checker::check(&index, &policy);
    let mut by_kind: std::collections::BTreeMap<String, usize> = Default::default();
    for f in &findings {
        *by_kind.entry(format!("{:?}", f.kind)).or_default() += 1;
    }
    println!("\n== findings ({}) ==", findings.len());
    for (kind, n) in &by_kind {
        println!("  {kind:<18} {n}");
    }

    // `--show <Kind>` lists one kind in full. A summary count is what a project
    // is graded on; the list is what somebody fixing it needs.
    let args: Vec<String> = std::env::args().collect();
    if let Some(at) = args.iter().position(|a| a == "--show") {
        if let Some(wanted) = args.get(at + 1) {
            println!("\n== {wanted} ==");
            for f in findings.iter().filter(|f| format!("{:?}", f.kind) == *wanted) {
                println!("  {}", f.message);
            }
        }
    }
    println!(
        "\n== scanned {} files ==  blocking: {}",
        index.scanned.len(),
        trace::checker::blocks(&findings)
    );
}
