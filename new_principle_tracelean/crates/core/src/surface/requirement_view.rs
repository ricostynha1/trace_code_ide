//! One requirement, opened: its clauses, what each reached, and what claims it.
//!
//! The first TraceLean's clause detail put a requirement's clauses beside the
//! code, models and tests that claimed them, each a link that opened the file
//! at the line. This is that view as a buffer. A link is a span whose text is
//! `path:line` and whose action is `file.open`; the shell opens the file at
//! that line, as it would for any `path:line` a person points at.

use serde::{Deserialize, Serialize};

use crate::evidence::{Level, Marks};
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

/// The marks as pieces of a line, a letter or a dash each with the role saying
/// which mark it is and whether it is held.
/// A mark that cannot be held is plain: it is neither a gap nor evidence.
pub fn mark_pieces(marks: Marks) -> Vec<(String, Role)> {
    marks
        .each()
        .iter()
        .map(|&(mark, held, letter)| {
            let shown = crate::evidence::letter_of(marks, (mark, held, letter)).to_string();
            (shown, if marks.applies(mark) { Role::Mark { mark, held } } else { Role::Plain })
        })
        .collect()
}

/// One clause as the view shows it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ClauseShown {
    /// `None` for a requirement that declares no clauses.
    pub key: Option<String>,
    pub text: String,
    /// What narrows the clause, key and text, shown indented under it.
    #[serde(default)]
    pub narrowings: Vec<(String, String)>,
    /// What evidence it holds, written `AJTP` with gaps.
    pub marks: Marks,
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
    /// The judgement recorded for the clause against its model, if any.
    #[serde(default)]
    pub judged: Option<Judged>,
}

/// A clause's judgement as the view shows it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Judged {
    /// `agrees`, `drift` or `unmodelable`.
    pub verdict: String,
    /// `judgedBy` rather than `by`, which the model's language reserves.
    pub judged_by: String,
    pub delegated_by: Option<String>,
    pub note: Option<String>,
    pub level: Level,
}

/// The judgement among a clause's records: what sits in its judgement slot.
///
/// @implements REQ-JUDGE.judgement_shown
pub fn judged_of(
    records: &[crate::trace::record::Evidence],
    req_id: &str,
    clause: Option<&str>,
) -> Option<Judged> {
    records.iter().find_map(|record| {
        if record.key.req_id != req_id
            || record.key.clause.as_deref() != clause
            || record.key.bond != crate::evidence::Bond::RequirementModel
        {
            return None;
        }
        match &record.detail {
            crate::trace::record::Detail::Judge { verdict, judged_by, delegated_by, note, .. } => Some(Judged {
                verdict: verdict.clone(),
                judged_by: judged_by.clone(),
                delegated_by: delegated_by.clone(),
                note: note.clone(),
                level: record.effective_level(),
            }),
            _ => None,
        }
    })
}

/// A judgement in one line: `agrees by claude-review — L2 (delegated by ana)`,
/// or `judged: drift — <note>` for a verdict that the two differ.
///
/// @implements REQ-JUDGE.judgement_shown
/// @drt REQ-JUDGE.judgement_shown
pub fn judged_text(judged: Judged) -> String {
    let delegated = judged
        .delegated_by
        .as_deref()
        .map(|person| format!(" (delegated by {person})"))
        .unwrap_or_default();
    if judged.verdict == "agrees" {
        format!("agrees by {} — {:?}{delegated}", judged.judged_by, judged.level)
    } else {
        let note = judged.note.as_deref().unwrap_or("no note");
        format!("judged: {} — {note}  by {}{delegated}", judged.verdict, judged.judged_by)
    }
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
    /// The prose under the frontmatter: what the document says besides its
    /// clauses. Empty for a requirement whose body is its one clause, which
    /// the clause already shows.
    #[serde(default)]
    pub body: String,
    pub clauses: Vec<ClauseShown>,
    /// The characters a line may take before it wraps.
    pub width: usize,
    /// The same as a clause's `lines`, over every item implementing any of
    /// its clauses, each line counted once.
    #[serde(default)]
    pub lines: Option<(u64, u64, u64)>,
}

/// How much ran, as a coverage line says it: `3/4 lines run (75%), by 2 tests`.
pub fn covered_text(run: u64, all: u64, tests: u64) -> String {
    if all == 0 {
        return NO_LINES.to_string();
    }
    let percent = run * 100 / all;
    format!("{run}/{all} lines run ({percent}%), by {tests} test{}", if tests == 1 { "" } else { "s" })
}

/// What a coverage line says when code claims the clause and no measurement
/// of that code as it is now exists.
pub const NOT_MEASURED: &str = "not measured: run `tracelean-trace . --coverage`";

/// What it says when the measured code has no line a test could run — a type,
/// a constant, or code no test binary builds: neither covered nor not.
pub const NO_LINES: &str = "no line a test could run";

/// Whether code claims the clause.
fn implemented(clause: &ClauseShown) -> bool {
    clause.claims.iter().any(|c| c.role == "implements")
}

/// How many clauses hold each kind of evidence, one letter each and in the
/// order the `evidence` line above it names them — implements, proves, models,
/// tests, strong, drt — each in the colour of its claim, so the two lines read
/// as one thing written twice: in words and in letters. A count is out of the
/// clauses a model can answer, `I·` where none can.
fn marks_line(out: &mut Lines, clauses: &[ClauseShown]) {
    use crate::trace::annotation::Role as R;
    let modelled: Vec<&ClauseShown> = clauses.iter().filter(|c| !c.marks.structural).collect();
    let count = |held: &dyn Fn(&ClauseShown) -> bool| modelled.iter().filter(|c| held(c)).count();
    let counts = [
        ("I", count(&implemented), R::Implements),
        ("P", count(&|c| c.marks.proved), R::Proves),
        ("M", count(&|c| c.claims.iter().any(|claim| claim.role == "models")), R::Models),
        ("T", count(&|c| c.marks.tested), R::Tests),
        ("S", count(&|c| c.pins.as_ref().is_some_and(|(state, _)| state == "pinned")), R::Pins),
        ("D", count(&|c| c.tested), R::Drt),
    ];
    let said: Vec<(String, Role)> = counts
        .iter()
        .map(|&(letter, held, role)| match modelled.len() {
            0 => (format!("{letter}·"), Role::Plain),
            of => (format!("{letter}{held}/{of}"), Role::Claim { role }),
        })
        .collect();
    let mut pieces: Vec<(&str, Role, &[&str])> = vec![("marks      ", Role::Plain, &[])];
    for (n, (text, role)) in said.iter().enumerate() {
        if n > 0 {
            pieces.push((" ", Role::Plain, &[]));
        }
        pieces.push((text.as_str(), *role, &[]));
    }
    out.line(&pieces);
}

/// A count's colour: whole in green, short in red, nothing to run in neither.
pub fn covered_role(run: u64, all: u64) -> Role {
    match (run, all) {
        (_, 0) => Role::Plain,
        _ if run == all => Role::Added,
        _ => Role::Removed,
    }
}

/// A coverage line: the requirement or clause, which opens the lines behind
/// the count, then the count — whole in green, short in red.
fn covered_line(out: &mut Lines, lead: &str, name: &str, (run, all, tests): (u64, u64, u64)) {
    let role = covered_role(run, all);
    let said = covered_text(run, all, tests);
    out.line(&[(lead, Role::Plain, &[]), (name, Role::Requirement, &["trace.coverage"]), ("  ", Role::Plain, &[]), (&said, role, &[])]);
}

/// A link as the view writes it: what a person reads and a click opens.
pub fn link_text(path: &str, line: u32) -> String {
    format!("{path}:{line}")
}

/// The requirement as a buffer, titled `requirement <id>`.
///
/// @implements REQ-SHOW.requirement_opened
/// @implements REQ-LINECOV.requirement_summary
/// @drt REQ-SHOW.requirement_opened
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
    // What the document says: opening a requirement shows its text, not only
    // its title, status and clauses.
    if !view.body.is_empty() {
        out.blank();
        for paragraph in view.body.split('\n') {
            out.wrapped("", paragraph, Role::Plain, &[]);
        }
        out.blank();
    }
    // How many clauses each kind of claim reaches, in the claims' colours: where
    // this requirement is implemented, tested, modelled and proved, at a glance.
    let total = view.clauses.len();
    // The order is the one the marks line below it keeps: implements, proves,
    // models, tests, strong, drt.
    let reaching = |role: crate::trace::annotation::Role| {
        view.clauses
            .iter()
            .filter(|c| {
                c.claims.iter().any(|claim| claim.role == role.as_str())
                    || (role == crate::trace::annotation::Role::Drt && c.tested)
            })
            .count()
    };
    let pinned = view.clauses.iter().filter(|c| c.pins.as_ref().is_some_and(|(state, _)| state == "pinned")).count();
    use crate::trace::annotation::Role as R;
    let counts: Vec<(R, String)> = vec![
        (R::Implements, format!("implements {}/{total}", reaching(R::Implements))),
        (R::Proves, format!("proves {}/{total}", reaching(R::Proves))),
        (R::Models, format!("models {}/{total}", reaching(R::Models))),
        (R::Tests, format!("tests {}/{total}", reaching(R::Tests))),
        // How many clauses Lean has said their specification pins: strong.
        (R::Pins, format!("strong {pinned}/{total}")),
        (R::Drt, format!("drt {}/{total}", reaching(R::Drt))),
    ];
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
    marks_line(&mut out, &view.clauses);
    match view.lines {
        Some(lines) => covered_line(&mut out, "covered    ", &view.id, lines),
        // Code claims it and nothing measured the code as it is: said, so an
        // absent count is not read as nothing to count.
        None if view.clauses.iter().any(implemented) => out.line(&[
            ("covered    ", Role::Plain, &[]),
            (NOT_MEASURED, Role::Removed, &[]),
        ]),
        None => {}
    }
    for clause in &view.clauses {
        out.blank();
        let key = clause.key.clone().unwrap_or_else(|| "(the whole requirement)".into());
        let letters = mark_pieces(clause.marks);
        // A mark that cannot be held is plain, and offers nothing.
        let mut row: Vec<(&str, Role, &[&str])> = letters
            .iter()
            .map(|(text, role)| {
                let actions: &[&str] = if *role == Role::Plain { &[] } else { &["trace.rollup"] };
                (text.as_str(), *role, actions)
            })
            .collect();
        row.push(("  ", Role::Plain, &[]));
        row.push((&key, Role::Heading, &[]));
        out.line(&row);
        out.wrapped("  ", &clause.text, Role::Plain, &[]);
        // Narrowings belong to the clause: indented under its text, never
        // given a level or a line of their own in the counts above.
        for (key, text) in &clause.narrowings {
            out.wrapped("    ", &format!("{key}: {text}"), Role::Plain, &[]);
        }
        if let Some(key) = &clause.key {
            let name = format!("{}.{key}", view.id);
            out.line(&[("  for agent   ", Role::Plain, &[]), (&name, Role::Requirement, &["trace.context"])]);
        }
        if clause.claims.is_empty() {
            out.line(&[("  nothing claims it yet", Role::Removed, &[])]);
        }
        // How much of the code that implements it the tests run.
        match clause.lines {
            Some(lines) => {
                let name = match &clause.key {
                    Some(key) => format!("{}.{key}", view.id),
                    None => view.id.clone(),
                };
                covered_line(&mut out, "  covered     ", &name, lines);
            }
            None if implemented(clause) => out.line(&[("  covered     ", Role::Plain, &[]), (NOT_MEASURED, Role::Removed, &[])]),
            None => {}
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
        // What was judged: an agreement plainly, a drift as a fault, and a
        // delegated verdict with the person who delegated it.
        if let Some(judged) = &clause.judged {
            let said = judged_text(judged.clone());
            if judged.verdict == "agrees" {
                out.line(&[("  judged      ", Role::Plain, &[]), (&said, Role::Plain, &[])]);
            } else {
                out.wrapped("  ", &said, Role::Removed, &[]);
            }
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
            body: "The prose of the document.".into(),
            clauses: vec![
                ClauseShown {
                    key: Some("holds".into()),
                    text: "It shall hold.".into(),
                    narrowings: vec![("empty".into(), "With nothing, it holds.".into())],
                    marks: Marks { annotated: true, judged: true, tested: true, ..Marks::NONE },
                    claims: vec![Claim { role: "implements".into(), path: "src/x.rs".into(), line: 12, symbol: Some("hold".into()) }],
                    pins: Some(("pinned".into(), "X.holds_pinned".into())),
                    tested: true,
                    lines: Some((3, 4, 2)),
                    judged: Some(Judged {
                        verdict: "agrees".into(),
                        judged_by: "claude-review".into(),
                        delegated_by: Some("ana".into()),
                        note: None,
                        level: Level::L2,
                    }),
                },
                ClauseShown {
                    key: Some("unclaimed".into()),
                    text: "Nobody does this.".into(),
                    narrowings: vec![],
                    marks: Marks::NONE,
                    claims: vec![],
                    pins: None,
                    tested: false,
                    lines: None,
                    judged: Some(Judged {
                        verdict: "drift".into(),
                        judged_by: "ana".into(),
                        delegated_by: None,
                        note: Some("the model rounds".into()),
                        level: Level::L1,
                    }),
                },
            ],
            width: 60,
            lines: Some((3, 8, 2)),
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
        assert!(shown.text.contains("The prose of the document."), "the body is not shown: {}", shown.text);
        assert!(shown.text.contains("  pinned\n    X.holds_pinned"), "{}", shown.text);
        assert!(shown.text.contains("drt 1/2"), "a derived test counts: {}", shown.text);
        assert!(shown.text.contains("strong 1/2 drt 1/2"), "a pinned clause counts: {}", shown.text);
        // Unmeasured, the count says so rather than vanishing.
        let mut unmeasured = view();
        unmeasured.lines = None;
        unmeasured.clauses[0].lines = None;
        let said = requirement_view(unmeasured).text;
        assert!(said.contains(&format!("covered    {NOT_MEASURED}")), "{said}");
        assert!(said.contains(&format!("  covered     {NOT_MEASURED}")), "{said}");
        assert!(shown.text.contains("covered     REQ-X.holds  3/4 lines run (75%), by 2 tests"), "{}", shown.text);
        // The requirement's own count, over all of it; each opens its lines.
        assert!(shown.text.contains("covered    REQ-X  3/8 lines run (37%), by 2 tests"), "{}", shown.text);
        let opens = |needle: &str| actions_at(shown.clone(), offset_of(&shown, needle));
        assert_eq!(opens("REQ-X.holds  3/4"), vec!["trace.coverage".to_string()]);
        assert_eq!(opens("REQ-X  3/8"), vec!["trace.coverage".to_string()]);
        assert!(shown.text.find("holds").unwrap() < shown.text.find("unclaimed").unwrap());
        assert!(shown.text.contains("AJT-  holds") && shown.text.contains("----  unclaimed"), "{}", shown.text);
    }

    /// A narrowing is shown indented under its clause's text, and is not
    /// counted as a clause.
    ///
    /// @tests REQ-REQDOC.narrowings_nest
    #[test]
    fn narrowings_are_indented_under_their_clause() {
        let shown = requirement_view(view());
        assert!(
            shown.text.contains("  It shall hold.\n    empty: With nothing, it holds.\n"),
            "{}",
            shown.text
        );
        assert!(shown.text.contains("implements 1/2"), "{}", shown.text);
    }

    /// A delegated agreement names who delegated it; a drift is shown as one,
    /// with its note, in the fault colour.
    ///
    /// @tests REQ-JUDGE.judgement_shown
    #[test]
    fn a_judgement_shows_its_verdict_and_who_delegated_it() {
        let shown = requirement_view(view());
        assert!(
            shown.text.contains("judged      agrees by claude-review — L2 (delegated by ana)"),
            "{}",
            shown.text
        );
        let drift = offset_of(&shown, "judged: drift — the model rounds");
        assert!(shown.text.contains("judged: drift — the model rounds  by ana"), "{}", shown.text);
        let role = shown.spans.iter().find(|s| s.start <= drift && drift < s.stop).map(|s| s.role);
        assert_eq!(role, Some(Role::Removed), "a drift is not shown as a fault");
    }

    /// Measured code with no line a test could run says so, uncoloured,
    /// rather than that all of it ran.
    #[test]
    fn nothing_to_run_is_not_all_of_it_run() {
        assert_eq!(covered_text(0, 0, 0), NO_LINES);
        assert_eq!(covered_role(0, 0), Role::Plain);
        assert_eq!((covered_role(2, 2), covered_role(1, 2)), (Role::Added, Role::Removed));
    }

    #[test]
    fn a_link_splits_into_its_path_and_line() {
        assert_eq!(split_link("src/x.rs:12"), ("src/x.rs", Some(12)));
        assert_eq!(split_link("src/x.rs"), ("src/x.rs", None));
        assert_eq!(split_link("a:b"), ("a:b", None));
    }
}
