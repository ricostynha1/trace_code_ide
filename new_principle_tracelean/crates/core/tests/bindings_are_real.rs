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

const REACH: &str = r##"
open Lean Elab Command Term in
elab "#tracelean_reach " f:ident g:ident : command => liftTermElabM do
  let env ← getEnv
  let bound ← TraceLeanCheck.resolve f.getId
  let model ← TraceLeanCheck.resolve g.getId
  let found := env.contains bound && env.contains model
  IO.println s!"tracelean-reach {f.getId} {g.getId} {found && TraceLeanCheck.reaches env model [bound] {}}"
"##;

/// @tests REQ-DRT-BIND.binding_is_the_bond
#[test]
#[ignore = "runs Lean on the built models"]
fn every_binding_compares_the_code_with_a_model_that_computes_the_clauses_model() {
    use tracelean_core::trace::anchor::AnchorKind;
    let root = project_root();
    let index = build(&root);
    let mut pairs: BTreeSet<(String, String, String)> = BTreeSet::new();
    let mut imports: BTreeSet<String> = BTreeSet::new();
    for binding in merged(&root, &index) {
        let Some(model) = binding.model.as_ref().filter(|m| m.import.starts_with("TraceLean.")) else { continue };
        imports.insert(model.import.clone());
        for op in binding.clauses() {
            let (req_id, clause) = tracelean_core::drt::config::split_op(&op);
            for link in index.links.iter().filter(|l| l.role == Role::Models && l.req_id == req_id && l.clause.as_deref() == clause) {
                let (AnchorKind::Decl { symbol_path }, Some(module)) =
                    (&link.anchor.kind, link.anchor.file.strip_prefix("formal/").and_then(|f| f.strip_suffix(".lean")))
                else {
                    continue;
                };
                imports.insert(module.replace('/', "."));
                pairs.insert((op.clone(), model.function.clone(), symbol_path.replace("::", ".")));
            }
        }
    }
    let mut text = String::from("import Lean\n");
    for module in &imports {
        text.push_str(&format!("import {module}\n"));
    }
    text.push_str(tracelean_core::trace::proofcheck::PRELUDE);
    text.push_str(REACH);
    for (_, bound, model) in &pairs {
        text.push_str(&format!("#tracelean_reach {bound} {model}\n"));
    }
    let file = std::env::temp_dir().join(format!("tracelean-reach-{}.lean", std::process::id()));
    std::fs::write(&file, text).unwrap();
    let ran = std::process::Command::new("lake").args(["env", "lean"]).arg(&file).current_dir(root.join("formal")).output().expect("lake runs");
    let _ = std::fs::remove_file(&file);
    let said = String::from_utf8_lossy(&ran.stdout).to_string();
    let reaches: BTreeSet<(String, String)> = said
        .lines()
        .filter_map(|l| match l.split_whitespace().collect::<Vec<_>>().as_slice() {
            ["tracelean-reach", bound, model, "true"] => Some((bound.to_string(), model.to_string())),
            _ => None,
        })
        .collect();
    assert!(said.lines().filter(|l| l.starts_with("tracelean-reach ")).count() == pairs.len(), "Lean did not answer for every pair:\n{said}{}", String::from_utf8_lossy(&ran.stderr));
    let wrong: Vec<String> = pairs
        .iter()
        .filter(|(_, bound, model)| !reaches.contains(&(bound.clone(), model.clone())))
        .map(|(op, bound, model)| format!("{op}: bound to {bound}, which never computes {model}"))
        .collect();
    assert!(wrong.is_empty(), "these bindings compare the code with something other than the clause's model: {wrong:#?}");
}
