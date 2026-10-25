mod harness;

use std::collections::BTreeSet;
use std::process::Command;

use tracelean_core::trace::anchor::AnchorKind;
use tracelean_core::trace::annotation::Role;
use tracelean_core::trace::index::{build, Index};
use tracelean_core::trace::{earn, store};

fn toolchain() -> String {
    let out = Command::new("lean").arg("--version").output().expect("lean runs");
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

fn unproved(build_output: &str) -> BTreeSet<(String, u32)> {
    let mut out = BTreeSet::new();
    for line in build_output.lines() {
        if !line.contains("declaration uses 'sorry'") {
            continue;
        }
        let pieces: Vec<&str> = line.split(':').map(str::trim).collect();
        let Some(at) = pieces.iter().position(|p| p.ends_with(".lean")) else { continue };
        let file = pieces[at].trim_start_matches("./").trim_start_matches("./").to_string();
        let at_line = pieces.get(at + 1).and_then(|n| n.parse::<u32>().ok()).unwrap_or(0);
        out.insert((file, at_line));
    }
    out
}

fn covers(file: &str, start: u32, end: u32, warned: &(String, u32)) -> bool {
    (file.ends_with(warned.0.as_str()) || warned.0.ends_with(file))
        && warned.1 >= start
        && warned.1 <= end + 1
}

struct Proved {
    req_id: String,
    clause: Option<String>,
    theorem: String,
    file: String,
    start: u32,
    end: u32,
}

fn proved_links(index: &Index) -> Vec<Proved> {
    index
        .links
        .iter()
        .filter(|link| link.role == Role::Proves)
        .filter_map(|link| match &link.anchor.kind {
            AnchorKind::Decl { symbol_path } => Some(Proved {
                req_id: link.req_id.clone(),
                clause: link.clause.clone(),
                theorem: symbol_path.clone(),
                file: link.anchor.file.clone(),
                start: link.anchor.start_line,
                end: link.anchor.end_line,
            }),
            _ => None,
        })
        .collect()
}

/// @tests REQ-EVID.mark_per_bond
/// @tests REQ-EVID.record_reproducible
/// @tests REQ-STALE.inputs_identified
#[test]
#[ignore = "builds the Lean models"]
fn every_theorem_the_kernel_accepts_earns_its_clause_a_record() {
    let root = harness::project_root();
    let formal = root.join("formal");

    let built = harness::build_formal(&formal);
    let output = format!(
        "{}{}",
        String::from_utf8_lossy(&built.stdout),
        String::from_utf8_lossy(&built.stderr)
    );
    assert!(built.status.success(), "the models did not build:\n{output}");

    let held_back = unproved(&output);
    let toolchain = toolchain();
    assert!(!toolchain.is_empty(), "no toolchain to name as an input");

    let index = build(&root);
    let links = proved_links(&index);
    assert!(links.len() > 50, "only {} proofs found; the index is wrong", links.len());

    let mut written = 0;
    let mut skipped = Vec::new();
    for link in links {
        if held_back.iter().any(|warned| covers(&link.file, link.start, link.end, warned)) {
            skipped.push(format!("{}::{} (uses sorry)", link.file, link.theorem));
            continue;
        }
        match earn::proof_record(
            &index,
            &link.req_id,
            link.clause.as_deref(),
            &link.theorem,
            &toolchain,
        ) {
            Ok(record) => {
                store::write(&root, &record).expect("the store is writable");
                written += 1;
            }
            Err(why) => skipped.push(format!("{}{:?}: {why:?}", link.req_id, link.clause)),
        }
    }

    assert!(written > 0, "no proof earned a record; skipped:\n{skipped:#?}");
    println!("{written} proof records written, {} skipped", skipped.len());
    for reason in &skipped {
        println!("  skipped {reason}");
    }

    let read_back = store::read_all(&root);
    assert!(
        read_back.iter().any(|record| matches!(
            record.detail,
            tracelean_core::trace::record::Detail::Proof { .. }
        )),
        "the store holds no proof record after writing {written}"
    );
}

/// @tests REQ-STRENGTH.attempted_distinguished
/// @tests REQ-STRENGTH.qualifies_proof
#[test]
#[ignore = "builds the Lean models"]
fn a_pinning_proof_the_kernel_accepted_reads_as_pinned() {
    use tracelean_core::trace::strength::{confirm, obligations, Strength};

    let root = harness::project_root();
    let built = harness::build_formal(&root.join("formal"));
    let output = format!(
        "{}{}",
        String::from_utf8_lossy(&built.stdout),
        String::from_utf8_lossy(&built.stderr)
    );
    assert!(built.status.success(), "the models did not build:\n{output}");
    let held_back = unproved(&output);

    let index = build(&root);
    let mut all = obligations(&index);
    let mut pinned = Vec::new();
    for obligation in all.iter_mut() {
        let Strength::Attempted { theorem_name } = obligation.strength.clone() else { continue };
        let warned = held_back
            .iter()
            .any(|(file, _)| theorem_name.contains(file.as_str()) || file.contains(&theorem_name));
        confirm(obligation, !warned);
        if matches!(obligation.strength, Strength::Pinned { .. }) {
            pinned.push(obligation.symbol.clone());
        }
    }

    assert!(
        pinned.iter().any(|symbol| symbol.ends_with("Rollup::combine")),
        "the pinning proof for `combine` did not confirm; pinned: {pinned:#?}"
    );
    println!("{} obligation(s) pinned: {pinned:#?}", pinned.len());

    let open = all.iter().filter(|o| matches!(o.strength, Strength::Open)).count();
    assert!(open > 0, "every obligation read as settled, which cannot be right yet");
    println!("{open} obligation(s) still open");
}

#[test]
fn a_declaration_that_uses_sorry_is_never_recorded() {
    let warned = unproved(
        "warning: ./TraceLean/Strength.lean:12:8: declaration uses 'sorry'\n\
         info: TraceLean.Act: build succeeded\n",
    );
    assert_eq!(
        warned.iter().next(),
        Some(&("TraceLean/Strength.lean".to_string(), 12)),
        "the warning was not read: {warned:?}"
    );
    assert!(unproved("info: build succeeded\n").is_empty());

    let at = &("TraceLean/Strength.lean".to_string(), 12);
    assert!(covers("formal/TraceLean/Strength.lean", 10, 14, at), "the owner was not held back");
    assert!(!covers("formal/TraceLean/Strength.lean", 20, 24, at), "a neighbour was held back");
    assert!(!covers("formal/TraceLean/Act.lean", 10, 14, at), "another file was held back");
}
