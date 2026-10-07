//! One requirement, opened: its clauses, what each reached, and what claims it.
//!
//! The first TraceLean's clause detail put a requirement's clauses beside the
//! code, models and tests that claimed them, each a link that opened the file
//! at the line. This is that view as a buffer. A link is a span whose text is
//! `path:line` and whose action is `file.open`; the shell opens the file at
//! that line, as it would for any `path:line` a person points at.
//!
//! @implements REQ-SHOW.requirement_opened

use serde::{Deserialize, Serialize};

use crate::evidence::Level;
use crate::surface::sandbox_view::Lines;
use crate::surface::view::{Buffer, Role};

/// Something that claims a clause: the annotation, and where it is.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Claim {
    /// `implements`, `tests`, `models`, `proves`, `drt`, …
    pub role: String,
    pub path: String,
    /// One-based, as an editor counts.
    pub line: u32,
    /// The item the annotation sits on — `to_celsius`, `Thermo::toCelsius` —
    /// when it sits on one rather than on a region or the whole file.
    #[serde(default)]
    pub symbol: Option<String>,
}

/// One clause as the view shows it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ClauseShown {
    /// `None` for a requirement that declares no clauses.
    pub key: Option<String>,
    pub text: String,
    pub level: Level,
    /// The chain the level is the minimum of, as `L2/L1/L3`.
    pub chain: String,
    pub claims: Vec<Claim>,
    /// Whether its specification pins its model, for a modelled clause:
    /// `pinned`, `attempted` or `open`, with the theorem, or what is owed.
    #[serde(default)]
    pub pins: Option<(String, String)>,
    /// Whether the code was differentially tested against the model, however
    /// the test came about — a `@drt` claim or `tracelean-trace --drt`.
    #[serde(default)]
    pub tested: bool,
    /// How much of its implementing items tests run, when it was measured:
    /// executable lines run, executable lines, and how many tests ran them.
    #[serde(default)]
    pub lines: Option<(u64, u64, u64)>,
}

/// Everything the view is produced from.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RequirementShown {
    pub id: String,
    pub title: String,
    /// The requirement's own document.
    pub file: String,
    /// `draft`, `approved`, … as its frontmatter says.
    pub status: String,
    pub refines: Vec<String>,
    /// The requirements that refine this one: what a change here reaches.
    #[serde(default)]
    pub refined_by: Vec<String>,
    pub clauses: Vec<ClauseShown>,
    /// The characters a line may take before it wraps.
    pub width: usize,
}

/// A link as the view writes it: what a person reads and a click opens.
pub fn link_text(path: &str, line: u32) -> String {
    format!("{path}:{line}")
}

/// The requirement as a buffer, titled `requirement <id>`.
pub fn requirement_view(view: RequirementShown) -> Buffer {
    let mut out = Lines::new(view.width);
    out.line(&[(&view.id, Role::Requirement, &[])]);
    out.wrapped("", &view.title, Role::Heading, &[]);
    out.line(&[(&view.file, Role::Path, &["file.open"])]);
    // A draft can be approved from here; the button acts on the document
    // named on the line above.
    let status = format!("status {}", view.status);
    if view.status == "draft" {
        out.line(&[(&status, Role::Plain, &[]), ("  ", Role::Plain, &[]), ("[ Approve ]", Role::Added, &["trace.approve"])]);
    } else {
        out.line(&[(&status, Role::Plain, &[])]);
    }
    for (word, names) in [("refines    ", &view.refines), ("refined by ", &view.refined_by)] {
        if names.is_empty() {
            continue;
        }
        let mut pieces: Vec<(&str, Role, &[&str])> = vec![(word, Role::Plain, &[])];
        for (n, name) in names.iter().enumerate() {
            if n > 0 {
                pieces.push((" ", Role::Plain, &[]));
            }
            pieces.push((name.as_str(), Role::Requirement, &["trace.requirement"]));
        }
        out.line(&pieces);
    }
    // How many clauses each kind of claim reaches, in the claims' colours: where
    // this requirement is implemented, tested, modelled and proved, at a glance.
    let total = view.clauses.len();
    let counts: Vec<(crate::trace::annotation::Role, String)> = [
        crate::trace::annotation::Role::Implements,
        crate::trace::annotation::Role::Tests,
        crate::trace::annotation::Role::Models,
        crate::trace::annotation::Role::Proves,
        crate::trace::annotation::Role::Drt,
    ]
    .into_iter()
    .map(|role| {
        let reached = view
            .clauses
            .iter()
            .filter(|c| {
                c.claims.iter().any(|claim| claim.role == role.as_str())
                    || (role == crate::trace::annotation::Role::Drt && c.tested)
            })
            .count();
        (role, format!("{} {reached}/{total}", role.as_str()))
    })
    .collect();
    // What an agent needs to change it: the name opens its context.
    out.line(&[("for agent  ", Role::Plain, &[]), (&view.id, Role::Requirement, &["trace.context"]), ("  context to copy", Role::Plain, &[])]);
    let mut pieces: Vec<(&str, Role, &[&str])> = vec![("evidence   ", Role::Plain, &[])];
    for (n, (role, said)) in counts.iter().enumerate() {
        if n > 0 {
            pieces.push((" ", Role::Plain, &[]));
        }
        pieces.push((said.as_str(), Role::Claim { role: *role }, &[]));
    }
    out.line(&pieces);
    for clause in &view.clauses {
        out.blank();
        let grade = format!("{:?}", clause.level);
        let key = clause.key.clone().unwrap_or_else(|| "(the whole requirement)".into());
        let chain = format!("  {}", clause.chain);
        out.line(&[
            (&grade, Role::Level { grade: clause.level }, &["trace.rollup"]),
            ("  ", Role::Plain, &[]),
            (&key, Role::Heading, &[]),
            (&chain, Role::Plain, &[]),
        ]);
        out.wrapped("  ", &clause.text, Role::Plain, &[]);
        if let Some(key) = &clause.key {
            let name = format!("{}.{key}", view.id);
            out.line(&[("  for agent   ", Role::Plain, &[]), (&name, Role::Requirement, &["trace.context"])]);
        }
        if clause.claims.is_empty() {
            out.line(&[("  nothing claims it yet", Role::Removed, &[])]);
        }
        // How much of the code that implements it the tests run.
        if let Some((run, all, tests)) = clause.lines {
            let role = if run == all { Role::Added } else { Role::Removed };
            let said = format!("{run}/{all} lines run, by {tests} test{}", if tests == 1 { "" } else { "s" });
            out.line(&[("  covered     ", Role::Plain, &[]), (&said, role, &[])]);
        }
        // Whether the specification leaves one answer per input, as Lean said.
        if let Some((state, said)) = &clause.pins {
            let role = if state == "pinned" { Role::Added } else { Role::Removed };
            out.line(&[("  ", Role::Plain, &[]), (state.as_str(), role, &[])]);
            out.wrapped("    ", said, Role::Plain, &[]);
        }
        // A clause a model claims can be judged against it, by a person: the
        // name opens the prompt to carry to them.
        if clause.claims.iter().any(|c| c.role == "models") {
            let name = match &clause.key {
                Some(key) => format!("{}.{key}", view.id),
                None => view.id.clone(),
            };
            out.line(&[("  judge       ", Role::Plain, &[]), (&name, Role::Requirement, &["trace.judge"])]);
        }
        // Each claim's kind in its chip's colour, then the link to it.
        for claim in &clause.claims {
            let kind = format!("{:<10}", claim.role);
            let role = match crate::trace::annotation::Role::parse(&claim.role) {
                Some(role) => Role::Claim { role },
                None => Role::Plain,
            };
            let link = link_text(&claim.path, claim.line);
            let symbol = claim.symbol.as_deref().map(|s| format!("  {s}"));
            let mut pieces: Vec<(&str, Role, &[&str])> =
                vec![("  ", Role::Plain, &[]), (&kind, role, &[]), (" ", Role::Plain, &[]), (&link, Role::Path, &["file.open"])];
            if let Some(symbol) = &symbol {
                pieces.push((symbol, Role::Token { kind: crate::surface::view::TokenKind::Function }, &[]));
            }
            out.line(&pieces);
        }
    }
    out.finish(&format!("requirement {}", view.id))
}

/// The prompt a person carries to whoever judges a clause against its model,
/// as a record titled `judge <clause>`. The text is the prompt and nothing
/// else, so copying the buffer copies exactly the prompt
/// (`REQ-JUDGE.prompt_exported`).
pub fn judge_view(clause: &str, prompt: String) -> Buffer {
    let title = format!("judge {clause}");
    Buffer {
        id: format!("record:{title}"),
        kind: crate::surface::view::BufferKind::Record { title },
        text: prompt,
        spans: Vec::new(),
    }
}

/// Where a `path:line` link points: the path, and the line if it names one.
pub fn split_link(text: &str) -> (&str, Option<u32>) {
    match text.rsplit_once(':') {
        Some((path, line)) if !path.is_empty() => match line.parse::<u32>() {
            Ok(n) => (path, Some(n)),
            Err(_) => (text, None),
        },
        _ => (text, None),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::surface::view::{actions_at, faults};

    fn view() -> RequirementShown {
        RequirementShown {
            id: "REQ-X".into(),
            title: "Something shall hold".into(),
            file: "reqs/REQ-X.md".into(),
            status: "draft".into(),
            refines: vec!["ARCH-A".into()],
            refined_by: vec!["REQ-Y".into()],
            clauses: vec![
                ClauseShown {
                    key: Some("holds".into()),
                    text: "It shall hold.".into(),
                    level: Level::L2,
                    chain: "L2/L3/L2".into(),
                    claims: vec![Claim { role: "implements".into(), path: "src/x.rs".into(), line: 12, symbol: Some("hold".into()) }],
                    pins: Some(("pinned".into(), "X.holds_pinned".into())),
                    tested: true,
                    lines: Some((3, 4, 2)),
                },
                ClauseShown {
                    key: Some("unclaimed".into()),
                    text: "Nobody does this.".into(),
                    level: Level::L1,
                    chain: "L1/L1/L1".into(),
                    claims: vec![],
                    pins: None,
                    tested: false,
                    lines: None,
                },
            ],
            width: 60,
        }
    }

    fn offset_of(buffer: &Buffer, needle: &str) -> usize {
        let at = buffer.text.find(needle).expect(needle);
        buffer.text[..at].chars().count()
    }

    /// Each clause shows its level and what claims it, and a claim is a link
    /// that opens the file at the line.
    ///
    /// @tests REQ-SHOW.requirement_opened
    #[test]
    fn a_requirement_shows_its_clauses_and_links_to_what_claims_them() {
        let shown = requirement_view(view());
        assert!(faults(shown.clone()).is_empty());
        assert_eq!(shown.id, "record:requirement REQ-X");
        assert_eq!(actions_at(shown.clone(), offset_of(&shown, "src/x.rs:12")), vec!["file.open".to_string()]);
        assert_eq!(actions_at(shown.clone(), offset_of(&shown, "ARCH-A")), vec!["trace.requirement".to_string()]);
        assert!(shown.text.contains("nothing claims it yet"));
        assert!(shown.text.contains("  pinned\n    X.holds_pinned"), "{}", shown.text);
        assert!(shown.text.contains("drt 1/2"), "a derived test counts: {}", shown.text);
        assert!(shown.text.contains("covered     3/4 lines run, by 2 tests"), "{}", shown.text);
        assert!(shown.text.find("holds").unwrap() < shown.text.find("unclaimed").unwrap());
    }

    #[test]
    fn a_link_splits_into_its_path_and_line() {
        assert_eq!(split_link("src/x.rs:12"), ("src/x.rs", Some(12)));
        assert_eq!(split_link("src/x.rs"), ("src/x.rs", None));
        assert_eq!(split_link("a:b"), ("a:b", None));
    }
}
