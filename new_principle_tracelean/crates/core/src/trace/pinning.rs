use serde::{Deserialize, Serialize};

use super::annotation::Role;
use super::index::Index;
use super::strength::Strength;

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PinPlan {
    pub req_id: String,
    pub clause: Option<String>,
    pub spec: Option<String>,
    pub model: Option<String>,
    pub inputs: usize,
    pub theorem: Option<String>,
    pub file: Option<String>,
    pub key: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PinRecord {
    pub theorem_name: String,
    pub key: String,
    pub pinned: bool,
    #[serde(default)]
    pub said: String,
}

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

pub fn check_source(plan: &PinPlan) -> Option<String> {
    let theorem = plan.theorem.as_ref()?;
    let mut out = format!("\n\n-- TraceLean: is `{theorem}` the pinning obligation, and is it finished?\n");
    if let (Some(spec), Some(model)) = (&plan.spec, &plan.model) {
        out.push_str(&format!("example : {} := {theorem}\n", statement(spec.clone(), model.clone(), plan.inputs)));
    }
    out.push_str(&format!("#print axioms {theorem}\n"));
    Some(out)
}

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

pub const OPEN_WITHOUT_BOTH: &str =
    "needs one `@models` function and one `@specifies` `def … : Prop` beside it";

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
            _ => ("open".into(), OPEN_WITHOUT_BOTH.into()),
        },
    }
}

/// @implements REQ-STRENGTH.inputs_counted
/// @drt REQ-STRENGTH.inputs_counted
pub fn shape(declaration: String) -> Option<(bool, usize)> {
    let at = declaration.find("def ").or_else(|| declaration.find("abbrev "))?;
    let header = &declaration[at..];
    let header = &header[..header.find(":=").unwrap_or(header.len())];
    let (mut depth, mut names, mut group) = (0usize, 0usize, String::new());
    let mut explicit = false;
    let (mut typed, mut arrows, mut result) = (false, 0usize, String::new());
    for c in header.chars() {
        if typed {
            match c {
                '(' | '{' | '[' => {
                    depth += 1;
                    result.push(c);
                }
                ')' | '}' | ']' => {
                    depth = depth.saturating_sub(1);
                    result.push(c);
                }
                '|' if depth == 0 => break,
                '→' if depth == 0 => {
                    arrows += 1;
                    result.clear();
                }
                '>' if depth == 0 && result.ends_with('-') => {
                    arrows += 1;
                    result.clear();
                }
                _ => result.push(c),
            }
            continue;
        }
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
            ':' if depth == 0 => typed = true,
            _ if depth >= 1 => group.push(c),
            _ => {}
        }
    }
    Some((typed && result.trim() == "Prop", names + arrows))
}

fn lean_name(symbol: &str) -> String {
    symbol.replace("::", ".")
}

pub fn plans(index: &Index, files: &std::collections::BTreeMap<String, String>) -> Vec<PinPlan> {
    use super::anchor::AnchorKind;
    let mut clauses: Vec<(String, Option<String>)> = index
        .links
        .iter()
        .filter(|l| matches!(l.role, Role::Models | Role::Specifies | Role::Pins))
        .map(|l| (l.req_id.clone(), l.clause.clone()))
        .collect();
    clauses.sort();
    clauses.dedup();
    clauses
        .into_iter()
        .map(|(req_id, clause)| {
            let mut plan = PinPlan { req_id: req_id.clone(), clause: clause.clone(), ..Default::default() };
            let all = |role: Role| {
                let mut found: Vec<_> = index
                    .links
                    .iter()
                    .filter(|l| l.req_id == req_id && l.clause == clause && l.role == role)
                    .filter_map(|l| match &l.anchor.kind {
                        AnchorKind::Decl { symbol_path } => Some((symbol_path.clone(), l)),
                        _ => None,
                    })
                    .collect();
                found.dedup_by(|a, b| a.0 == b.0 && a.1.anchor.file == b.1.anchor.file);
                found
            };
            let the_one = |role: Role| {
                let mut found = all(role);
                if found.len() == 1 { found.pop() } else { None }
            };
            let text_of = |link: &super::index::Link| -> String {
                files
                    .get(&link.anchor.file)
                    .map(|t| {
                        t.lines()
                            .skip(link.anchor.start_line as usize)
                            .take((link.anchor.end_line.saturating_sub(link.anchor.start_line) + 1) as usize)
                            .collect::<Vec<_>>()
                            .join("\n")
                    })
                    .unwrap_or_default()
            };
            let mut hashes: Vec<String> = Vec::new();
            if let Some((symbol, link)) = the_one(Role::Specifies) {
                plan.spec = Some(lean_name(&symbol));
                hashes.push(link.anchor.body_hash.clone());
            }
            let pin = the_one(Role::Pins);
            if let Some((symbol, link)) = &pin {
                plan.theorem = Some(lean_name(symbol));
                plan.file = Some(link.anchor.file.clone());
                hashes.push(link.anchor.body_hash.clone());
            }
            let models: Vec<_> = all(Role::Models)
                .into_iter()
                .filter_map(|(symbol, link)| shape(text_of(link)).map(|(_, inputs)| (symbol, link, inputs)))
                .collect();
            let named: Vec<_> = match &pin {
                Some((_, link)) if models.len() > 1 => {
                    let theorem = text_of(link);
                    models.iter().filter(|(symbol, _, _)| mentions(&theorem, short_name(symbol))).collect()
                }
                _ => models.iter().collect(),
            };
            if let [(symbol, link, inputs)] = named.as_slice() {
                plan.model = Some(lean_name(symbol));
                plan.inputs = *inputs;
                hashes.push(link.anchor.body_hash.clone());
            }
            hashes.sort();
            plan.key = hashes.join("+");
            plan
        })
        .collect()
}

fn short_name(symbol: &str) -> &str {
    let last = symbol.rsplit("::").next().unwrap_or(symbol);
    last.rsplit('.').next().unwrap_or(last)
}

fn mentions(text: &str, name: &str) -> bool {
    let ident = |c: char| c.is_alphanumeric() || c == '_' || c == '\'';
    text.match_indices(name).any(|(at, _)| {
        let before = text[..at].chars().next_back();
        let after = text[at + name.len()..].chars().next();
        !before.is_some_and(ident) && !after.is_some_and(ident)
    })
}

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
        let shape = |s: &str| shape(s.to_string());
        assert_eq!(shape("/-- doc -/\ndef ToF (c f : Int) : Prop :=\n  c = f"), Some((true, 2)));
        assert_eq!(shape("def toF (c : Int) : Int := c"), Some((false, 1)));
        assert_eq!(shape("def g {α : Type} [Inhabited α] (xs : List α) (n : Nat) : α := default"), Some((false, 2)));
        assert_eq!(shape("theorem t : 1 = 1 := rfl"), None);
        assert_eq!(shape("def d (l : Link) (now : List (String × String)) : State := x"), Some((false, 2)));
        assert_eq!(shape("def e (f : (Nat → Nat)) (p : Nat × (Nat × Nat)) : Nat := 0"), Some((false, 2)));
    }

    /// @tests REQ-STRENGTH.inputs_counted
    #[test]
    fn a_pattern_matched_definition_takes_the_arrows_of_its_type() {
        let shape = |s: &str| shape(s.to_string());
        assert_eq!(
            shape("def firstBadRange (content : String) : List Edit → Option LowerError\n  | [] => none\n  | e :: rest => let x := e; x"),
            Some((false, 2))
        );
        assert_eq!(shape("def unavailableBecause : ServerState → String\n  | .running _ => \"a\""), Some((false, 1)));
        assert_eq!(shape("def Holds : Nat → List Nat -> Prop\n  | _, _ => True"), Some((true, 2)));
        assert_eq!(shape("def h : (Nat → Nat) → Nat := fun f => f 0"), Some((false, 1)));
        assert_eq!(shape("def k (n : Nat) : Nat → Prop := fun _ => True"), Some((true, 2)));
    }

    fn plans_of(text: &str) -> Vec<PinPlan> {
        let file = "specs/T.lean";
        let index = Index { links: crate::trace::index::links_of(file, text), ..Default::default() };
        let files = [(file.to_string(), text.to_string())].into_iter().collect();
        plans(&index, &files)
    }

    /// @tests REQ-STRENGTH.per_input
    #[test]
    fn the_spec_and_the_model_are_named_by_their_roles_not_their_order() {
        let text = "namespace T\n\n/-- @models REQ-A.c -/\ndef f (c : Int) : Int := c\n\n\
                    /-- @specifies REQ-A.c -/\ndef P (c y : Int) : Prop := y = c\n\n\
                    /-- @pins REQ-A.c -/\ntheorem t : True := trivial\n\nend T\n";
        let plan = &plans_of(text)[0];
        assert_eq!(plan.spec.as_deref(), Some("T.P"));
        assert_eq!(plan.model.as_deref(), Some("T.f"));
        assert_eq!(plan.inputs, 1);
        assert_eq!(plan.theorem.as_deref(), Some("T.t"));
        assert_eq!(plan.key.matches('+').count(), 2, "{}", plan.key);
    }

    #[test]
    fn with_two_models_there_is_no_model_to_pin() {
        let text = "/-- @models REQ-A.c -/\ndef f (c : Int) : Int := c\n\n\
                    /-- @models REQ-A.c -/\ndef g (c : Int) : Int := c\n\n\
                    /-- @specifies REQ-A.c -/\ndef P (c y : Int) : Prop := y = c\n";
        let plan = &plans_of(text)[0];
        assert_eq!(plan.spec.as_deref(), Some("P"));
        assert_eq!(plan.model, None);
        assert_eq!(shown(plan, None).1, OPEN_WITHOUT_BOTH);

        let pinned = format!("{text}\n/-- @pins REQ-A.c -/\ntheorem t : ∀ x, P x (g x) := fun _ => rfl\n");
        let plan = &plans_of(&pinned)[0];
        assert_eq!(plan.model.as_deref(), Some("g"));
        assert!(!mentions("theorem t : gg x", "g") && mentions("(g x)", "g"));
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
