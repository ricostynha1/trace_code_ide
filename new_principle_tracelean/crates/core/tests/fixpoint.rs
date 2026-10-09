//! Stage-2 acceptance: the ported kernel and the bootstrap kernel must agree
//! about this tree, except where the port deliberately fixes a defect.
//!
//! @tests ARCH-SELFHOST.fixpoint
//! @structural ARCH-SELFHOST.fixpoint reason="the claim is about two whole programs agreeing on this repository; there is no argument to generate"

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::process::Command;

fn project_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap().parent().unwrap().to_path_buf()
}

/// Divergences the port is right about. Each entry rewrites a stage-0 anchor
/// into the spelling the port produces, with the reason recorded in
/// `docs/decisions/ADR-0007-fixpoint-is-modulo-declared-fixes.md`.
fn normalise_stage0(line: &str) -> String {
    // 1. Stage 0 spells a Rust impl block as a path segment `impl T`.
    let line = line.replace("::impl ", "::");
    // 3. Stage 0 marks a whole-file anchor with an `@file` suffix. Both kernels
    //    agree that the anchor is the file; only the spelling differs.
    let line = line.replace("@file", "");
    line.to_string()
}

/// The form in which the two kernels' anchors are comparable.
///
/// 2. Stage 0 drops Lean namespaces entirely: `namespace … end` is a pair of
///    siblings in the grammar rather than a node containing its declarations,
///    so it anchors `TraceLean.Tree.stateAt` as plain `stateAt`. The port keeps
///    the scope, which is the fix — two declarations of the same name in
///    different namespaces collide without it. Since one side simply lacks the
///    information, Lean anchors are compared by the declaration's own name
///    within its file, and the scopes the port adds are not held against it.
fn comparable(anchor: &str) -> String {
    let Some((file, path)) = anchor.split_once(".lean::") else { return anchor.to_string() };
    let own = path.rsplit("::").next().unwrap_or(path);
    format!("{file}.lean::{own}")
}

/// A link line split into what it claims and where it points.
fn split_link(line: &String) -> Option<(String, String)> {
    line.split_once(" -> ").map(|(claim, anchor)| (claim.to_string(), comparable(anchor)))
}

fn links_from(output: &str) -> Vec<String> {
    let mut links: Vec<String> = output
        .lines()
        .filter(|l| l.trim_start().starts_with('@'))
        .map(|l| l.split_whitespace().collect::<Vec<_>>().join(" "))
        .collect();
    links.sort();
    links
}

#[test]
#[ignore = "runs the stage-0 driver; run with --ignored"]
fn the_two_kernels_agree_about_this_tree() {
    let root = project_root();
    let stage0 = root.join("tools/stage0-trace/target/release/stage0-trace");
    if !stage0.exists() {
        eprintln!("stage-0 driver not built; skipping");
        return;
    }

    let old = Command::new(&stage0).arg(&root).output().expect("stage-0 runs");
    let old_links: Vec<String> = links_from(&String::from_utf8_lossy(&old.stdout))
        .iter()
        .map(|l| normalise_stage0(l))
        .collect();

    let index = tracelean_core::trace::index::build(&root);
    let mut new_links: Vec<String> = index
        .links
        .iter()
        .map(|l| {
            format!(
                "@{} {}{} -> {}",
                l.role.as_str(),
                l.req_id,
                l.clause.as_deref().map(|c| format!(".{c}")).unwrap_or_default(),
                l.anchor.ident()
            )
        })
        .collect();
    new_links.sort();

    let old_set: BTreeSet<(String, String)> = old_links.iter().filter_map(split_link).collect();
    // 5. `@specifies` is a role stage 0 does not have (ADR-0014): it reads
    //    none of those annotations, so the port's are not held against it.
    let new_set: BTreeSet<(String, String)> = new_links
        .iter()
        .filter_map(split_link)
        .filter(|(claim, _)| !claim.starts_with("@specifies "))
        .collect();

    let mut only_old: Vec<&(String, String)> = old_set.difference(&new_set).collect();
    let mut only_new: Vec<&(String, String)> = new_set.difference(&old_set).collect();

    // 4. The port resolves the *member*, where stage 0 stops at the type or
    //    namespace containing it: a Rust `impl` block opens a scope rather than
    //    being a declaration (ADR-0011), and a Lean `def Kind.progress` is one
    //    declaration whose name contains a dot. Stage 0 gave several members of
    //    the same type one anchor between them, which is exactly the ambiguity
    //    the port's scheme exists to remove. Matched here rather than rewritten,
    //    because the member's name cannot be recovered from the type's.
    let mut refined = 0;
    only_old.retain(|(key, anchor)| {
        let found = only_new.iter().any(|(k, a)| {
            k == key
                && a.len() > anchor.len()
                && a.starts_with(anchor.as_str())
                && matches!(&a[anchor.len()..anchor.len() + 1], ":" | ".")
        });
        if found {
            refined += 1;
        }
        !found
    });
    only_new.retain(|(key, anchor)| {
        !old_set.iter().any(|(k, a)| {
            k == key
                && anchor.len() > a.len()
                && anchor.starts_with(a.as_str())
                && matches!(&anchor[a.len()..a.len() + 1], ":" | ".")
        })
    });
    // A liveness check on the rule, not a target: annotations that move onto
    // their enclosing type (ADR-0014) lower the count.
    assert!(refined > 5,"only {refined} anchors were refinements; the rule may be dead");

    // 6. An annotation inside a declaration — a Lean constructor's doc comment,
    //    a Rust variant's or field's, a comment in a function body — binds to
    //    that declaration in the port and to the next declaration in stage 0
    //    (ADR-0014). The same claim in the same Lean or Rust file, once on
    //    each side, is that one rebinding.
    let lean_file = |a: &str| {
        a.split_once("::")
            .map(|(f, _)| f.to_string())
            .filter(|f| f.ends_with(".lean") || f.ends_with(".rs"))
    };
    let rebound: Vec<(String, String)> = only_old
        .iter()
        .filter(|(key, anchor)| {
            only_new.iter().filter(|(k, a)| k == key && lean_file(a).is_some() && lean_file(a) == lean_file(anchor)).count() == 1
        })
        .map(|(key, anchor)| (key.clone(), lean_file(anchor).unwrap_or_default()))
        .collect();
    only_old.retain(|(key, anchor)| !rebound.contains(&(key.clone(), lean_file(anchor).unwrap_or_default())));
    only_new.retain(|(key, anchor)| !rebound.contains(&(key.clone(), lean_file(anchor).unwrap_or_default())));
    // When stage 0's next declaration already carried the same claim from an
    // annotation of its own, the rebinding has no old half: links are a set,
    // so the moved one merged into it. A port-only link whose claim stage 0
    // also makes in the same file, with nothing else left over, is that.
    if only_old.is_empty() {
        only_new.retain(|(key, anchor)| {
            let file = lean_file(anchor);
            file.is_none() || !old_set.iter().any(|(k, a)| k == key && lean_file(a) == file)
        });
    }

    assert!(
        only_old.is_empty() && only_new.is_empty(),
        "the kernels disagree.\n  only stage 0: {only_old:#?}\n  only the port: {only_new:#?}"
    );
}

/// A second run over the same tree must produce the same index.
///
/// @tests ARCH-SELFHOST.idempotent
/// @tests ARCH-DETERMINISM.same_input_same_bytes
/// @structural ARCH-SELFHOST.idempotent reason="a claim about running the system twice over a real tree"
/// @structural ARCH-DETERMINISM.same_input_same_bytes reason="the input is a repository, which is not a value the schema grammar can describe"
#[test]
fn building_the_index_twice_gives_the_same_answer() {
    let root = project_root();
    let a = tracelean_core::trace::index::build(&root);
    let b = tracelean_core::trace::index::build(&root);
    assert_eq!(
        serde_json::to_string(&a).unwrap(),
        serde_json::to_string(&b).unwrap()
    );
}
