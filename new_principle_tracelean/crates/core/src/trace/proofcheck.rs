use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

use super::annotation::Role;
use super::strength::Strength;

/// @implements REQ-STRENGTH.kernel_decides
pub const PRELUDE: &str = include_str!("proofcheck.lean");

pub fn rules_hash() -> String {
    super::hash::text(PRELUDE)
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", tag = "reason", content = "via")]
pub enum Refusal {
    Sorry,
    Axiom(String),
    Silent,
    Example,
    Indirect(String),
    Vacuous,
    Type,
    Missing,
    Statement,
    Unchecked,
}

impl Refusal {
    /// @implements REQ-STRENGTH.refusal_reported
    pub fn message(&self) -> String {
        match self {
            Refusal::Sorry => "finish the proof: it depends on sorry".into(),
            Refusal::Axiom(name) => format!(
                "remove the dependency on axiom `{name}`, or declare it with @assumes in its doc comment if it stands for something outside Lean"
            ),
            Refusal::Silent => "restate it about the clause's model: its statement never applies the model".into(),
            Refusal::Example => {
                "state it for every input: the model is only applied to fixed values, so it is an example".into()
            }
            Refusal::Indirect(via) => format!(
                "restate it about the model directly: it reaches the model only through `{via}`, whose behaviour it then fixes"
            ),
            Refusal::Vacuous => "strengthen it: it holds of every function in place of the model".into(),
            Refusal::Type => "model the clause with a function: a type can be judged, not proved".into(),
            Refusal::Missing => "build it: Lean does not know this declaration".into(),
            Refusal::Statement => "state exactly the generated obligation (tracelean-trace . --pins)".into(),
            Refusal::Unchecked => "check it: Lean gave no verdict (tracelean-trace . --proofs)".into(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", tag = "verdict")]
pub enum Verdict {
    Accepted { assumed: Vec<String> },
    Refused { refusal: Refusal },
}

impl Verdict {
    pub fn is_accepted(&self) -> bool {
        matches!(self, Verdict::Accepted { .. })
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CheckLink {
    pub role: Role,
    pub req_id: String,
    pub clause: Option<String>,
    pub symbol: String,
    pub file: String,
    pub line: u32,
    pub body_hash: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Claimed {
    pub theorem: String,
    pub req_id: String,
    pub clause: Option<String>,
    pub file: String,
    pub line: u32,
    pub body_hash: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelPlan {
    pub model: String,
    pub file: String,
    pub body_hash: String,
    pub clauses: Vec<(String, Option<String>)>,
    pub theorems: Vec<Claimed>,
    pub pins: Vec<Claimed>,
    #[serde(default)]
    pub sources: String,
}

pub fn package_of(file: &str, files: &BTreeMap<String, String>) -> Option<String> {
    let mut dir = file;
    while let Some((parent, _)) = dir.rsplit_once('/') {
        if ["lakefile.lean", "lakefile.toml"].iter().any(|f| files.contains_key(&format!("{parent}/{f}"))) {
            return Some(parent.to_string());
        }
        dir = parent;
    }
    ["lakefile.lean", "lakefile.toml"].iter().any(|f| files.contains_key(*f)).then(String::new)
}

/// @implements REQ-STRENGTH.verdict_kept
pub fn with_sources(plans: Vec<ModelPlan>, files: &BTreeMap<String, String>) -> Vec<ModelPlan> {
    plans
        .into_iter()
        .map(|mut plan| {
            let package = package_of(&plan.file, files);
            let mut text = String::new();
            for (path, content) in files {
                let inside = match &package {
                    Some(dir) if dir.is_empty() => path.ends_with(".lean"),
                    Some(dir) => path.ends_with(".lean") && path.starts_with(&format!("{dir}/")),
                    None => *path == plan.file,
                };
                if inside {
                    text.push_str(path);
                    text.push('\0');
                    text.push_str(content);
                    text.push('\0');
                }
            }
            plan.sources = super::hash::text(&text);
            plan
        })
        .collect()
}

pub fn lean_name(symbol: &str) -> String {
    symbol.replace("::", ".")
}

/// @implements REQ-STRENGTH.obligation_generated
/// @implements REQ-STRENGTH.strength_per_model
/// @implements ARCH-DETERMINISM.stable_ordering
/// @drt REQ-STRENGTH.strength_per_model
pub fn plans(links: Vec<CheckLink>) -> Vec<ModelPlan> {
    let mut model_of: BTreeMap<(String, Option<String>), (String, String, String)> = BTreeMap::new();
    for link in links.iter().filter(|l| l.role == Role::Models && l.file.ends_with(".lean")) {
        model_of
            .entry((link.req_id.clone(), link.clause.clone()))
            .or_insert((link.symbol.clone(), link.file.clone(), link.body_hash.clone()));
    }
    let mut out: BTreeMap<String, ModelPlan> = BTreeMap::new();
    for (clause, (model, file, body_hash)) in &model_of {
        let plan = out.entry(model.clone()).or_insert_with(|| ModelPlan {
            model: model.clone(),
            file: file.clone(),
            body_hash: body_hash.clone(),
            ..Default::default()
        });
        plan.clauses.push(clause.clone());
    }
    for link in links.iter().filter(|l| matches!(l.role, Role::Proves | Role::Pins)) {
        let Some((model, _, _)) = model_of.get(&(link.req_id.clone(), link.clause.clone())) else { continue };
        let Some(plan) = out.get_mut(model) else { continue };
        let claimed = Claimed {
            theorem: link.symbol.clone(),
            req_id: link.req_id.clone(),
            clause: link.clause.clone(),
            file: link.file.clone(),
            line: link.line,
            body_hash: link.body_hash.clone(),
        };
        let into = if link.role == Role::Proves { &mut plan.theorems } else { &mut plan.pins };
        if !into.contains(&claimed) {
            into.push(claimed);
        }
    }
    for plan in out.values_mut() {
        plan.theorems.sort_by(|a, b| (&a.theorem, &a.req_id, &a.clause).cmp(&(&b.theorem, &b.req_id, &b.clause)));
        plan.pins.sort_by(|a, b| (&a.theorem, &a.req_id, &a.clause).cmp(&(&b.theorem, &b.req_id, &b.clause)));
    }
    out.into_values().collect()
}

pub fn module_of(file: &str, package: &str) -> Option<String> {
    let rel = file.strip_prefix(package)?.trim_start_matches('/');
    let stem = rel.strip_suffix(".lean")?;
    Some(stem.replace('/', "."))
}

fn header(imports: &[String]) -> String {
    let mut out = String::from("import Lean\n");
    for module in imports {
        out.push_str(&format!("import {module}\n"));
    }
    out.push_str("\nset_option maxHeartbeats 1000000\nset_option maxRecDepth 4096\n\n");
    out.push_str(PRELUDE);
    out.push('\n');
    out
}

pub fn standalone(model_source: String, check: String) -> String {
    let (imports, body): (Vec<&str>, Vec<&str>) =
        model_source.lines().partition(|line| line.trim_start().starts_with("import "));
    let (head, rest) = check.split_once("\nset_option").unwrap_or((&check, ""));
    let mut out = String::from(head);
    out.push('\n');
    for line in imports {
        out.push_str(line);
        out.push('\n');
    }
    out.push('\n');
    out.push_str(&body.join("\n"));
    out.push_str("\n\nset_option");
    out.push_str(rest);
    out
}

fn names(claimed: &[Claimed]) -> String {
    let mut names: Vec<String> = claimed.iter().map(|c| lean_name(&c.theorem)).collect();
    names.dedup();
    names.join(", ")
}

/// @implements REQ-STRENGTH.kernel_decides
pub fn facts_source(imports: Vec<String>, plans: Vec<ModelPlan>) -> String {
    let mut out = header(&imports);
    for plan in &plans {
        out.push_str(&format!("#tracelean_facts {} [{}]\n", lean_name(&plan.model), names(&plan.theorems)));
    }
    out
}

/// @implements REQ-STRENGTH.obligation_generated
/// @implements REQ-STRENGTH.not_proved_by_us
pub fn obligation_source(imports: Vec<String>, plans: Vec<ModelPlan>, statements: Vec<(String, String)>) -> String {
    let mut out = header(&imports);
    for plan in &plans {
        let model = lean_name(&plan.model);
        let mut kept: Vec<String> = statements.iter().filter(|(m, _)| *m == model).map(|(_, t)| t.clone()).collect();
        kept.dedup();
        out.push_str(&format!("#tracelean_obligation {model} [{}] [{}]\n", kept.join(", "), names(&plan.pins)));
    }
    out
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProofFacts {
    pub found: bool,
    pub is_type: bool,
    pub mentions: bool,
    pub general: bool,
    pub leak: Option<String>,
    pub ignores: bool,
    pub closes: bool,
    pub axioms: Vec<(String, bool)>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PinFacts {
    pub found: bool,
    pub statement: bool,
    pub axioms: Vec<(String, bool)>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", tag = "heard")]
pub enum Heard {
    Facts { theorem: String, model: String, facts: ProofFacts },
    Obligation { model: String, statement: String },
    PinFacts { theorem: String, model: String, facts: PinFacts },
    Missing { model: String },
}

pub const LEAN_OWN: &[&str] = &["propext", "Classical.choice", "Quot.sound"];

/// @implements REQ-STRENGTH.axioms_declared
/// @implements REQ-STRENGTH.assumptions_shown
fn axiom_verdict(axioms: &[(String, bool)]) -> Result<Vec<String>, Refusal> {
    let mut assumed = Vec::new();
    for (name, declared) in axioms {
        if name == "sorryAx" {
            return Err(Refusal::Sorry);
        }
        if LEAN_OWN.contains(&name.as_str()) {
            continue;
        }
        if !declared {
            return Err(Refusal::Axiom(name.clone()));
        }
        assumed.push(name.clone());
    }
    assumed.sort();
    Ok(assumed)
}

/// @implements REQ-STRENGTH.proof_states_the_model
/// @implements REQ-STRENGTH.proof_is_general
/// @implements REQ-STRENGTH.proof_is_direct
/// @implements REQ-STRENGTH.proof_not_vacuous
/// @implements REQ-STRENGTH.model_is_a_function
/// @implements REQ-STRENGTH.axioms_declared
/// @implements REQ-STRENGTH.assumptions_shown
/// @drt REQ-STRENGTH.proof_states_the_model
/// @drt REQ-STRENGTH.proof_is_general
/// @drt REQ-STRENGTH.proof_is_direct
/// @drt REQ-STRENGTH.proof_not_vacuous
/// @drt REQ-STRENGTH.model_is_a_function
/// @drt REQ-STRENGTH.axioms_declared
/// @drt REQ-STRENGTH.assumptions_shown
pub fn verdict(facts: ProofFacts) -> Verdict {
    let refused = |refusal| Verdict::Refused { refusal };
    if !facts.found {
        return refused(Refusal::Missing);
    }
    if facts.is_type {
        return refused(Refusal::Type);
    }
    let assumed = match axiom_verdict(&facts.axioms) {
        Ok(assumed) => assumed,
        Err(refusal) => return refused(refusal),
    };
    if !facts.mentions {
        return refused(Refusal::Silent);
    }
    if !facts.general {
        return refused(Refusal::Example);
    }
    if let Some(via) = facts.leak {
        return refused(Refusal::Indirect(via));
    }
    if facts.ignores || facts.closes {
        return refused(Refusal::Vacuous);
    }
    Verdict::Accepted { assumed }
}

/// @implements REQ-STRENGTH.pinned_by_theorem
/// @drt REQ-STRENGTH.pinned_by_theorem
pub fn pin_verdict(facts: PinFacts) -> Verdict {
    let refused = |refusal| Verdict::Refused { refusal };
    if !facts.found {
        return refused(Refusal::Missing);
    }
    match axiom_verdict(&facts.axioms) {
        Err(refusal) => refused(refusal),
        Ok(_) if !facts.statement => refused(Refusal::Statement),
        Ok(assumed) => Verdict::Accepted { assumed },
    }
}

fn field<'a>(words: &[&'a str], key: &str) -> Option<&'a str> {
    words.iter().find_map(|w| w.strip_prefix(key).and_then(|rest| rest.strip_prefix('=')))
}

fn flag(words: &[&str], key: &str) -> bool {
    field(words, key) == Some("true")
}

fn axioms_of(words: &[&str]) -> Vec<(String, bool)> {
    match field(words, "axioms") {
        None | Some("-") => Vec::new(),
        Some(list) => list
            .split(',')
            .filter(|a| !a.is_empty())
            .map(|a| match a.rsplit_once(':') {
                Some((name, tag)) => (name.to_string(), tag == "assumed"),
                None => (a.to_string(), false),
            })
            .collect(),
    }
}

/// @implements REQ-STRENGTH.kernel_decides
pub fn heard(output: String) -> Vec<Heard> {
    let mut out = Vec::new();
    let mut lines = output.lines();
    while let Some(line) = lines.next() {
        let words: Vec<&str> = line.split_whitespace().collect();
        match words.as_slice() {
            ["tracelean-facts", theorem, model, rest @ ..] => out.push(Heard::Facts {
                theorem: theorem.to_string(),
                model: model.to_string(),
                facts: ProofFacts {
                    found: flag(rest, "found"),
                    is_type: flag(rest, "type"),
                    mentions: flag(rest, "mentions"),
                    general: flag(rest, "general"),
                    leak: field(rest, "leak").filter(|l| *l != "-").map(str::to_string),
                    ignores: flag(rest, "ignores"),
                    closes: flag(rest, "closes"),
                    axioms: axioms_of(rest),
                },
            }),
            ["tracelean-pinfacts", theorem, model, rest @ ..] => out.push(Heard::PinFacts {
                theorem: theorem.to_string(),
                model: model.to_string(),
                facts: PinFacts { found: flag(rest, "found"), statement: flag(rest, "statement"), axioms: axioms_of(rest) },
            }),
            ["tracelean-model", model, "missing"] => out.push(Heard::Missing { model: model.to_string() }),
            ["tracelean-obligation", model] => {
                let mut statement = Vec::new();
                for next in lines.by_ref() {
                    if next.trim() == "tracelean-end" {
                        break;
                    }
                    statement.push(next);
                }
                out.push(Heard::Obligation { model: model.to_string(), statement: statement.join("\n") });
            }
            _ => {}
        }
    }
    out
}

/// @implements REQ-STRENGTH.obligation_generated
/// @drt REQ-STRENGTH.obligation_generated
pub fn accepted(heard: Vec<Heard>) -> Vec<(String, String)> {
    heard
        .into_iter()
        .filter_map(|h| match h {
            Heard::Facts { theorem, model, facts } if verdict(facts.clone()).is_accepted() => Some((model, theorem)),
            _ => None,
        })
        .collect()
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProofEntry {
    pub theorem: String,
    pub model: String,
    pub req_id: String,
    pub clause: Option<String>,
    pub file: String,
    pub line: u32,
    pub verdict: Verdict,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PinEntry {
    pub theorem: String,
    pub req_id: String,
    pub clause: Option<String>,
    pub verdict: Verdict,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelEntry {
    pub model: String,
    pub clauses: Vec<(String, Option<String>)>,
    pub statement: String,
    pub key: String,
    pub pins: Vec<PinEntry>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProofReport {
    pub rules: String,
    pub proofs: Vec<ProofEntry>,
    pub models: Vec<ModelEntry>,
}

/// @implements REQ-STRENGTH.verdict_kept
pub fn key(plan: ModelPlan, accepted: Vec<String>) -> String {
    let mut parts: Vec<String> = plan
        .theorems
        .iter()
        .filter(|t| accepted.contains(&lean_name(&t.theorem)))
        .map(|t| t.body_hash.clone())
        .collect();
    parts.sort();
    parts.dedup();
    parts.insert(0, plan.body_hash.clone());
    parts.insert(0, plan.sources.clone());
    parts.join("+")
}

/// @implements REQ-STRENGTH.refusal_reported
/// @implements REQ-STRENGTH.assumptions_shown
/// @implements ARCH-DETERMINISM.stable_ordering
pub fn report(plans: Vec<ModelPlan>, heard: Vec<Heard>, rules: String) -> ProofReport {
    let mut proofs_heard: BTreeMap<(String, String), Verdict> = BTreeMap::new();
    let mut pins_heard: BTreeMap<(String, String), Verdict> = BTreeMap::new();
    let mut statements: BTreeMap<String, String> = BTreeMap::new();
    let mut missing: BTreeSet<String> = BTreeSet::new();
    for item in heard {
        match item {
            Heard::Facts { theorem, model, facts } => {
                proofs_heard.entry((theorem, model)).or_insert_with(|| verdict(facts));
            }
            Heard::PinFacts { theorem, model, facts } => {
                pins_heard.entry((theorem, model)).or_insert_with(|| pin_verdict(facts));
            }
            Heard::Obligation { model, statement } => {
                statements.entry(model).or_insert(statement);
            }
            Heard::Missing { model } => {
                missing.insert(model);
            }
        }
    }
    let unchecked = || Verdict::Refused { refusal: Refusal::Unchecked };
    let mut out = ProofReport { rules, ..Default::default() };
    for plan in plans {
        let name = lean_name(&plan.model);
        let gone = missing.contains(&name);
        let mut accepted = Vec::new();
        for claimed in &plan.theorems {
            let verdict = if gone {
                Verdict::Refused { refusal: Refusal::Missing }
            } else {
                proofs_heard.get(&(lean_name(&claimed.theorem), name.clone())).cloned().unwrap_or_else(unchecked)
            };
            if verdict.is_accepted() {
                accepted.push(lean_name(&claimed.theorem));
            }
            out.proofs.push(ProofEntry {
                theorem: claimed.theorem.clone(),
                model: plan.model.clone(),
                req_id: claimed.req_id.clone(),
                clause: claimed.clause.clone(),
                file: claimed.file.clone(),
                line: claimed.line,
                verdict,
            });
        }
        let pins = plan
            .pins
            .iter()
            .map(|claimed| PinEntry {
                theorem: claimed.theorem.clone(),
                req_id: claimed.req_id.clone(),
                clause: claimed.clause.clone(),
                verdict: pins_heard
                    .get(&(lean_name(&claimed.theorem), name.clone()))
                    .cloned()
                    .unwrap_or_else(unchecked),
            })
            .collect();
        out.models.push(ModelEntry {
            model: plan.model.clone(),
            clauses: plan.clauses.clone(),
            statement: statements.get(&name).cloned().unwrap_or_default(),
            key: key(plan, accepted),
            pins,
        });
    }
    out.proofs.sort_by(|a, b| (&a.req_id, &a.clause, &a.theorem).cmp(&(&b.req_id, &b.clause, &b.theorem)));
    out
}

/// @implements REQ-STRENGTH.open_is_the_default
/// @implements REQ-STRENGTH.attempted_distinguished
/// @implements REQ-STRENGTH.nondeterministic_declared
/// @implements REQ-STRENGTH.verdict_kept
/// @drt REQ-STRENGTH.qualifies_proof
/// @drt REQ-STRENGTH.open_is_the_default
/// @drt REQ-STRENGTH.attempted_distinguished
/// @drt REQ-STRENGTH.nondeterministic_declared
/// @drt REQ-STRENGTH.verdict_kept
pub fn standing(
    held: Option<(String, Vec<String>)>,
    key_now: String,
    pins_now: Vec<String>,
    nondeterministic: Option<String>,
) -> Strength {
    if let Some(reason) = nondeterministic {
        return Strength::Nondeterministic { reason };
    }
    let pinned = held
        .filter(|(key, _)| *key == key_now)
        .and_then(|(_, accepted)| accepted.into_iter().find(|p| pins_now.contains(p)));
    match (pinned, pins_now.first()) {
        (Some(theorem_name), _) => Strength::Pinned { theorem_name },
        (None, Some(theorem_name)) => Strength::Attempted { theorem_name: theorem_name.clone() },
        (None, None) => Strength::Open,
    }
}

pub fn links_of(index: &super::index::Index) -> Vec<CheckLink> {
    use super::anchor::AnchorKind;
    index
        .links
        .iter()
        .filter(|l| matches!(l.role, Role::Models | Role::Proves | Role::Pins))
        .filter_map(|l| match &l.anchor.kind {
            AnchorKind::Decl { symbol_path } => Some(CheckLink {
                role: l.role,
                req_id: l.req_id.clone(),
                clause: l.clause.clone(),
                symbol: symbol_path.clone(),
                file: l.anchor.file.clone(),
                line: l.line,
                body_hash: l.anchor.body_hash.clone(),
            }),
            _ => None,
        })
        .collect()
}

/// @implements REQ-STRENGTH.qualifies_proof
pub fn described(shown: &Shown) -> (String, String) {
    let detail = match &shown.strength {
        Strength::Pinned { theorem_name } => theorem_name.clone(),
        Strength::Attempted { theorem_name } => format!("{theorem_name}: {}", shown.said),
        Strength::Nondeterministic { reason } => reason.clone(),
        Strength::Open => format!("owes a theorem annotated @pins for {}: tracelean-trace . --pins", shown.model),
    };
    (shown.strength.as_str().to_string(), detail)
}

pub struct Shown {
    pub strength: Strength,
    pub statement: String,
    pub model: String,
    pub said: String,
}

/// @implements REQ-STRENGTH.strength_per_model
/// @implements REQ-STRENGTH.qualifies_proof
pub fn strengths(
    index: &super::index::Index,
    report: &ProofReport,
    files: &BTreeMap<String, String>,
) -> BTreeMap<(String, Option<String>), Shown> {
    let nondeterministic: BTreeMap<(String, Option<String>), String> = index
        .links
        .iter()
        .filter(|l| l.role == Role::Models)
        .filter_map(|l| match &l.qualifier {
            Some(super::annotation::Qualifier::Nondeterministic { reason }) => Some((
                (l.req_id.clone(), l.clause.clone()),
                reason.clone().unwrap_or_else(|| "no reason given".into()),
            )),
            _ => None,
        })
        .collect();
    let mut out = BTreeMap::new();
    for plan in with_sources(plans(links_of(index)), files) {
        let entry = report.models.iter().find(|m| m.model == plan.model).cloned();
        let accepted: Vec<String> = report
            .proofs
            .iter()
            .filter(|p| p.model == plan.model && p.verdict.is_accepted())
            .map(|p| lean_name(&p.theorem))
            .collect();
        let pins_now: Vec<String> = plan.pins.iter().map(|p| p.theorem.clone()).collect();
        let declared = plan.clauses.iter().find_map(|c| nondeterministic.get(c).cloned());
        let key_now = key(plan.clone(), accepted);
        let held = entry.as_ref().map(|e| {
            (e.key.clone(), e.pins.iter().filter(|p| p.verdict.is_accepted()).map(|p| p.theorem.clone()).collect())
        });
        let strength = standing(held, key_now.clone(), pins_now, declared);
        let said = match (&strength, &entry) {
            (Strength::Attempted { theorem_name }, Some(e)) if e.key == key_now => e
                .pins
                .iter()
                .find(|p| &p.theorem == theorem_name)
                .map(|p| match &p.verdict {
                    Verdict::Refused { refusal } => refusal.message(),
                    Verdict::Accepted { .. } => String::new(),
                })
                .unwrap_or_else(|| Refusal::Unchecked.message()),
            _ => "changed since it was checked: tracelean-trace . --proofs".into(),
        };
        for clause in &plan.clauses {
            out.insert(
                clause.clone(),
                Shown {
                    strength: strength.clone(),
                    statement: entry.as_ref().map(|e| e.statement.clone()).unwrap_or_default(),
                    model: plan.model.clone(),
                    said: said.clone(),
                },
            );
        }
    }
    out
}
