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
        if arg == "--show" {
            let _ = rest.next();
        } else if !arg.starts_with("--") {
            positional.push(arg);
        }
    }
    let root = positional.first().map(PathBuf::from).unwrap_or_else(|| PathBuf::from("."));
    let root = root.canonicalize().unwrap_or(root);
    let index = trace::index::build(&root);

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

        let Some(model_at) = trace::material::model_of(&index, &req_id, clause.as_deref()) else {
            eprintln!("nothing models {target}, so there is nothing to judge it against");
            return;
        };
        let source = read_lines(&root.join(&model_at.file), model_at.start_line, model_at.end_line);
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
            note: value_after(&args, "--note"),
            requirement_hash: material.requirement_hash.clone(),
            model_hash: material.model_hash.clone(),
        };
        match tracelean_core::judge::record(material, judgement, model_at.link_hash) {
            tracelean_core::judge::Outcome::Recorded { evidence } => {
                match trace::store::write(&root, &evidence) {
                    Ok(path) => println!("recorded {target} as judged: {}", path.display()),
                    Err(error) => eprintln!("cannot write the record: {error}"),
                }
            }
            tracelean_core::judge::Outcome::Drifted => {
                println!("{target}: drift recorded as no evidence; the clause and the model differ")
            }
            tracelean_core::judge::Outcome::Proposed { proposal } => {
                println!("{target}: a proposal, which nothing applies for you:\n  {}", proposal.suggestion)
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

    let findings = trace::checker::check(&index, &trace::checker::Policy::default());
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
