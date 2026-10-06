//! Stage-0 driver.
//!
//! Runs the *existing* TraceLean's trace kernel over this project and prints
//! what it found. This is the only way anything here is checked until the
//! ported kernel exists, and it is the reason every on-disk format in this tree
//! is constrained to what stage 0 already parses (ADR-0001).
//!
//! It is deliberately a separate crate under `tools/` rather than a dependency
//! of the port: nothing in `crates/` may depend on stage 0, or the bootstrap
//! would never end.

use std::path::PathBuf;

use tracelean_core::trace;

fn main() {
    let root = std::env::args()
        .nth(1)
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."));
    let root = root.canonicalize().unwrap_or(root);

    let index = trace::build(&root);

    println!("== requirements ({}) ==", index.requirements.len());
    for (id, req) in &index.requirements {
        println!(
            "  {id:<20} {:<10} {:<9} clauses={:<3} refines={:?}",
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

    println!("\n== findings ({}) ==", index.findings.len());
    let mut by_kind: std::collections::BTreeMap<String, usize> = Default::default();
    for f in &index.findings {
        *by_kind
            .entry(format!("{:?}", f.kind))
            .or_default() += 1;
    }
    for (kind, n) in &by_kind {
        println!("  {kind:<22} {n}");
    }

    // Frontmatter problems are the ones that mean a document this project
    // wrote is not being read the way it was meant. Print them in full: a
    // requirement that silently fails to parse is worse than a missing one.
    println!("\n== detail ==");
    for f in &index.findings {
        println!("  [{:?}] {:?} {}", f.severity, f.kind, f.message);
    }
}
