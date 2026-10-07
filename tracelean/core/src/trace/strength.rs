//! Spec strength: do the proved properties *determine* the model?
//!
//! `@proves` says the model has a property. It says nothing about how much that
//! property rules out, and the difference is enormous: `discountCents s ≤ s` is
//! a real machine-checked theorem that the constant-zero function also
//! satisfies. Evidence at L4 is currently unqualified in exactly the way L3
//! would be without its coverage floor -- a run that found nothing must also
//! have had a chance of finding something, and a proof that holds of a hundred
//! different functions has barely constrained the one it is about.
//!
//! The measure comes from specification quality work in Dafny, where a contract
//! is called strong when the set of outputs satisfying it for a given input has
//! exactly one element. A Lean model is already a function, so that count is 1
//! by construction and measuring it there says nothing. The relational
//! specification here is the *set of proved theorems*, and the question ports
//! to it directly:
//!
//! ```text
//! ∀ f g, Spec f → Spec g → f = g
//! ```
//!
//! Provable ⇒ the theorems pin the model: anything satisfying them is it.
//! Unprovable ⇒ there is slack, and the slack is behaviour the proofs do not
//! rule out.
//!
//! TraceLean generates that obligation. It cannot prove it -- that is the
//! human's part, and the honest report while nobody has is "open", never
//! "fine".

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use super::annotation::Qualifier;
use super::{Role, TraceIndex};

/// What is known about one clause's spec strength.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum Strength {
    /// A `@pins` theorem exists and elaborates without `sorry`.
    Pinned,
    /// A `@pins` theorem exists but is not finished.
    Attempted { unproved: Vec<String> },
    /// Nobody has said anything. The default, and not a failure -- but not a
    /// pass either, which is the whole point of naming the state.
    Open,
    /// Declared unpinnable, with a reason.
    Nondeterministic { reason: Option<String> },
}

impl Strength {
    pub fn as_str(&self) -> &'static str {
        match self {
            Strength::Pinned => "pinned",
            Strength::Attempted { .. } => "attempted",
            Strength::Open => "open",
            Strength::Nondeterministic { .. } => "nondeterministic",
        }
    }
}

/// The obligation for one modelled clause, and everything needed to state it.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Obligation {
    /// Every clause this declaration models. The strength question is about the
    /// *function*, so one declaration owes one obligation however many clauses
    /// it serves, and every theorem proved about it counts towards the answer.
    pub clauses: Vec<(String, Option<String>)>,
    /// The `@models` declaration whose strength is in question.
    pub declaration: String,
    pub model_file: PathBuf,
    /// Names of the `@proves` theorems folded into `Spec`.
    pub theorems: Vec<String>,
    /// Generated Lean: the `Spec` predicate, the self-check, and the
    /// obligation with `sorry` where a proof goes.
    pub source: String,
    /// Modules the generated source imports.
    pub imports: Vec<String>,
    /// Namespaces to `open`, so the declarations can be named as they are
    /// written. A module name is not a namespace, and opening one is an error.
    pub namespaces: Vec<String>,
    /// What could not be derived. Shown, never silently defaulted.
    pub problems: Vec<String>,
}

/// Split a Lean declaration head into its binders and the type after the colon.
///
/// Purely syntactic, which is the only option without elaborating: binder
/// groups are balanced bracket runs, the first `:` at depth zero after them
/// opens the type, and `:=` at depth zero ends it. Enough for the shapes a
/// reference model is written in, and a wrong split fails loudly in Lean rather
/// than quietly producing a weaker obligation -- the generated file re-proves
/// that the model satisfies its own abstracted spec for exactly that reason.
pub fn split_head(source: &str) -> Option<(String, String)> {
    let after_name = {
        let mut it = source.char_indices().peekable();
        // skip the keyword and the declaration name
        let mut seen = 0;
        let mut idx = 0;
        while let Some((i, c)) = it.next() {
            if c.is_whitespace() {
                if seen == 2 {
                    idx = i;
                    break;
                }
                continue;
            }
            if i == 0 || source[..i].ends_with(char::is_whitespace) {
                seen += 1;
            }
            idx = i + c.len_utf8();
            if seen == 2 && it.peek().map(|(_, c)| c.is_whitespace()).unwrap_or(false) {
                break;
            }
        }
        idx
    };

    let bytes: Vec<char> = source[after_name..].chars().collect();
    let mut depth = 0i32;
    let mut binders_end = None;
    let mut type_start = None;
    let mut type_end = None;
    let mut i = 0;
    while i < bytes.len() {
        let c = bytes[i];
        match c {
            '(' | '{' | '[' | '⦃' => depth += 1,
            ')' | '}' | ']' | '⦄' => depth -= 1,
            ':' if depth == 0 => {
                if i + 1 < bytes.len() && bytes[i + 1] == '=' {
                    type_end = Some(i);
                    break;
                }
                if type_start.is_none() {
                    binders_end = Some(i);
                    type_start = Some(i + 1);
                }
            }
            _ => {}
        }
        i += 1;
    }

    let binders_end = binders_end?;
    let type_start = type_start?;
    let type_end = type_end.unwrap_or(bytes.len());
    let binders: String = bytes[..binders_end].iter().collect();
    let ty: String = bytes[type_start..type_end].iter().collect();
    Some((binders.trim().to_string(), ty.trim().to_string()))
}

/// Replace every occurrence of a declaration's name, qualified or not, with
/// `f`. Word boundaries only, so `discountCents` does not match inside
/// `discountCentsRounded`.
fn abstract_symbol(statement: &str, name: &str, namespace: Option<&str>) -> String {
    let mut out = statement.to_string();
    let mut targets = vec![name.to_string()];
    if let Some(ns) = namespace {
        targets.insert(0, format!("{ns}.{name}"));
    }
    for target in targets {
        out = replace_identifier(&out, &target, "f");
    }
    out
}

fn replace_identifier(haystack: &str, needle: &str, with: &str) -> String {
    let is_ident = |c: char| c.is_alphanumeric() || c == '_' || c == '\'' || c == '.';
    let mut out = String::with_capacity(haystack.len());
    let mut rest = haystack;
    while let Some(at) = rest.find(needle) {
        let before_ok = rest[..at].chars().next_back().map(|c| !is_ident(c)).unwrap_or(true);
        let after = &rest[at + needle.len()..];
        let after_ok = after.chars().next().map(|c| !is_ident(c)).unwrap_or(true);
        out.push_str(&rest[..at]);
        if before_ok && after_ok {
            out.push_str(with);
        } else {
            out.push_str(needle);
        }
        rest = after;
    }
    out.push_str(rest);
    out
}

/// The function type a candidate must have, built from the model's own binders.
fn function_type(binders: &str, result: &str) -> Option<String> {
    let mut parts = Vec::new();
    let chars: Vec<char> = binders.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        if chars[i] == '(' {
            let mut depth = 1;
            let start = i + 1;
            let mut j = start;
            while j < chars.len() && depth > 0 {
                match chars[j] {
                    '(' => depth += 1,
                    ')' => depth -= 1,
                    _ => {}
                }
                j += 1;
            }
            let group: String = chars[start..j - 1].iter().collect();
            let (names, ty) = group.split_once(':')?;
            let count = names.split_whitespace().count().max(1);
            for _ in 0..count {
                parts.push(ty.trim().to_string());
            }
            i = j;
        } else {
            i += 1;
        }
    }
    parts.push(result.trim().to_string());
    Some(parts.join(" → "))
}

/// The `namespace` a line sits inside, if any.
fn namespace_at(source: &str, line: usize) -> Option<String> {
    let mut current = None;
    for (i, text) in source.lines().enumerate() {
        if i > line {
            break;
        }
        let trimmed = text.trim();
        if let Some(rest) = trimmed.strip_prefix("namespace ") {
            current = Some(rest.trim().to_string());
        } else if trimmed.starts_with("end ") {
            current = None;
        }
    }
    current
}

fn module_name(file: &Path) -> String {
    file.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default()
}

/// Build the obligation for every modelled declaration that has proofs to fold
/// in.
pub fn obligations(root: &Path, index: &TraceIndex) -> Vec<Obligation> {
    let mut sources: BTreeMap<PathBuf, String> = BTreeMap::new();
    let mut read = |file: &Path| -> Option<String> {
        if let Some(text) = sources.get(file) {
            return Some(text.clone());
        }
        let text = std::fs::read_to_string(root.join(file)).ok()?;
        sources.insert(file.to_path_buf(), text.clone());
        Some(text)
    };

    // Group by the declaration, not by the clause. `shippingCents` models three
    // clauses of REQ-SHIPPING; asking the strength question three times would
    // emit three predicates with the same name, and answering it once is also
    // what the question means.
    let mut groups: BTreeMap<(PathBuf, String), Vec<(String, Option<String>)>> = BTreeMap::new();
    let mut lines: BTreeMap<(PathBuf, String), u32> = BTreeMap::new();
    for model in index.links.iter().filter(|l| l.role == Role::Models) {
        let Some(symbol) = model.anchor.symbol_path() else { continue };
        let declaration = symbol.rsplit("::").next().unwrap_or(symbol).to_string();
        let key = (model.anchor.file.clone(), declaration);
        groups
            .entry(key.clone())
            .or_default()
            .push((model.req_id.clone(), model.clause.clone()));
        lines.entry(key).or_insert(model.anchor.start_line);
    }

    let mut out = Vec::new();
    for ((file, declaration), clauses) in groups {
        let Some(model_source) = read(&file) else { continue };

        let mut proofs = Vec::new();
        for (req_id, clause) in &clauses {
            for link in index.links_for_clause(req_id, clause.as_deref()) {
                if link.role == Role::Proves && !proofs.iter().any(|p: &&super::Link| {
                    p.anchor.symbol_path() == link.anchor.symbol_path()
                }) {
                    proofs.push(link);
                }
            }
        }
        if proofs.is_empty() {
            continue;
        }

        let mut problems = Vec::new();
        let start_line = lines.get(&(file.clone(), declaration.clone())).copied().unwrap_or(0);
        let namespace = namespace_at(&model_source, start_line as usize);
        let model_decl = super::anchor::declarations_in(&file, &model_source)
            .into_iter()
            .find(|d| d.name == declaration);
        let candidate_type = model_decl
            .as_ref()
            .and_then(|d| {
                let text = decl_text(&model_source, d.start_line, d.end_line);
                let (binders, result) = split_head(&text)?;
                function_type(&binders, &result)
            })
            .unwrap_or_else(|| {
                problems.push(format!(
                    "could not read a type for `{declaration}` from its declaration, so the \
                     obligation names it `_` and Lean will have to infer it"
                ));
                "_".to_string()
            });

        let mut conjuncts = Vec::new();
        let mut theorems = Vec::new();
        let mut imports = vec![module_name(&file)];
        let mut namespaces: Vec<String> = namespace.iter().cloned().collect();
        for proof in &proofs {
            let Some(name) = proof
                .anchor
                .symbol_path()
                .map(|s| s.rsplit("::").next().unwrap_or(s).to_string())
            else {
                problems.push("a `@proves` link is not attached to a declaration".into());
                continue;
            };
            let Some(proof_source) = read(&proof.anchor.file) else { continue };
            let Some(decl) = super::anchor::declarations_in(&proof.anchor.file, &proof_source)
                .into_iter()
                .find(|d| d.name == name)
            else {
                problems.push(format!(
                    "could not find `{name}` in {}",
                    proof.anchor.file.display()
                ));
                continue;
            };
            let text = decl_text(&proof_source, decl.start_line, decl.end_line);
            let Some((binders, statement)) = split_head(&text) else {
                problems.push(format!("could not read the statement of `{name}`"));
                continue;
            };
            let abstracted = abstract_symbol(&statement, &declaration, namespace.as_deref());
            if abstracted == statement {
                // A theorem that never mentions the model says nothing about
                // it, and folding it in would make the spec look stronger than
                // it is.
                problems.push(format!(
                    "`{name}` does not mention `{declaration}`, so it constrains nothing about \
                     it and was left out of the spec"
                ));
                continue;
            }
            let binder_text = if binders.trim().is_empty() {
                String::new()
            } else {
                format!(" {}", binders.trim())
            };
            conjuncts.push(format!("(∀{binder_text}, {})", abstracted.trim()));
            theorems.push(name);
            let module = module_name(&proof.anchor.file);
            if !imports.contains(&module) {
                imports.push(module);
            }
            if let Some(ns) = namespace_at(&proof_source, decl.start_line as usize) {
                if !namespaces.contains(&ns) {
                    namespaces.push(ns);
                }
            }
        }

        if conjuncts.is_empty() {
            continue;
        }

        let spec_name = format!("Spec_{declaration}");
        let qualified = match &namespace {
            Some(ns) => format!("{ns}.{declaration}"),
            None => declaration.clone(),
        };
        let source = render(
            &spec_name,
            &candidate_type,
            &conjuncts,
            &qualified,
            &declaration,
            &theorems,
            &clauses,
        );

        out.push(Obligation {
            clauses,
            declaration,
            model_file: file,
            theorems,
            source,
            imports,
            namespaces,
            problems,
        });
    }
    out
}

fn decl_text(source: &str, start_line: u32, end_line: u32) -> String {
    source
        .lines()
        .skip(start_line as usize)
        .take((end_line - start_line + 1) as usize)
        .collect::<Vec<_>>()
        .join("\n")
}

#[allow(clippy::too_many_arguments)]
fn render(
    spec_name: &str,
    candidate_type: &str,
    conjuncts: &[String],
    qualified: &str,
    declaration: &str,
    theorems: &[String],
    clauses: &[(String, Option<String>)],
) -> String {
    let labels: Vec<String> = clauses
        .iter()
        .map(|(req, clause)| match clause {
            Some(c) => format!("{req}.{c}"),
            None => req.clone(),
        })
        .collect();
    let label = labels.join(", ");
    let pins = labels
        .iter()
        .map(|l| format!("@pins {l}"))
        .collect::<Vec<_>>()
        .join("\n    ");
    let body = conjuncts.join(" ∧\n  ");
    let witness = if theorems.len() == 1 {
        theorems[0].clone()
    } else {
        format!("⟨{}⟩", theorems.join(", "))
    };
    format!(
        r#"/-- The properties proved about `{declaration}`, as a predicate over candidate
    functions. Generated from the proof links for {label}: {theorems}.

    (The role is named in prose here on purpose. Written with its leading
    marker it would be a real annotation in a generated file, and the checker
    would report an annotation with no requirement attached -- which is exactly
    what happened the first time.) -/
def {spec_name} (f : {candidate_type}) : Prop :=
  {body}

/-- The model satisfies its own abstracted specification.

    Generated, and a guard on the generation itself: the predicate above is
    built by rewriting the theorems' statements, and if that rewriting went
    wrong this stops elaborating. -/
theorem {declaration}_models_its_spec : {spec_name} {qualified} := by
  unfold {spec_name}
  exact {witness}

/-- {pins}

    Do those properties *determine* `{declaration}`, or merely constrain it?
    Proving this says anything satisfying them is that function. Leaving it
    `sorry` says nobody knows yet, which TraceLean reports as open rather than
    as passing.

    If it turns out to be false, that is the more useful answer: exhibit a
    second function satisfying `{spec_name}` and differing from `{qualified}`,
    and the witness shows exactly which behaviour the proofs fail to rule out.

    If `{declaration}` is genuinely not determined by its inputs -- randomness,
    a clock, concurrency -- delete this and write the nondeterministic
    qualifier, with a reason, beside the model instead. (Spelled out with its
    leading marker it would be a real annotation rather than a mention of one,
    which is why this sentence names it in prose.) -/
theorem {declaration}_pinned :
    ∀ f g, {spec_name} f → {spec_name} g → f = g := by
  sorry
"#,
        theorems = theorems.join(", "),
    )
}

/// Assemble one Lean file holding every obligation.
pub fn render_file(obligations: &[Obligation]) -> String {
    let mut imports: Vec<String> = Vec::new();
    for o in obligations {
        for module in &o.imports {
            if !imports.contains(module) {
                imports.push(module.clone());
            }
        }
    }
    let mut out = String::from(
        "/-\n  Spec-strength obligations, generated by TraceLean.\n\n  \
         Each set of proved properties gets one question: do they determine the\n  \
         model, or merely constrain it? TraceLean can state the question. Only a\n  \
         person can answer it, so every obligation arrives as `sorry` and is\n  \
         reported as open until somebody proves or refutes it.\n\n  \
         This file is a scaffold: it is written once and never overwritten, because\n  \
         the proofs you add here are the point.\n-/\n",
    );
    for module in &imports {
        out.push_str(&format!("import {module}\n"));
    }
    // The obligations quantify over functions that ignore some of their
    // arguments, and an unused binder in a generated statement is noise rather
    // than a finding.
    out.push_str("\nset_option linter.unusedVariables false\n");
    let mut namespaces: Vec<String> = Vec::new();
    for o in obligations {
        for ns in &o.namespaces {
            if !namespaces.contains(ns) {
                namespaces.push(ns.clone());
            }
        }
    }
    for ns in &namespaces {
        out.push_str(&format!("open {ns}\n"));
    }
    out.push('\n');
    for o in obligations {
        out.push_str(&o.source);
        out.push('\n');
    }
    out
}

/// What the index knows about a clause's strength, before Lean has been run.
pub fn declared(index: &TraceIndex, req_id: &str, clause: Option<&str>) -> Strength {
    let links = index.links_for_clause(req_id, clause);
    for link in &links {
        if let Some(Qualifier::Nondeterministic { reason }) = &link.qualifier {
            return Strength::Nondeterministic { reason: reason.clone() };
        }
    }
    if links.iter().any(|l| l.role == Role::Pins) {
        // Whether it is finished is a question for the toolchain; the index
        // only knows somebody claimed it.
        return Strength::Attempted { unproved: Vec::new() };
    }
    Strength::Open
}

/// Names of declarations Lean reported as using `sorry`.
///
/// `lake env lean` prints one warning per unfinished declaration. An obligation
/// that elaborates without one is proved -- there is no partial credit and no
/// way to claim it without the kernel agreeing.
pub fn unproved_in(lean_output: &str) -> Vec<String> {
    let mut out = Vec::new();
    for line in lean_output.lines() {
        if line.contains("declaration uses 'sorry'") {
            out.push(line.trim().to_string());
        }
    }
    out
}

/// The lake package a file belongs to: the nearest ancestor holding a lakefile.
///
/// `lake env lean` has to run inside the package or the file's own imports do
/// not resolve, which is the same reason the Lean language server is started
/// there rather than at the project root.
fn package_root_for(root: &Path, file: &Path) -> Option<PathBuf> {
    let absolute = root.join(file);
    let mut dir = absolute.parent();
    while let Some(current) = dir {
        if !current.starts_with(root) {
            break;
        }
        if current.join("lakefile.toml").exists() || current.join("lakefile.lean").exists() {
            return Some(current.to_path_buf());
        }
        if current == root {
            break;
        }
        dir = current.parent();
    }
    None
}

/// Lines Lean reported as using `sorry`, 1-indexed as Lean prints them.
pub fn sorry_lines(lean_output: &str) -> Vec<u32> {
    lean_output
        .lines()
        .filter(|l| l.contains("declaration uses 'sorry'"))
        .filter_map(|l| {
            let mut parts = l.split(':');
            let _file = parts.next()?;
            parts.next()?.trim().parse::<u32>().ok()
        })
        .collect()
}

/// Ask the toolchain whether each `@pins` claim is actually finished.
///
/// A claim is not evidence. `@pins` says somebody wrote the theorem; only the
/// kernel can say they proved it, and a `sorry` anywhere inside the declaration
/// means they did not. Without a Lean toolchain the answer is "claimed, not
/// checked" rather than a pass -- the same rule the judge and the differential
/// runner follow.
pub fn check(root: &Path, index: &TraceIndex) -> BTreeMap<(String, Option<String>), Strength> {
    let mut out = BTreeMap::new();

    // Declared states first: nondeterministic wins over everything, and a
    // clause nobody has claimed is open.
    for req in index.requirements.values() {
        for clause in req.clause_keys() {
            let key = (req.id.clone(), clause.clone());
            out.insert(key, declared(index, &req.id, clause.as_deref()));
        }
    }

    let mut files: BTreeMap<PathBuf, Vec<&super::Link>> = BTreeMap::new();
    for link in index.links.iter().filter(|l| l.role == Role::Pins) {
        files.entry(link.anchor.file.clone()).or_default().push(link);
    }

    for (file, links) in files {
        let Some(package) = package_root_for(root, &file) else { continue };
        let Ok(toolchain) = crate::drt::lean_runner::toolchain() else { continue };
        let relative = root.join(&file);
        let output = std::process::Command::new(&toolchain.lake)
            .args(["env", "lean"])
            .arg(&relative)
            .current_dir(&package)
            .output();
        let Ok(output) = output else { continue };
        let text = format!(
            "{}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        let unfinished = sorry_lines(&text);

        for link in links {
            // Anchors are 0-indexed; Lean prints 1-indexed lines.
            let start = link.anchor.start_line + 1;
            let end = link.anchor.end_line + 1;
            let inside: Vec<String> = unfinished
                .iter()
                .filter(|line| **line >= start && **line <= end)
                .map(|line| format!("{}:{line}", file.display()))
                .collect();
            let state = if inside.is_empty() {
                Strength::Pinned
            } else {
                Strength::Attempted { unproved: inside }
            };
            let key = (link.req_id.clone(), link.clause.clone());
            // Never overwrite a declared exemption with a machine result: the
            // exemption is the considered answer.
            match out.get(&key) {
                Some(Strength::Nondeterministic { .. }) => {}
                _ => {
                    out.insert(key, state);
                }
            }
        }
    }

    out
}
