use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::evidence::Bond;
use crate::trace::index::Index;
use crate::trace::proofcheck::{self, ModelPlan, ProofReport};
use crate::trace::record::Key;
use crate::trace::{earn, store};

fn lake_root(root: &Path, file: &str) -> Option<PathBuf> {
    let mut at = root.join(file).parent().map(Path::to_path_buf);
    while let Some(dir) = at {
        if ["lakefile.lean", "lakefile.toml"].iter().any(|f| dir.join(f).is_file()) {
            return Some(dir);
        }
        if dir == root {
            return None;
        }
        at = dir.parent().map(Path::to_path_buf);
    }
    None
}

fn lake() -> PathBuf {
    let elan = std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".elan/bin/lake"));
    match Command::new("lake").arg("--version").output() {
        Ok(_) => PathBuf::from("lake"),
        Err(_) => elan.filter(|p| p.exists()).unwrap_or_else(|| PathBuf::from("lake")),
    }
}

pub fn path(root: &Path) -> PathBuf {
    root.join(".tracelean").join("proofs.json")
}

pub fn load(root: &Path) -> ProofReport {
    std::fs::read_to_string(path(root)).ok().and_then(|text| serde_json::from_str(&text).ok()).unwrap_or_default()
}

pub fn save(root: &Path, report: &ProofReport) -> std::io::Result<()> {
    if let Some(parent) = path(root).parent() {
        std::fs::create_dir_all(parent)?;
    }
    let mut text = serde_json::to_string_pretty(report).unwrap_or_default();
    text.push('\n');
    std::fs::write(path(root), text)
}

pub fn toolchain() -> String {
    Command::new("lean")
        .arg("--version")
        .output()
        .map(|out| String::from_utf8_lossy(&out.stdout).trim().to_string())
        .unwrap_or_default()
}

fn run_lean(package: Option<&Path>, text: String) -> Result<String, String> {
    let scratch = std::env::temp_dir().join(format!("tracelean-proofs-{}", std::process::id()));
    std::fs::create_dir_all(&scratch).map_err(|e| format!("cannot write the check: {e}"))?;
    let file = scratch.join("Check.lean");
    std::fs::write(&file, text).map_err(|e| format!("cannot write the check: {e}"))?;
    let ran = match package {
        Some(package) => Command::new(lake()).args(["env", "lean"]).arg(&file).current_dir(package).output(),
        None => Command::new("lean").arg(&file).output(),
    };
    let _ = std::fs::remove_dir_all(&scratch);
    let ran = ran.map_err(|e| format!("could not run Lean: {e}"))?;
    Ok(format!("{}{}", String::from_utf8_lossy(&ran.stdout), String::from_utf8_lossy(&ran.stderr)))
}

/// @implements REQ-STRENGTH.kernel_decides
pub fn check(root: &Path, index: &Index) -> Result<ProofReport, String> {
    let files = crate::observe::workspace::snapshot(root).files;
    let plans = proofcheck::with_sources(proofcheck::plans(proofcheck::links_of(index)), &files);
    let mut by_package: BTreeMap<PathBuf, Vec<ModelPlan>> = BTreeMap::new();
    let mut alone: BTreeMap<String, Vec<ModelPlan>> = BTreeMap::new();
    for plan in &plans {
        match lake_root(root, &plan.file) {
            Some(package) => by_package.entry(package).or_default().push(plan.clone()),
            None => alone.entry(plan.file.clone()).or_default().push(plan.clone()),
        }
    }
    let mut heard = Vec::new();
    for (file, plans) in &alone {
        let source = std::fs::read_to_string(root.join(file)).map_err(|e| format!("cannot read {file}: {e}"))?;
        let facts = proofcheck::heard(run_lean(
            None,
            proofcheck::standalone(source.clone(), proofcheck::facts_source(Vec::new(), plans.clone())),
        )?);
        let accepted = proofcheck::accepted(facts.clone());
        let obligations = proofcheck::heard(run_lean(
            None,
            proofcheck::standalone(source, proofcheck::obligation_source(Vec::new(), plans.clone(), accepted)),
        )?);
        heard.extend(facts);
        heard.extend(obligations);
    }
    for (package, plans) in &by_package {
        let prefix = package.strip_prefix(root).unwrap_or(package).display().to_string();
        let mut modules = BTreeSet::new();
        for plan in plans {
            let files = std::iter::once(&plan.file).chain(plan.theorems.iter().chain(&plan.pins).map(|c| &c.file));
            for file in files {
                if let Some(module) = proofcheck::module_of(file, &prefix) {
                    modules.insert(module);
                }
            }
        }
        let modules: Vec<String> = modules.into_iter().collect();
        let built = Command::new(lake()).arg("build").current_dir(package).output();
        if let Ok(built) = &built {
            if !built.status.success() {
                return Err(format!(
                    "the models in {prefix} do not build:\n{}{}",
                    String::from_utf8_lossy(&built.stdout),
                    String::from_utf8_lossy(&built.stderr)
                ));
            }
        }
        let facts = proofcheck::heard(run_lean(Some(package), proofcheck::facts_source(modules.clone(), plans.clone()))?);
        let accepted = proofcheck::accepted(facts.clone());
        let obligations =
            proofcheck::heard(run_lean(Some(package), proofcheck::obligation_source(modules, plans.clone(), accepted))?);
        heard.extend(facts);
        heard.extend(obligations);
    }
    Ok(proofcheck::report(plans, heard, proofcheck::rules_hash()))
}

pub fn earn_all(root: &Path, index: &Index, report: &ProofReport, toolchain: &str) -> (usize, usize) {
    let mut by_clause: BTreeMap<(String, Option<String>), Vec<&proofcheck::ProofEntry>> = BTreeMap::new();
    for entry in &report.proofs {
        by_clause.entry((entry.req_id.clone(), entry.clause.clone())).or_default().push(entry);
    }
    let (mut written, mut removed) = (0, 0);
    for ((req_id, clause), entries) in by_clause {
        let accepted = entries.iter().find(|e| e.verdict.is_accepted());
        let earned = accepted.and_then(|e| {
            earn::proof_record(index, &req_id, clause.as_deref(), &proofcheck::lean_name(&e.theorem), toolchain).ok()
        });
        match earned {
            Some(record) if store::write(root, &record).is_ok() => written += 1,
            _ => {
                let key = Key { req_id: req_id.clone(), clause: clause.clone(), bond: Bond::ModelProof };
                if store::remove_slot(root, &key).is_ok() {
                    removed += 1;
                }
            }
        }
    }
    (written, removed)
}
