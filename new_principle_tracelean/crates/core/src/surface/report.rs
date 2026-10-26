use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

use crate::evidence::Mark;
use crate::trace::anchor::AnchorKind;
use crate::trace::checker::Finding;
use crate::trace::diff::{said, Change, ClauseState};
use crate::trace::index::{Index, Link};
use crate::trace::lockfile::Lockfile;
use crate::trace::proofcheck::{ProofReport, Shown, Verdict};
use crate::trace::record::Detail;

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CardFacts {
    pub held: Vec<Mark>,
    pub structural: bool,
    pub roles: Vec<String>,
    pub refused: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum Missing {
    NotAnnotated,
    NoModel,
    NotJudged,
    NotImplemented,
    Unbound,
    NotAgreed,
    NotApplicable,
    NoTheorem,
    AllRefused { reasons: Vec<String> },
    NotChecked,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Reason {
    pub mark: Mark,
    pub missing: Missing,
}

const ALL_MARKS: [Mark; 4] = [Mark::Annotated, Mark::Judged, Mark::Tested, Mark::Proved];

fn missing_for(facts: &CardFacts, mark: Mark) -> Missing {
    let has = |role: &str| facts.roles.iter().any(|r| r == role);
    match mark {
        Mark::Annotated => Missing::NotAnnotated,
        Mark::Judged if has("models") => Missing::NotJudged,
        Mark::Judged => Missing::NoModel,
        _ if facts.structural => Missing::NotApplicable,
        _ if !has("models") => Missing::NoModel,
        Mark::Tested if !has("implements") => Missing::NotImplemented,
        Mark::Tested if !has("drt") => Missing::Unbound,
        Mark::Tested => Missing::NotAgreed,
        _ if !has("proves") => Missing::NoTheorem,
        _ if facts.refused.is_empty() => Missing::NotChecked,
        _ => Missing::AllRefused { reasons: facts.refused.clone() },
    }
}

/// @implements REQ-REPORT.why_not_held
/// @drt REQ-REPORT.why_not_held
pub fn reasons(facts: CardFacts) -> Vec<Reason> {
    ALL_MARKS
        .iter()
        .filter(|m| !facts.held.contains(m))
        .map(|&mark| Reason { mark, missing: missing_for(&facts, mark) })
        .collect()
}

pub fn explain(missing: &Missing) -> String {
    match missing {
        Missing::NotAnnotated => "no annotation names this clause".into(),
        Missing::NoModel => "no Lean model says what it means".into(),
        Missing::NotJudged => "nobody has judged the model against the clause as they stand now (--judge)".into(),
        Missing::NotImplemented => "no code implements it".into(),
        Missing::Unbound => "model and code exist, but nothing compares them".into(),
        Missing::NotAgreed => "no differential run agrees with the model and code as they stand now (--drt)".into(),
        Missing::NotApplicable => "not applicable: the clause is about the tree, and a test that reads it answers it".into(),
        Missing::NoTheorem => "no theorem claims it (@proves)".into(),
        Missing::AllRefused { reasons } => format!("Lean refused every theorem: {}", reasons.join("; ")),
        Missing::NotChecked => "no accepted proof for the model as it stands now (--proofs)".into(),
    }
}

pub struct Shared<'a> {
    pub index: &'a Index,
    pub lock: &'a Lockfile,
    pub proofs: &'a ProofReport,
    pub strengths: &'a BTreeMap<(String, Option<String>), Shown>,
    pub findings: &'a [Finding],
    pub files: &'a BTreeMap<String, String>,
    pub changes: &'a BTreeMap<(String, Option<String>), Vec<Change>>,
    pub base: Option<&'a str>,
}

fn esc(text: &str) -> String {
    text.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

fn symbol(link: &Link) -> String {
    match &link.anchor.kind {
        AnchorKind::Decl { symbol_path } => symbol_path.clone(),
        AnchorKind::Region { .. } => format!("region at line {}", link.anchor.start_line + 1),
        AnchorKind::File => "the whole file".into(),
    }
}

fn source(shared: &Shared, link: &Link) -> String {
    let Some(text) = shared.files.get(&link.anchor.file) else { return String::new() };
    let lines: Vec<&str> = text.lines().collect();
    let from = (link.anchor.start_line as usize).min(lines.len());
    let to = (link.anchor.end_line as usize + 1).min(lines.len()).max(from);
    lines[from..to].join("\n")
}

fn place(link: &Link) -> String {
    format!("{}:{}", link.anchor.file, link.anchor.start_line + 1)
}

fn letter(mark: Mark) -> char {
    match mark {
        Mark::Annotated => 'A',
        Mark::Judged => 'J',
        Mark::Tested => 'T',
        Mark::Proved => 'P',
    }
}

fn linked(shared: &Shared, links: &[&Link], role: &str, title: &str, with_source: bool, out: &mut String) {
    let here: Vec<&&Link> = links.iter().filter(|l| l.role.as_str() == role).collect();
    if here.is_empty() {
        return;
    }
    out.push_str(&format!("<div class=\"ev\"><h4>{title}</h4><ul>"));
    for link in here {
        out.push_str(&format!("<li><code>{}</code> <span class=\"at\">{}</span>", esc(&symbol(link)), esc(&place(link))));
        if with_source {
            out.push_str(&format!("<details><summary>source</summary><pre>{}</pre></details>", esc(&source(shared, link))));
        }
        out.push_str("</li>");
    }
    out.push_str("</ul></div>");
}

/// @implements REQ-REPORT.card_per_clause
/// @implements REQ-REPORT.evidence_on_card
/// @implements REQ-REPORT.findings_on_card
/// @implements REQ-REPORT.changes_marked
fn card(shared: &Shared, state: &ClauseState) -> String {
    let key = (state.req_id.clone(), state.clause.clone());
    let op = crate::drt::qualified_op(&state.req_id, state.clause.as_deref());
    let links: Vec<&Link> =
        shared.index.links.iter().filter(|l| l.req_id == state.req_id && l.clause == state.clause).collect();
    let structural = links.iter().any(|l| l.is_structural()) && !links.iter().any(|l| l.role.as_str() == "models");
    let refused: Vec<String> = shared
        .proofs
        .proofs
        .iter()
        .filter(|p| p.req_id == state.req_id && p.clause == state.clause)
        .filter_map(|p| match &p.verdict {
            Verdict::Refused { refusal } => Some(format!("{}: {}", p.theorem, refusal.message())),
            Verdict::Accepted { .. } => None,
        })
        .collect();
    let mut roles: Vec<String> = links.iter().map(|l| l.role.as_str().to_string()).collect();
    roles.sort();
    roles.dedup();
    let why = reasons(CardFacts { held: state.held.clone(), structural, roles, refused });
    let findings: Vec<&Finding> = shared
        .findings
        .iter()
        .filter(|f| f.req_id.as_deref() == Some(state.req_id.as_str()) && f.clause == state.clause)
        .collect();
    let changes = shared.changes.get(&key);
    let lost = changes.is_some_and(|c| c.iter().any(Change::is_loss));

    let mut classes = vec!["card".to_string()];
    for mark in ALL_MARKS {
        if !state.held.contains(&mark) && !(structural && matches!(mark, Mark::Tested | Mark::Proved)) {
            classes.push(format!("no{}", letter(mark)));
        }
    }
    if changes.is_some() {
        classes.push("changed".into());
    }
    if lost {
        classes.push("lost".into());
    }
    if !findings.is_empty() {
        classes.push("finding".into());
    }
    let mut out = format!("<article class=\"{}\" id=\"{}\" data-search=\"{}\">", classes.join(" "), esc(&op), esc(&format!("{op} {}", state.text).to_lowercase()));
    out.push_str(&format!("<header><a class=\"op\" href=\"#{0}\">{0}</a><span class=\"marks\">", esc(&op)));
    for mark in ALL_MARKS {
        let held = state.held.contains(&mark);
        let na = structural && matches!(mark, Mark::Tested | Mark::Proved);
        let class = if held { "on" } else if na { "na" } else { "off" };
        let tip = why.iter().find(|r| r.mark == mark).map(|r| explain(&r.missing)).unwrap_or_else(|| "held".into());
        out.push_str(&format!("<span class=\"m {class}\" title=\"{}\">{}</span>", esc(&tip), letter(mark)));
    }
    out.push_str("</span></header>");
    out.push_str(&format!("<p class=\"text\">{}</p>", esc(&state.text)));

    if let Some(changes) = changes {
        out.push_str(&format!(
            "<div class=\"changes\"><h4>Changed since {}</h4><ul>",
            esc(shared.base.unwrap_or("the base"))
        ));
        for change in changes {
            let class = if change.is_loss() || matches!(change, Change::AssumptionAdded { .. }) { " class=\"bad\"" } else { "" };
            out.push_str(&format!("<li{class}><pre>{}</pre></li>", esc(&said(change))));
        }
        out.push_str("</ul></div>");
    }

    let shown: Vec<&Reason> = why.iter().filter(|r| r.missing != Missing::NotApplicable).collect();
    if !shown.is_empty() {
        out.push_str("<div class=\"why\"><h4>Not held</h4><ul>");
        for reason in shown {
            out.push_str(&format!("<li><b>{}</b> {}</li>", letter(reason.mark), esc(&explain(&reason.missing))));
        }
        out.push_str("</ul></div>");
    }

    if !findings.is_empty() {
        out.push_str("<div class=\"findings\"><h4>Findings</h4><ul>");
        for f in &findings {
            out.push_str(&format!(
                "<li><b>{:?}</b> {} <span class=\"at\">{}:{}</span></li>",
                f.kind,
                esc(&f.message),
                esc(&f.file),
                f.line
            ));
        }
        out.push_str("</ul></div>");
    }

    out.push_str("<details class=\"evidence\"><summary>Evidence</summary>");
    linked(shared, &links, "models", "Model", true, &mut out);
    linked(shared, &links, "specifies", "Specification", true, &mut out);
    linked(shared, &links, "implements", "Implemented by", false, &mut out);
    linked(shared, &links, "drt", "Compared by", false, &mut out);
    linked(shared, &links, "tests", "Tests", false, &mut out);

    let theorems: Vec<&Link> = links.iter().copied().filter(|l| matches!(l.role.as_str(), "proves" | "pins")).collect();
    if !theorems.is_empty() {
        out.push_str("<div class=\"ev\"><h4>Theorems</h4><ul>");
        for link in theorems {
            let name = symbol(link);
            let verdict = shared
                .proofs
                .proofs
                .iter()
                .find(|p| p.theorem == name && p.req_id == state.req_id && p.clause == state.clause)
                .map(|p| match &p.verdict {
                    Verdict::Accepted { assumed } if assumed.is_empty() => "<span class=\"ok\">accepted</span>".to_string(),
                    Verdict::Accepted { assumed } => {
                        format!("<span class=\"ok\">accepted</span>, assuming <code>{}</code>", esc(&assumed.join(", ")))
                    }
                    Verdict::Refused { refusal } => format!("<span class=\"bad\">refused</span>: {}", esc(&refusal.message())),
                })
                .unwrap_or_else(|| if link.role.as_str() == "pins" { "pins its model".into() } else { "not checked".into() });
            out.push_str(&format!(
                "<li>@{} <code>{}</code> {verdict} <span class=\"at\">{}</span><details><summary>statement and proof</summary><pre>{}</pre></details></li>",
                link.role.as_str(),
                esc(&name),
                esc(&place(link)),
                esc(&source(shared, link))
            ));
        }
        out.push_str("</ul></div>");
    }

    if let Some(shown) = shared.strengths.get(&key) {
        let (state_name, detail) = crate::trace::proofcheck::described(shown);
        out.push_str(&format!(
            "<div class=\"ev\"><h4>Strength</h4><p><b>{}</b> {}</p>",
            esc(&state_name),
            esc(&detail)
        ));
        if !shown.statement.is_empty() {
            out.push_str(&format!(
                "<details><summary>obligation for <code>{}</code></summary><pre>{}</pre></details>",
                esc(&shown.model),
                esc(&shown.statement)
            ));
        }
        out.push_str("</div>");
    }

    let judged = shared.lock.evidence.iter().find_map(|r| match &r.detail {
        Detail::Judge { verdict, judged_by, delegated_by, note, .. }
            if r.key.req_id == state.req_id && r.key.clause == state.clause =>
        {
            Some((verdict.clone(), judged_by.clone(), delegated_by.clone(), note.clone()))
        }
        _ => None,
    });
    if let Some((verdict, by, delegated, note)) = judged {
        let delegated = delegated.map(|d| format!(", for {}", esc(&d))).unwrap_or_default();
        out.push_str(&format!(
            "<div class=\"ev\"><h4>Judgement</h4><p><b>{}</b> by {}{delegated}{}</p></div>",
            esc(&verdict),
            esc(&by),
            note.map(|n| format!(": {}", esc(&n))).unwrap_or_default()
        ));
    }
    out.push_str("</details></article>");
    out
}

const STYLE: &str = r#"
:root{--bg:#fbfbfa;--fg:#1d1d1b;--muted:#6b6a66;--card:#fff;--line:#e3e1dc;--on:#1f7a4d;--off:#b3261e;--na:#9a988f;--chg:#8a5a00;--chgbg:#fff6e0;--code:#f3f2ee}
@media (prefers-color-scheme:dark){:root{--bg:#161615;--fg:#ecebe7;--muted:#a3a19a;--card:#1f1f1d;--line:#34332f;--on:#5cc28f;--off:#f08a80;--na:#77756e;--chg:#f0c060;--chgbg:#2c2614;--code:#262623}}
*{box-sizing:border-box}body{margin:0;background:var(--bg);color:var(--fg);font:15px/1.5 system-ui,-apple-system,"Segoe UI",sans-serif}
main{max-width:1000px;margin:0 auto;padding:16px}
h1{font-size:22px;margin:8px 0}h2{font-size:17px;margin:28px 0 8px;border-bottom:1px solid var(--line);padding-bottom:4px}h2 small{color:var(--muted);font-weight:400}
h4{margin:10px 0 4px;font-size:12px;text-transform:uppercase;letter-spacing:.04em;color:var(--muted)}
.summary{display:flex;flex-wrap:wrap;gap:8px;margin:8px 0 12px}.summary span{background:var(--card);border:1px solid var(--line);border-radius:6px;padding:4px 10px}
.filters{position:sticky;top:0;background:var(--bg);padding:8px 0;display:flex;flex-wrap:wrap;gap:10px;align-items:center;z-index:1;border-bottom:1px solid var(--line)}
.filters input[type=search]{flex:1 1 200px;padding:6px 8px;border:1px solid var(--line);border-radius:6px;background:var(--card);color:var(--fg)}
.card{background:var(--card);border:1px solid var(--line);border-radius:8px;padding:10px 14px;margin:10px 0}
.card.changed{border-left:4px solid var(--chg)}.card.lost{border-left:4px solid var(--off)}
header{display:flex;justify-content:space-between;gap:8px;align-items:baseline;flex-wrap:wrap}
.op{font-family:ui-monospace,SFMono-Regular,Menlo,monospace;font-weight:600;color:var(--fg);text-decoration:none;word-break:break-all}
.marks{display:flex;gap:4px}.m{display:inline-block;width:24px;text-align:center;border-radius:4px;font:600 13px/22px ui-monospace,monospace;border:1px solid}
.m.on{background:var(--on);border-color:var(--on);color:#fff}.m.off{color:var(--off);border-color:var(--off)}.m.na{color:var(--na);border-color:var(--na);border-style:dashed}
.text{margin:6px 0}.at{color:var(--muted);font-size:12px;word-break:break-all}
ul{margin:4px 0;padding-left:20px}pre{background:var(--code);padding:8px;border-radius:6px;overflow-x:auto;font-size:12.5px;margin:4px 0;white-space:pre-wrap}
code{font-family:ui-monospace,monospace;font-size:13px;word-break:break-all}
.changes{background:var(--chgbg);border-radius:6px;padding:4px 10px}.changes pre{background:transparent;padding:0}
.bad,.changes li.bad pre{color:var(--off);font-weight:600}.ok{color:var(--on);font-weight:600}
.why li b,.findings li b{font-family:ui-monospace,monospace}
details summary{cursor:pointer;color:var(--muted);font-size:13px}.evidence{margin-top:6px}
.hidden{display:none}
"#;

const SCRIPT: &str = r#"
const q=document.getElementById('q');const boxes=[...document.querySelectorAll('.filters input[type=checkbox]')];
function apply(){const t=q.value.toLowerCase();const need=boxes.filter(b=>b.checked).map(b=>b.value);
document.querySelectorAll('.card').forEach(c=>{const ok=(!t||c.dataset.search.includes(t))&&need.every(n=>c.classList.contains(n));c.classList.toggle('hidden',!ok)});
document.querySelectorAll('section').forEach(s=>s.classList.toggle('hidden',!s.querySelector('.card:not(.hidden)')));}
q.addEventListener('input',apply);boxes.forEach(b=>b.addEventListener('change',apply));
"#;

/// @implements REQ-REPORT.static_page
pub fn page(shared: &Shared, states: &[ClauseState]) -> String {
    let total = states.len();
    let count = |mark: Mark| states.iter().filter(|s| s.held.contains(&mark)).count();
    let mut out = String::from("<!doctype html><html lang=\"en\"><head><meta charset=\"utf-8\"><meta name=\"viewport\" content=\"width=device-width,initial-scale=1\"><title>TraceLean report</title><style>");
    out.push_str(STYLE);
    out.push_str("</style></head><body><main><h1>Requirements and their evidence</h1>");
    out.push_str("<div class=\"summary\">");
    out.push_str(&format!("<span>{total} clauses</span>"));
    for mark in ALL_MARKS {
        out.push_str(&format!("<span><b>{}</b> {} of {total}</span>", letter(mark), count(mark)));
    }
    if let Some(base) = shared.base {
        let lost = shared.changes.values().filter(|c| c.iter().any(Change::is_loss)).count();
        out.push_str(&format!(
            "<span>{} changed since <code>{}</code>, {lost} with a loss</span>",
            shared.changes.len(),
            esc(base)
        ));
    }
    out.push_str(&format!("<span>{} findings</span></div>", shared.findings.len()));
    out.push_str("<div class=\"filters\"><input id=\"q\" type=\"search\" placeholder=\"Search clauses\">");
    for (value, label) in [("noJ", "not judged"), ("noT", "not tested"), ("noP", "not proved"), ("finding", "with findings")] {
        out.push_str(&format!("<label><input type=\"checkbox\" value=\"{value}\"> {label}</label>"));
    }
    if shared.base.is_some() {
        out.push_str("<label><input type=\"checkbox\" value=\"changed\"> changed</label><label><input type=\"checkbox\" value=\"lost\"> lost a mark</label>");
    }
    out.push_str("</div>");
    let gone: Vec<(&(String, Option<String>), &Vec<Change>)> = shared
        .changes
        .iter()
        .filter(|(key, _)| !states.iter().any(|s| s.req_id == key.0 && s.clause == key.1))
        .collect();
    if !gone.is_empty() {
        out.push_str("<section><h2>Removed <small>clauses that no longer exist</small></h2>");
        for ((req_id, clause), changes) in gone {
            let op = crate::drt::qualified_op(req_id, clause.as_deref());
            out.push_str(&format!(
                "<article class=\"card changed lost\" id=\"{0}\" data-search=\"{1}\"><header><span class=\"op\">{0}</span></header><div class=\"changes\"><ul>",
                esc(&op),
                esc(&op.to_lowercase())
            ));
            for change in changes {
                out.push_str(&format!("<li><pre>{}</pre></li>", esc(&said(change))));
            }
            out.push_str("</ul></div></article>");
        }
        out.push_str("</section>");
    }
    for (id, requirement) in &shared.index.requirements {
        let here: Vec<&ClauseState> = states.iter().filter(|s| s.req_id == *id).collect();
        if here.is_empty() {
            continue;
        }
        out.push_str(&format!(
            "<section><h2>{} <small>{} · {}</small></h2>",
            esc(id),
            esc(&requirement.title),
            esc(&requirement.file)
        ));
        for state in here {
            out.push_str(&card(shared, state));
        }
        out.push_str("</section>");
    }
    out.push_str("</main><script>");
    out.push_str(SCRIPT);
    out.push_str("</script></body></html>\n");
    out
}
