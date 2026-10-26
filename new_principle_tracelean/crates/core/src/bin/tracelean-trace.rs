use std::path::PathBuf;

use tracelean_core::trace;

fn state_name(state: &trace::doclink::State) -> &'static str {
    match state {
        trace::doclink::State::Current => "current",
        trace::doclink::State::InReview { .. } => "in-review",
        trace::doclink::State::Dangling => "dangling",
    }
}

fn value_after(args: &[String], flag: &str) -> Option<String> {
    args.iter().position(|a| a == flag).and_then(|at| args.get(at + 1)).cloned()
}

fn read_lines(path: &std::path::Path, start: u32, end: u32) -> String {
    let Ok(text) = std::fs::read_to_string(path) else { return String::new() };
    text.lines()
        .skip(start as usize)
        .take((end.saturating_sub(start) + 1) as usize)
        .collect::<Vec<_>>()
        .join("\n")
}

fn main() {
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

    if std::env::args().any(|a| a == "--hashes") {
        for (target, hash) in trace::doclink::current_hashes(&index) {
            println!("{target}\t{hash}");
        }
        return;
    }

    // @implements REQ-JUDGE.human_decides
    // @implements REQ-JUDGE.prompt_exported
    // @implements REQ-JUDGE.no_call
    if let Some(at) = std::env::args().position(|a| a == "--judge") {
        let args: Vec<String> = std::env::args().collect();
        let Some(target) = args.get(at + 1) else {
            eprintln!("--judge needs a clause, as REQ-X.clause");
            return;
        };
        let (req_id, clause) = { let (r, c) = tracelean_core::drt::config::split_op(&target); (r.to_string(), c.map(String::from)) };

        let Some(_) = trace::material::model_of(&index, &req_id, clause.as_deref()) else {
            eprintln!("nothing models {target}, so there is nothing to judge it against");
            return;
        };
        let read = |role| -> Vec<(trace::material::ModelAt, String)> {
            trace::material::declarations_of(&index, &req_id, clause.as_deref(), role)
                .into_iter()
                .map(|at| {
                    let text = read_lines(&root.join(&at.file), at.start_line, at.end_line);
                    (at, text)
                })
                .collect()
        };
        let models = read(trace::annotation::Role::Models);
        let source = match models.is_empty() {
            true => trace::material::structural_source(
                &trace::material::structural_checks(&index, &req_id, clause.as_deref())
                    .into_iter()
                    .map(|at| {
                        let text = read_lines(&root.join(&at.file), at.start_line, at.end_line);
                        (at, text)
                    })
                    .collect::<Vec<_>>(),
            ),
            false => trace::material::shown_source(&models, &read(trace::annotation::Role::Specifies)),
        };
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

    if std::env::args().any(|a| a == "--drt-due") {
        let files = tracelean_core::observe::workspace::snapshot(&root).files;
        for (file, due) in tracelean_core::drt::auto::suites_due(&root, &index, &files) {
            match due.is_empty() {
                true => println!("current {file}"),
                false => println!("due     {file}  {}", due.join(" ")),
            }
        }
        return;
    }

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
        let report = tracelean_core::drt::proofs::load(&root);
        if report.rules != trace::proofcheck::rules_hash() && !report.proofs.is_empty() {
            found += 1;
            println!("proof check        the rules changed since the proofs were checked\n    redo: tracelean-trace . --proofs");
        }
        let checked: std::collections::BTreeSet<&String> = report.proofs.iter().map(|p| &p.theorem).collect();
        let modelled: std::collections::BTreeSet<(&String, &Option<String>)> = index
            .links
            .iter()
            .filter(|l| l.role == trace::annotation::Role::Models && l.anchor.file.ends_with(".lean"))
            .map(|l| (&l.req_id, &l.clause))
            .collect();
        for link in index
            .links
            .iter()
            .filter(|l| l.role == trace::annotation::Role::Proves && modelled.contains(&(&l.req_id, &l.clause)))
        {
            if let trace::anchor::AnchorKind::Decl { symbol_path } = &link.anchor.kind {
                if link.anchor.file.ends_with(".lean") && !checked.contains(symbol_path) {
                    found += 1;
                    println!("proof check        {symbol_path}: never checked\n    redo: tracelean-trace . --proofs");
                }
            }
        }
        for (clause, shown) in trace::proofcheck::strengths(&index, &report, &files) {
            let held = report.models.iter().find(|m| m.model == shown.model);
            if held.is_some_and(|m| m.pins.iter().any(|p| p.verdict.is_accepted()))
                && !matches!(shown.strength, trace::strength::Strength::Pinned { .. })
            {
                found += 1;
                let name = format!("{}{}", clause.0, clause.1.as_deref().map(|c| format!(".{c}")).unwrap_or_default());
                println!("pinning            {name}: the model or a theorem about it changed\n    redo: tracelean-trace . --proofs");
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

    if std::env::args().any(|a| a == "--proofs") {
        use tracelean_core::drt::proofs;
        let report = match proofs::check(&root, &index) {
            Ok(report) => report,
            Err(why) => {
                eprintln!("{why}");
                std::process::exit(2);
            }
        };
        if let Err(error) = tracelean_core::drt::proofs::save(&root, &report) {
            eprintln!("cannot keep the verdicts: {error}");
        }
        let (written, removed) = proofs::earn_all(&root, &index, &report, &proofs::toolchain());
        let accepted = report.proofs.iter().filter(|p| p.verdict.is_accepted()).count();
        println!("{accepted} of {} theorems accepted; {written} proof records written, {removed} withdrawn", report.proofs.len());
        for entry in report.proofs.iter().filter(|p| !p.verdict.is_accepted()) {
            if let trace::proofcheck::Verdict::Refused { refusal } = &entry.verdict {
                let name = match &entry.clause {
                    Some(clause) => format!("{}.{clause}", entry.req_id),
                    None => entry.req_id.clone(),
                };
                println!("refused  {name}  {}:{}  {}\n    {}", entry.file, entry.line, entry.theorem, refusal.message());
            }
        }
        let pinned = report.models.iter().filter(|m| m.pins.iter().any(|p| p.verdict.is_accepted())).count();
        println!("{pinned} of {} models pinned (tracelean-trace . --pins)", report.models.len());
        return;
    }

    if std::env::args().any(|a| a == "--pins") || std::env::args().any(|a| a == "--strength") {
        let report = tracelean_core::drt::proofs::load(&root);
        let files = tracelean_core::observe::workspace::snapshot(&root).files;
        let shown = trace::proofcheck::strengths(&index, &report, &files);
        let wanted = std::env::args().skip_while(|a| a != "--pins" && a != "--strength").nth(1);
        let mut models: std::collections::BTreeMap<String, (&trace::proofcheck::Shown, Vec<String>)> = Default::default();
        for ((req, clause), s) in &shown {
            let name = match clause {
                Some(c) => format!("{req}.{c}"),
                None => req.clone(),
            };
            models.entry(s.model.clone()).or_insert((s, Vec::new())).1.push(name);
        }
        let mut counts: std::collections::BTreeMap<&str, usize> = Default::default();
        for (model, (s, clauses)) in &models {
            *counts.entry(s.strength.as_str()).or_default() += 1;
            let detail = match &s.strength {
                trace::strength::Strength::Pinned { theorem_name } => theorem_name.clone(),
                trace::strength::Strength::Attempted { theorem_name } => format!("{theorem_name}: not accepted"),
                trace::strength::Strength::Nondeterministic { reason } => reason.clone(),
                trace::strength::Strength::Open => "owes a theorem annotated @pins".into(),
            };
            println!("{:<17} {model}  [{}]  {detail}", s.strength.as_str(), clauses.join(" "));
            if wanted.as_deref().is_some_and(|w| model.contains(w) || clauses.iter().any(|c| c.contains(w))) {
                println!("    obligation:\n{}", s.statement.lines().map(|l| format!("      {l}")).collect::<Vec<_>>().join("\n"));
            }
        }
        println!("== strength ({} models) ==", models.len());
        for (state, n) in &counts {
            println!("  {state:<18} {n}");
        }
        return;
    }

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

    let policy = trace::checker::Policy { today: value_after(&args, "--today"), ..Default::default() };
    let mut findings = trace::checker::check(&index, &policy);
    findings.extend(trace::checker::refused(&tracelean_core::drt::proofs::load(&root), &policy));
    let mut by_kind: std::collections::BTreeMap<String, usize> = Default::default();
    for f in &findings {
        *by_kind.entry(format!("{:?}", f.kind)).or_default() += 1;
    }
    println!("\n== findings ({}) ==", findings.len());
    for (kind, n) in &by_kind {
        println!("  {kind:<18} {n}");
    }

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
