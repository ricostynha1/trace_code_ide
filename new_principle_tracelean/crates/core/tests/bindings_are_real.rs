use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use tracelean_core::trace::annotation::Role;
use tracelean_core::trace::index::build;

fn project_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap().parent().unwrap().to_path_buf()
}

fn merged(root: &Path, index: &tracelean_core::trace::index::Index) -> Vec<tracelean_core::drt::Binding> {
    let files = tracelean_core::observe::workspace::snapshot(root).files;
    tracelean_core::drt::auto::bindings(root, index, &files).expect("the bindings read")
}

fn written(root: &Path) -> BTreeSet<String> {
    tracelean_core::drt::config::entries(root).expect("drt.json").iter().map(tracelean_core::drt::config::entry_op).collect()
}

/// @tests REQ-DRT-BIND.binding_is_the_bond
/// @structural REQ-DRT-BIND.binding_is_the_bond reason="a claim that the binding file and the annotations name the same bonds, read off both"
#[test]
fn every_drt_annotation_has_a_binding_and_every_binding_has_an_annotation() {
    let root = project_root();
    let index = build(&root);

    let annotated: BTreeSet<String> = index
        .links
        .iter()
        .filter(|link| link.role == Role::Drt)
        .map(|link| match &link.clause {
            Some(clause) => format!("{}.{clause}", link.req_id),
            None => link.req_id.clone(),
        })
        .collect();
    let configured: BTreeSet<String> = merged(&root, &index).iter().flat_map(|b| b.clauses()).collect();
    let written = written(&root);

    let claimed_but_unconfigured: Vec<&String> =
        annotated.difference(&configured).collect();
    let configured_but_unclaimed: Vec<&String> =
        written.difference(&annotated).collect();

    assert!(
        claimed_but_unconfigured.is_empty(),
        "these clauses are annotated `@drt` with no binding behind them: {claimed_but_unconfigured:#?}"
    );
    assert!(
        configured_but_unclaimed.is_empty(),
        "these bindings exist with nothing claiming them: {configured_but_unclaimed:#?}"
    );
    assert!(!configured.is_empty(), "no bindings at all");
}

/// @tests REQ-DRT-RUST.params_from_source
#[test]
fn every_binding_resolves_against_the_source() {
    use tracelean_core::drt::rust_runner::resolve;

    let root = project_root();
    let index = build(&root);
    let claimed: BTreeSet<String> = index
        .links
        .iter()
        .filter(|link| link.role == Role::Drt)
        .map(|link| tracelean_core::drt::config::qualified_op(&link.req_id, link.clause.as_deref()))
        .collect();

    for binding in merged(&root, &index).into_iter().filter(|b| claimed.contains(&b.op())) {
        let resolved = resolve(&root, &binding)
            .unwrap_or_else(|e| panic!("binding {}: {e}", binding.op()));

        let declared = binding.model.as_ref().map(|m| m.arguments.len()).unwrap_or(0);
        assert_eq!(
            resolved.parameters.len(),
            declared,
            "binding {}: the model takes {declared} argument(s), the implementation takes {}",
            binding.op(),
            resolved.parameters.len()
        );
    }
}
