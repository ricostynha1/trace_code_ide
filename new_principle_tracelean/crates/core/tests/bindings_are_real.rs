//! The binding file and the annotations must describe the same set of bonds.
//!
//! A `@drt` annotation claims a clause is differentially tested; a binding in
//! `.tracelean/drt.json` is what makes that true. If either can exist without
//! the other, the claim and the mechanism have come apart — which is the exact
//! failure `REQ-CHECK.unbound_reported` exists to catch, applied to this
//! project's own configuration.
//!
//! @tests REQ-DRT-BIND.binding_is_the_bond
//! @tests ARCH-SELFHOST.every_feature_traced
//! @structural ARCH-SELFHOST.every_feature_traced reason="a claim about this project's own annotations and binding file agreeing"

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use tracelean_core::trace::annotation::Role;
use tracelean_core::trace::index::build;

fn project_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap().parent().unwrap().to_path_buf()
}

fn bindings_in_config(root: &Path) -> BTreeSet<String> {
    let text = std::fs::read_to_string(root.join(".tracelean/drt.json")).expect("drt.json");
    let value: serde_json::Value = serde_json::from_str(&text).expect("readable JSON");
    value["bindings"]
        .as_array()
        .expect("a bindings array")
        .iter()
        .flat_map(|binding| {
            let req = binding["req_id"].as_str().unwrap_or_default().to_string();
            let mut names = vec![match binding["clause"].as_str() {
                Some(clause) => format!("{req}.{clause}"),
                None => req.clone(),
            }];
            // A binding may name other clauses the same call checks. They are
            // as much a claim as the primary one, and are held to the same
            // rule: annotated, or not claimed.
            if let Some(also) = binding["also_checks"].as_array() {
                names.extend(also.iter().filter_map(|c| c.as_str()).map(|clause| {
                    // Qualified names reach clauses of other requirements; bare
                    // ones stay within this binding's own.
                    if clause.contains('.') {
                        clause.to_string()
                    } else {
                        format!("{req}.{clause}")
                    }
                }));
            }
            names
        })
        .collect()
}

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
    let configured = bindings_in_config(&root);

    let claimed_but_unconfigured: Vec<&String> =
        annotated.difference(&configured).collect();
    let configured_but_unclaimed: Vec<&String> =
        configured.difference(&annotated).collect();

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

/// Every binding must name an implementation that exists and can be resolved.
///
/// @tests REQ-DRT-RUST.params_from_source
#[test]
fn every_binding_resolves_against_the_source() {
    use std::collections::BTreeMap;
    use tracelean_core::drt::rust_runner::resolve;
    use tracelean_core::drt::{Binding, CallSpec};

    let root = project_root();
    let text = std::fs::read_to_string(root.join(".tracelean/drt.json")).expect("drt.json");
    let value: serde_json::Value = serde_json::from_str(&text).expect("readable JSON");

    for entry in value["bindings"].as_array().expect("bindings") {
        let implementation = &entry["implementation"];
        let params: BTreeMap<String, String> = implementation
            .get("params")
            .and_then(|p| p.as_object())
            .map(|map| {
                map.iter()
                    .map(|(k, v)| (k.clone(), v.as_str().unwrap_or_default().to_string()))
                    .collect()
            })
            .unwrap_or_default();

        let binding = Binding {
            req_id: entry["req_id"].as_str().unwrap_or_default().to_string(),
            clause: entry["clause"].as_str().map(str::to_string),
            op: None,
            also_checks: Vec::new(),
            also_implemented_by: Vec::new(),
            floors: Vec::new(),
            waive: Vec::new(),
            model: None,
            implementation: CallSpec {
                language: implementation["language"].as_str().unwrap_or_default().to_string(),
                entry: implementation["entry"].as_str().unwrap_or_default().to_string(),
                params,
            },
        };

        let resolved = resolve(&root, &binding)
            .unwrap_or_else(|e| panic!("binding {}: {e}", binding.op()));

        // The model declares the arguments it takes; the implementation must
        // take the same number, or the call cannot be the same call.
        let declared = entry["model"]["arguments"].as_array().map(|a| a.len()).unwrap_or(0);
        assert_eq!(
            resolved.parameters.len(),
            declared,
            "binding {}: the model takes {declared} argument(s), the implementation takes {}",
            binding.op(),
            resolved.parameters.len()
        );
    }
}
