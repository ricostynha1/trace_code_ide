use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::process::Command;

fn project_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap().parent().unwrap().to_path_buf()
}

fn normalise_stage0(line: &str) -> String {
    let line = line.replace("::impl ", "::");
    let line = line.replace("@file", "");
    line.to_string()
}

fn comparable(anchor: &str) -> String {
    let Some((file, path)) = anchor.split_once(".lean::") else { return anchor.to_string() };
    let own = path.rsplit("::").next().unwrap_or(path);
    format!("{file}.lean::{own}")
}

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

/// @tests ARCH-SELFHOST.fixpoint
/// @structural ARCH-SELFHOST.fixpoint reason="the claim is about two whole programs agreeing on this repository; there is no argument to generate"
#[test]
#[ignore = "runs the stage-0 driver; run with --ignored"]
fn the_two_kernels_agree_about_this_tree() {
    let root = project_root();
    let stage0 = root.join("tools/stage0-trace/target/release/stage0-trace");
    assert!(stage0.exists(), "the stage-0 driver is not built: cargo build --release in tools/stage0-trace");

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
    let new_set: BTreeSet<(String, String)> = new_links
        .iter()
        .filter_map(split_link)
        .filter(|(claim, _)| !claim.starts_with("@specifies "))
        .collect();

    let mut only_old: Vec<&(String, String)> = old_set.difference(&new_set).collect();
    let mut only_new: Vec<&(String, String)> = new_set.difference(&old_set).collect();

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
    assert!(refined > 5,"only {refined} anchors were refinements; the rule may be dead");

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
