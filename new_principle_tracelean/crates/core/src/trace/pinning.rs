//! Whether a clause's specification pins its model, as the Lean kernel says.
//!
//! A clause may be modelled twice: by a **specification**, a predicate saying
//! which answers are right (`def ToFahrenheit (c f : Int) : Prop`), and by a
//! **model**, the function the code is tested against. The specification pins
//! the model when the model meets it and no input has two right answers:
//!
//! ```text
//! (∀ x, P x (f x)) ∧ (∀ x y1 y2, P x y1 → P x y2 → y1 = y2)
//! ```
//!
//! A person proves that in a theorem annotated `@pins`; TraceLean never does
//! (`REQ-STRENGTH.not_proved_by_us`). What it does is check the theorem: append
//! to its file an `example` stating exactly the obligation, proved by the
//! theorem, and ask for its axioms. Lean accepting both, without `sorryAx`, is
//! the only way a clause becomes pinned. The verdict is kept under
//! `.tracelean/pins`, keyed by the hashes of the three declarations, so an edit
//! to any of them puts the clause back to *attempted*.

use serde::{Deserialize, Serialize};

use super::annotation::Role;
use super::index::Index;
use super::strength::Strength;

/// What a pinning check needs to know about one clause.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PinPlan {
    pub req_id: String,
    pub clause: Option<String>,
    /// The Prop-valued `@models` declaration, as Lean names it.
    pub spec: Option<String>,
    /// The other `@models` declaration: the function the code is tested against.
    pub model: Option<String>,
    /// How many explicit arguments the model takes.
    pub inputs: usize,
    /// The `@pins` theorem, as Lean names it.
    pub theorem: Option<String>,
    /// The file the theorem is in, where the check is appended.
    pub file: Option<String>,
    /// The three declarations' body hashes joined: what a verdict is about.
    pub key: String,
}

/// A verdict, as `.tracelean/pins` keeps it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PinRecord {
    pub theorem_name: String,
    pub key: String,
    pub pinned: bool,
    /// What Lean said, when it did not accept the theorem.
    #[serde(default)]
    pub said: String,
}

/// The obligation, in Lean, for a specification `spec` and a model `model` of
/// `inputs` arguments.
///
/// @implements REQ-STRENGTH.per_input
/// @drt REQ-STRENGTH.per_input
pub fn statement(spec: String, model: String, inputs: usize) -> String {
    let args: String = (1..=inputs).map(|n| format!(" x{n}")).collect();
    let meets = if inputs == 0 {
        format!("{spec} {model}")
    } else {
        format!("∀{args}, {spec}{args} ({model}{args})")
    };
    format!("({meets}) ∧ (∀{args} y1 y2, {spec}{args} y1 → {spec}{args} y2 → y1 = y2)")
}

/// What is appended to the theorem's file to check it.
///
/// With a specification, an `example` whose type is the obligation, so a
/// theorem stating anything weaker does not elaborate; without one, the
/// theorem is only asked to be finished. Either way `#print axioms` says
/// whether it leans on `sorry`.
pub fn check_source(plan: &PinPlan) -> Option<String> {
    let theorem = plan.theorem.as_ref()?;
    let mut out = format!("\n\n-- TraceLean: is `{theorem}` the pinning obligation, and is it finished?\n");
    if let (Some(spec), Some(model)) = (&plan.spec, &plan.model) {
        out.push_str(&format!("example : {} := {theorem}\n", statement(spec.clone(), model.clone(), plan.inputs)));
    }
    out.push_str(&format!("#print axioms {theorem}\n"));
    Some(out)
}

/// Whether Lean accepted the check: it exited cleanly, reported no error, and
/// listed the theorem's axioms without `sorryAx`.
///
/// @implements REQ-STRENGTH.kernel_decides
/// @drt REQ-STRENGTH.kernel_decides
pub fn accepted(theorem_name: String, output: String, exited_ok: bool) -> bool {
    let named = format!("'{theorem_name}'");
    exited_ok
        && !output.contains(": error")
        && output.lines().any(|line| {
            line.starts_with(&named)
                && (line.contains("does not depend on any axioms")
                    || (line.contains("depends on axioms") && !line.contains("sorryAx")))
        })
}

/// Where a clause stands: open without a `@pins` theorem, pinned only when the
/// kernel accepted that theorem as it now is, attempted otherwise.
///
/// @implements REQ-STRENGTH.verdict_kept
/// @implements REQ-STRENGTH.open_is_the_default
/// @drt REQ-STRENGTH.verdict_kept
pub fn standing(theorem_name: Option<String>, record: Option<PinRecord>, key: String) -> Strength {
    match (theorem_name, record) {
        (None, _) => Strength::Open,
        (Some(t), Some(r)) if r.pinned && r.theorem_name == t && r.key == key => Strength::Pinned { theorem_name: t },
        (Some(t), _) => Strength::Attempted { theorem_name: t },
    }
}

/// A clause's standing as a person reads it: the state, and the theorem, why
/// it is not yet pinned, or what it owes.
pub fn shown(plan: &PinPlan, record: Option<PinRecord>) -> (String, String) {
    let failed = record.as_ref().filter(|r| !r.pinned && r.key == plan.key).map(|r| r.said.clone());
    match standing(plan.theorem.clone(), record, plan.key.clone()) {
        Strength::Pinned { theorem_name } => ("pinned".into(), theorem_name),
        Strength::Attempted { theorem_name } => {
            let why = match failed.as_deref().and_then(|said| said.lines().find(|l| l.contains("error") || l.contains("sorry"))) {
                Some(line) => format!("Lean: {}", line.trim()),
                None => "not yet checked: tracelean-trace . --pins".into(),
            };
            ("attempted".into(), format!("{theorem_name}  {why}"))
        }
        _ => match (&plan.spec, &plan.model) {
            (Some(spec), Some(model)) => {
                ("open".into(), format!("owes a theorem annotated @pins: {}", statement(spec.clone(), model.clone(), plan.inputs)))
            }
            _ => ("open".into(), "no specification: model it with a `def … : Prop` beside the function".into()),
        },
    }
}

/// A declaration's shape, from its source: whether it is a proposition, and how
/// many explicit arguments it takes. `None` for what is not a `def`.
pub fn shape(declaration: &str) -> Option<(bool, usize)> {
    let at = declaration.find("def ").or_else(|| declaration.find("abbrev "))?;
    let header = &declaration[at..];
    let header = &header[..header.find(":=").unwrap_or(header.len())];
    let (mut depth, mut names, mut group, mut result) = (0usize, 0usize, String::new(), None);
    let mut explicit = false;
    for (i, c) in header.char_indices() {
        match c {
            '(' | '{' | '[' => {
                depth += 1;
                if depth == 1 {
                    group.clear();
                    explicit = c == '(';
                } else {
                    group.push(c);
                }
            }
            ')' | '}' | ']' => {
                depth = depth.saturating_sub(1);
                if depth > 0 {
                    group.push(c);
                } else if explicit {
                    if let Some((binders, _)) = group.split_once(':') {
                        names += binders.split_whitespace().count();
                    }
                }
            }
            ':' if depth == 0 => {
                result = Some(header[i + 1..].trim());
                break;
            }
            _ if depth >= 1 => group.push(c),
            _ => {}
        }
    }
    Some((result == Some("Prop"), names))
}

/// A Lean declaration's name, from an anchor's `Ns::name`.
fn lean_name(symbol: &str) -> String {
    symbol.replace("::", ".")
}

/// The plan for every clause a `@models` or `@pins` claims, from the index and
/// the files' text.
pub fn plans(index: &Index, files: &std::collections::BTreeMap<String, String>) -> Vec<PinPlan> {
    use super::anchor::AnchorKind;
    let mut clauses: Vec<(String, Option<String>)> = index
        .links
        .iter()
        .filter(|l| matches!(l.role, Role::Models | Role::Pins))
        .map(|l| (l.req_id.clone(), l.clause.clone()))
        .collect();
    clauses.sort();
    clauses.dedup();
    clauses
        .into_iter()
        .map(|(req_id, clause)| {
            let mut plan = PinPlan { req_id: req_id.clone(), clause: clause.clone(), ..Default::default() };
            let mut hashes: Vec<String> = Vec::new();
            for link in index.links.iter().filter(|l| l.req_id == req_id && l.clause == clause) {
                let AnchorKind::Decl { symbol_path } = &link.anchor.kind else { continue };
                let text: String = files
                    .get(&link.anchor.file)
                    .map(|t| {
                        t.lines()
                            .skip(link.anchor.start_line as usize)
                            .take((link.anchor.end_line.saturating_sub(link.anchor.start_line) + 1) as usize)
                            .collect::<Vec<_>>()
                            .join("\n")
                    })
                    .unwrap_or_default();
                match link.role {
                    Role::Models => match shape(&text) {
                        Some((true, _)) if plan.spec.is_none() => plan.spec = Some(lean_name(symbol_path)),
                        Some((false, inputs)) if plan.model.is_none() => {
                            plan.model = Some(lean_name(symbol_path));
                            plan.inputs = inputs;
                        }
                        _ => continue,
                    },
                    Role::Pins if plan.theorem.is_none() => {
                        plan.theorem = Some(lean_name(symbol_path));
                        plan.file = Some(link.anchor.file.clone());
                    }
                    _ => continue,
                }
                hashes.push(link.anchor.body_hash.clone());
            }
            hashes.sort();
            plan.key = hashes.join("+");
            plan
        })
        .collect()
}

/// Where a clause's verdict is kept.
pub fn record_path(root: &std::path::Path, req_id: &str, clause: Option<&str>) -> std::path::PathBuf {
    root.join(".tracelean").join("pins").join(format!("{req_id}.{}.json", clause.unwrap_or("_")))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// @tests REQ-STRENGTH.per_input
    #[test]
    fn the_obligation_is_met_and_unique_per_input() {
        assert_eq!(
            statement("P".into(), "f".into(), 1),
            "(∀ x1, P x1 (f x1)) ∧ (∀ x1 y1 y2, P x1 y1 → P x1 y2 → y1 = y2)"
        );
        assert_eq!(statement("P".into(), "c".into(), 0), "(P c) ∧ (∀ y1 y2, P y1 → P y2 → y1 = y2)");
    }

    #[test]
    fn a_declaration_says_whether_it_is_a_proposition_and_what_it_takes() {
        assert_eq!(shape("/-- doc -/\ndef ToF (c f : Int) : Prop :=\n  c = f"), Some((true, 2)));
        assert_eq!(shape("def toF (c : Int) : Int := c"), Some((false, 1)));
        assert_eq!(shape("def g {α : Type} [Inhabited α] (xs : List α) (n : Nat) : α := default"), Some((false, 2)));
        assert_eq!(shape("theorem t : 1 = 1 := rfl"), None);
        assert_eq!(shape("def d (l : Link) (now : List (String × String)) : State := x"), Some((false, 2)));
        assert_eq!(shape("def e (f : (Nat → Nat)) (p : Nat × (Nat × Nat)) : Nat := 0"), Some((false, 2)));
    }

    /// @tests REQ-STRENGTH.kernel_decides
    #[test]
    fn only_a_clean_answer_naming_the_theorem_is_accepted() {
        let clean = "'T.p' depends on axioms: [propext, Quot.sound]\n";
        assert!(accepted("T.p".into(), clean.into(), true));
        assert!(!accepted("T.p".into(), clean.into(), false));
        assert!(!accepted("T.p".into(), "'T.p' depends on axioms: [sorryAx]\n".into(), true));
        assert!(!accepted("T.p".into(), format!("x.lean:3:0: error: type mismatch\n{clean}"), true));
        assert!(accepted("T.p".into(), "'T.p' does not depend on any axioms\n".into(), true));
    }

    /// @tests REQ-STRENGTH.verdict_kept
    #[test]
    fn a_verdict_counts_only_for_the_theorem_and_declarations_it_was_about() {
        let record = PinRecord { theorem_name: "t".into(), key: "a+b".into(), pinned: true, said: String::new() };
        assert_eq!(standing(None, Some(record.clone()), "a+b".into()), Strength::Open);
        assert_eq!(standing(Some("t".into()), Some(record.clone()), "a+b".into()).as_str(), "pinned");
        assert_eq!(standing(Some("t".into()), Some(record.clone()), "a+c".into()).as_str(), "attempted");
        assert_eq!(standing(Some("u".into()), Some(record), "a+b".into()).as_str(), "attempted");
        assert_eq!(standing(Some("t".into()), None, "a+b".into()).as_str(), "attempted");
    }
}
