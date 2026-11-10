//! The overview as text: the same figures the window draws, as lines a terminal can show
//! and a Markdown reader can render. Nothing here reads a file; every input is a value.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

use crate::surface::produce::Node;

/// A directory of Rust source, the modules it names in its code, and the requirements
/// its files claim.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Module {
    pub name: String,
    pub files: usize,
    pub uses: Vec<String>,
    pub serves: Vec<String>,
}

/// One change an agent made and is waiting to have accepted, in the order it arrived.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Step {
    pub order: usize,
    pub kind: String,
    pub file: String,
    pub added: usize,
    pub removed: usize,
    /// Each clause the file claims, with the marks it holds now: `("REQ-A.a", "AJT-")`.
    pub clauses: Vec<(String, String)>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RunRow {
    pub op: String,
    pub clauses: usize,
    pub seed: u64,
    pub cases: u64,
    pub age: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RiskRow {
    pub clause: String,
    pub marks: String,
    pub file: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Figures {
    pub nodes: Vec<Node>,
    pub findings: Option<usize>,
    pub runs: Vec<RunRow>,
    pub unsaved: Vec<RiskRow>,
    pub agent: Vec<Step>,
    pub modules: Vec<Module>,
}

fn directory(path: &str) -> &str {
    path.rsplit_once('/').map_or(".", |(dir, _)| dir)
}

fn is_name(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_'
}

/// The crate names declared by the `Cargo.toml` files given, each with the directory its
/// source lives in: `("tracelean_core", "crates/core/src")`.
fn crates_of(files: &[(String, String)]) -> Vec<(String, String)> {
    let mut found = Vec::new();
    for (path, text) in files {
        if path != "Cargo.toml" && !path.ends_with("/Cargo.toml") {
            continue;
        }
        let mut in_package = false;
        for line in text.lines() {
            let line = line.trim();
            if line.starts_with('[') {
                in_package = line == "[package]";
                continue;
            }
            let Some(rest) = line.strip_prefix("name").filter(|_| in_package) else { continue };
            let Some(value) = rest.trim_start().strip_prefix('=') else { continue };
            let name = value.trim().trim_matches('"').replace('-', "_");
            let home = match directory(path) {
                "." => "src".to_string(),
                dir => format!("{dir}/src"),
            };
            found.push((name, home));
            break;
        }
    }
    found
}

/// Every `name::segment` in `text` where `name` stands alone, as the segment that follows.
fn segments_after<'a>(text: &'a str, name: &str) -> Vec<&'a str> {
    let mut found = Vec::new();
    let lead = format!("{name}::");
    let mut from = 0;
    while let Some(at) = text[from..].find(&lead) {
        let start = from + at;
        let after = start + lead.len();
        from = after;
        if text[..start].chars().next_back().is_some_and(|c| is_name(c) || c == ':') {
            continue;
        }
        let stop = text[after..].find(|c: char| !is_name(c)).map_or(text.len(), |n| after + n);
        if stop > after {
            found.push(&text[after..stop]);
        }
    }
    found
}

/// The module dependency graph: one module per directory holding Rust files, an edge to
/// every other module its code names through `crate::` or another crate of the tree, and
/// the requirements its files claim. Comments are not code and name nothing; a file
/// under a directory whose name starts with a dot is not part of the tree.
///
/// @implements REQ-MODULES.one_per_directory
/// @implements REQ-MODULES.edges_from_paths
/// @implements REQ-MODULES.serves_from_claims
pub fn modules(files: &[(String, String)], claims: &[(String, String)]) -> Vec<Module> {
    let crates = crates_of(files);
    let mut graph: BTreeMap<String, Module> = BTreeMap::new();
    let hidden = |path: &str| path.split('/').any(|part| part.starts_with('.') && part.len() > 1);
    let sources: Vec<&(String, String)> =
        files.iter().filter(|(path, _)| path.ends_with(".rs") && !hidden(path)).collect();
    for (path, _) in &sources {
        let name = directory(path).to_string();
        graph.entry(name.clone()).or_insert(Module { name, files: 0, uses: Vec::new(), serves: Vec::new() }).files += 1;
    }
    let names: BTreeSet<String> = graph.keys().cloned().collect();
    let resolve = |home: &str, segment: &str| -> Option<String> {
        let inner = format!("{home}/{segment}");
        if names.contains(&inner) {
            Some(inner)
        } else if names.contains(home) {
            Some(home.to_string())
        } else {
            None
        }
    };
    for (path, text) in &sources {
        let here = directory(path).to_string();
        let own = crates
            .iter()
            .filter(|(_, home)| here == *home || here.starts_with(&format!("{home}/")))
            .max_by_key(|(_, home)| home.len())
            .map(|(_, home)| home.clone());
        let code: String = text
            .lines()
            .map(|line| line.split("//").next().unwrap_or(""))
            .collect::<Vec<_>>()
            .join("\n");
        let mut used: BTreeSet<String> = BTreeSet::new();
        if let Some(home) = &own {
            used.extend(segments_after(&code, "crate").into_iter().filter_map(|s| resolve(home, s)));
        }
        for (name, home) in &crates {
            used.extend(segments_after(&code, name).into_iter().filter_map(|s| resolve(home, s)));
        }
        used.remove(&here);
        let module = graph.get_mut(&here).expect("every source file has its module");
        let mut all: BTreeSet<String> = module.uses.iter().cloned().collect();
        all.extend(used);
        module.uses = all.into_iter().collect();
    }
    for (file, req_id) in claims {
        if let Some(module) = graph.get_mut(directory(file)) {
            if file.ends_with(".rs") && !module.serves.contains(req_id) {
                module.serves.push(req_id.clone());
            }
        }
    }
    for module in graph.values_mut() {
        module.serves.sort();
    }
    graph.into_values().collect()
}

/// The modules that claim no requirement at all: code nothing asked for, or nothing says so.
///
/// @implements REQ-MODULES.unclaimed_flagged
pub fn serving_nothing(modules: &[Module]) -> Vec<String> {
    modules.iter().filter(|m| m.serves.is_empty()).map(|m| m.name.clone()).collect()
}

fn share(part: usize, whole: usize) -> usize {
    if whole == 0 {
        0
    } else {
        (100 * part + whole / 2) / whole
    }
}

fn bar(part: usize, whole: usize) -> String {
    let filled = if whole == 0 { 0 } else { (20 * part + whole / 2) / whole };
    format!("{}{}", "█".repeat(filled), "░".repeat(20 - filled.min(20)))
}

/// How long ago, in the largest whole unit.
pub fn ago(seconds: Option<u64>) -> String {
    let Some(seconds) = seconds else { return "–".to_string() };
    for (size, unit) in [(86400, "d"), (3600, "h"), (60, "min")] {
        if seconds >= size {
            return format!("{} {unit} ago", seconds / size);
        }
    }
    "just now".to_string()
}

fn held(node: &Node) -> [usize; 5] {
    [node.held.implemented, node.held.annotated, node.held.judged, node.held.tested, node.held.proved]
}

/// How many marks a requirement's clauses still lack, over all five marks.
fn left(node: &Node) -> usize {
    held(node).iter().map(|have| node.clauses.saturating_sub(*have)).sum()
}

const MARKS: [(&str, &str); 5] =
    [("I", "implemented"), ("A", "annotated"), ("J", "judged"), ("T", "tested"), ("P", "proved")];

fn names(list: &[String]) -> String {
    if list.is_empty() {
        "–".to_string()
    } else {
        list.join(", ")
    }
}

/// The module table alone, as Markdown lines.
///
/// @implements REQ-MODULES.shown_as_text
pub fn module_lines(modules: &[Module]) -> Vec<String> {
    let mut out = vec!["## Modules".to_string(), String::new()];
    if modules.is_empty() {
        out.push("No Rust source here.".to_string());
        return out;
    }
    out.push("| module | files | uses | serves |".to_string());
    out.push("|---|---|---|---|".to_string());
    for module in modules {
        out.push(format!("| {} | {} | {} | {} |", module.name, module.files, names(&module.uses), names(&module.serves)));
    }
    let idle = serving_nothing(modules);
    if !idle.is_empty() {
        out.push(String::new());
        out.push(format!("Claiming no requirement: {}", idle.join(", ")));
    }
    out
}

/// The overview as Markdown: the counts, how far the clauses got mark by mark, where work
/// is left (most left first), the runs on record, what unsaved edits and an agent's
/// waiting changes touch, and the modules. A section with nothing to say is left out,
/// except the first three, which always stand.
///
/// @implements REQ-FIGURE.overview_as_text
/// @implements REQ-FIGURE.agent_steps_listed
pub fn overview(figures: &Figures) -> Vec<String> {
    let nodes = &figures.nodes;
    let clauses: usize = nodes.iter().map(|n| n.clauses).sum();
    let done = nodes.iter().filter(|n| held(n).iter().all(|have| *have >= n.clauses)).count();
    let findings = figures.findings.map_or("–".to_string(), |n| n.to_string());
    let mut out = vec![
        "# Overview".to_string(),
        String::new(),
        format!(
            "{} requirements · {clauses} clauses · {done}/{} requirements with every mark · {findings} findings",
            nodes.len(),
            nodes.len()
        ),
        String::new(),
        "## How far the clauses got".to_string(),
        String::new(),
        "| mark | clauses | share | |".to_string(),
        "|---|---|---|---|".to_string(),
    ];
    for (at, (letter, name)) in MARKS.iter().enumerate() {
        let have: usize = nodes.iter().map(|n| held(n)[at]).sum();
        out.push(format!("| {letter} {name} | {have}/{clauses} | {}% | {} |", share(have, clauses), bar(have, clauses)));
    }
    out.extend([String::new(), "## Where work is left".to_string(), String::new()]);
    out.push("| requirement | I | A | J | T | P | marks left |".to_string());
    out.push("|---|---|---|---|---|---|---|".to_string());
    let mut order: Vec<&Node> = nodes.iter().collect();
    order.sort_by(|a, b| left(b).cmp(&left(a)).then(a.id.cmp(&b.id)));
    for node in order {
        let cells: Vec<String> = held(node).iter().map(|have| format!("{have}/{}", node.clauses)).collect();
        out.push(format!("| {} | {} | {} |", node.id, cells.join(" | "), left(node)));
    }
    if !figures.runs.is_empty() {
        out.extend([String::new(), "## Test runs on record".to_string(), String::new()]);
        out.push("| compared operation | clauses | cases | seed | recorded |".to_string());
        out.push("|---|---|---|---|---|".to_string());
        for run in &figures.runs {
            out.push(format!("| {} | {} | {} | {} | {} |", run.op, run.clauses, run.cases, run.seed, ago(run.age)));
        }
    }
    if !figures.unsaved.is_empty() {
        out.extend([String::new(), "## Your unsaved edits".to_string(), String::new()]);
        out.push("| clause | holds now | claimed by the unsaved file |".to_string());
        out.push("|---|---|---|".to_string());
        for row in &figures.unsaved {
            out.push(format!("| {} | {} | {} |", row.clause, row.marks, row.file));
        }
    }
    if !figures.agent.is_empty() {
        out.extend([String::new(), "## The agent's waiting changes".to_string(), String::new()]);
        out.push("| step | change | file | lines | clauses it claims (marks held) |".to_string());
        out.push("|---|---|---|---|---|".to_string());
        for step in &figures.agent {
            let claimed: Vec<String> = step.clauses.iter().map(|(name, marks)| format!("{name} ({marks})")).collect();
            out.push(format!(
                "| {} | {} | {} | +{} −{} | {} |",
                step.order,
                step.kind,
                step.file,
                step.added,
                step.removed,
                names(&claimed)
            ));
        }
    }
    if !figures.modules.is_empty() {
        out.push(String::new());
        out.extend(module_lines(&figures.modules));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::surface::produce::Tally;

    fn file(path: &str, text: &str) -> (String, String) {
        (path.to_string(), text.to_string())
    }

    fn claim(path: &str, req: &str) -> (String, String) {
        (path.to_string(), req.to_string())
    }

    fn find<'a>(all: &'a [Module], name: &str) -> &'a Module {
        all.iter().find(|m| m.name == name).unwrap_or_else(|| panic!("no module {name} in {all:?}"))
    }

    fn tree() -> Vec<(String, String)> {
        vec![
            file("Cargo.toml", "[package]\nname = \"top\"\n"),
            file("crates/a/Cargo.toml", "[package]\nname = \"a-b\"\nversion = \"1\"\n"),
            file("crates/a/src/lib.rs", "pub fn one() {}\n"),
            file("crates/a/src/deep/x.rs", "pub fn x() {}\n"),
            file("crates/a/src/deep/y.rs", "pub fn y() {}\n"),
            file("crates/c/Cargo.toml", "[package]\nname = \"c\"\n"),
            file("crates/c/src/main.rs", "use crate::inner::z;\nuse a_b::deep::x;\nfn main() {}\n"),
            file("crates/c/src/inner/z.rs", "pub fn z() {}\n"),
        ]
    }

    /// @tests REQ-MODULES.one_per_directory
    #[test]
    fn a_module_is_a_directory_of_rust_with_its_files_counted() {
        let all = modules(&tree(), &[]);
        let names: Vec<&str> = all.iter().map(|m| m.name.as_str()).collect();
        assert_eq!(names, ["crates/a/src", "crates/a/src/deep", "crates/c/src", "crates/c/src/inner"]);
        assert_eq!(find(&all, "crates/a/src/deep").files, 2);
        assert_eq!(find(&all, "crates/a/src").files, 1);
        assert!(modules(&[file("README.md", "x"), file("Cargo.toml", "")], &[]).is_empty(), "a tree with no Rust made modules");
    }

    /// @tests REQ-MODULES.one_per_directory
    #[test]
    fn files_under_a_dot_directory_are_not_part_of_the_tree() {
        let mut files = tree();
        files.push(file(".tracelean/hook.rs", "fn h() {}\n"));
        files.push(file("crates/a/.hidden/q.rs", "fn q() {}\n"));
        let all = modules(&files, &[]);
        assert!(all.iter().all(|m| !m.name.contains(".tracelean") && !m.name.contains(".hidden")), "{all:?}");
        assert_eq!(all.len(), 4);
    }

    /// @tests REQ-MODULES.edges_from_paths
    #[test]
    fn an_edge_comes_from_a_crate_path_and_from_another_crates_name() {
        let all = modules(&tree(), &[]);
        assert_eq!(find(&all, "crates/c/src").uses, ["crates/a/src/deep", "crates/c/src/inner"]);
        assert!(find(&all, "crates/a/src/deep").uses.is_empty());
        assert!(find(&all, "crates/c/src/inner").uses.is_empty());
    }

    /// @tests REQ-MODULES.edges_from_paths
    #[test]
    fn a_path_inside_a_comment_makes_no_edge() {
        let mut files = tree();
        files[6] = file(
            "crates/c/src/main.rs",
            "// use crate::inner::z;\n/// see a_b::deep::x\nfn main() { let _ = 1; } // crate::inner\n",
        );
        let all = modules(&files, &[]);
        assert!(find(&all, "crates/c/src").uses.is_empty(), "{:?}", find(&all, "crates/c/src"));
    }

    /// @tests REQ-MODULES.edges_from_paths
    #[test]
    fn a_module_never_uses_itself() {
        let files = vec![
            file("Cargo.toml", "[package]\nname = \"solo\"\n"),
            file("src/lib.rs", "use crate::a::f;\nuse solo::a::g;\n"),
            file("src/a/m.rs", "use crate::a::other;\nuse solo::a::more;\n"),
        ];
        let all = modules(&files, &[]);
        assert!(find(&all, "src/a").uses.is_empty(), "{all:?}");
        assert_eq!(find(&all, "src").uses, ["src/a"]);
    }

    /// @tests REQ-MODULES.edges_from_paths
    #[test]
    fn a_name_that_only_ends_in_a_crate_name_is_not_a_path_to_it() {
        let mut files = tree();
        files[6] = file("crates/c/src/main.rs", "use not_a_b::deep::x;\nfn main() {}\n");
        let all = modules(&files, &[]);
        assert!(find(&all, "crates/c/src").uses.is_empty());
    }

    /// @tests REQ-MODULES.serves_from_claims
    #[test]
    fn serves_is_sorted_once_each_and_only_from_rust_files_in_the_directory() {
        let claims = vec![
            claim("crates/a/src/deep/x.rs", "REQ-B"),
            claim("crates/a/src/deep/y.rs", "REQ-A"),
            claim("crates/a/src/deep/x.rs", "REQ-B"),
            claim("crates/a/src/deep/notes.md", "REQ-Z"),
            claim("crates/elsewhere/q.rs", "REQ-Q"),
        ];
        let all = modules(&tree(), &claims);
        assert_eq!(find(&all, "crates/a/src/deep").serves, ["REQ-A", "REQ-B"]);
        assert!(find(&all, "crates/a/src").serves.is_empty());
        assert_eq!(all.iter().filter(|m| !m.serves.is_empty()).count(), 1);
    }

    /// @tests REQ-MODULES.unclaimed_flagged
    #[test]
    fn the_modules_claiming_nothing_are_named() {
        let all = modules(&tree(), &[claim("crates/a/src/lib.rs", "REQ-A")]);
        assert_eq!(serving_nothing(&all), ["crates/a/src/deep", "crates/c/src", "crates/c/src/inner"]);
        assert!(serving_nothing(&[]).is_empty());
    }

    /// @tests REQ-MODULES.shown_as_text
    #[test]
    fn the_table_has_a_row_per_module_and_names_the_flagged_ones() {
        let all = modules(&tree(), &[claim("crates/a/src/lib.rs", "REQ-A")]);
        let lines = module_lines(&all);
        assert_eq!(lines[0], "## Modules");
        assert_eq!(lines[2], "| module | files | uses | serves |");
        assert!(lines.contains(&"| crates/a/src | 1 | – | REQ-A |".to_string()), "{lines:?}");
        assert!(lines.contains(&"| crates/c/src | 1 | crates/a/src/deep, crates/c/src/inner | – |".to_string()), "{lines:?}");
        assert_eq!(
            lines.last().map(String::as_str),
            Some("Claiming no requirement: crates/a/src/deep, crates/c/src, crates/c/src/inner")
        );
        let none = module_lines(&[]);
        assert_eq!(none, ["## Modules", "", "No Rust source here."]);
        let all_claimed = modules(&tree()[..3], &[claim("crates/a/src/lib.rs", "REQ-A")]);
        assert!(module_lines(&all_claimed).iter().all(|l| !l.starts_with("Claiming")));
    }

    fn node(id: &str, clauses: usize, held: [usize; 5]) -> Node {
        Node {
            id: id.to_string(),
            title: String::new(),
            refines: Vec::new(),
            marks: crate::evidence::Marks::NONE,
            clauses,
            modelled: 0,
            held: Tally { implemented: held[0], annotated: held[1], judged: held[2], tested: held[3], proved: held[4] },
            covered: None,
        }
    }

    fn figures() -> Figures {
        Figures {
            nodes: vec![
                node("REQ-B", 4, [4, 4, 0, 2, 0]),
                node("REQ-A", 4, [4, 4, 0, 2, 0]),
                node("REQ-DONE", 2, [2, 2, 2, 2, 2]),
                node("REQ-WORST", 3, [0, 0, 0, 0, 0]),
            ],
            findings: Some(7),
            ..Figures::default()
        }
    }

    fn position(lines: &[String], text: &str) -> usize {
        lines.iter().position(|l| l.contains(text)).unwrap_or_else(|| panic!("`{text}` not in {lines:#?}"))
    }

    /// @tests REQ-FIGURE.overview_as_text
    #[test]
    fn the_overview_counts_and_marks_and_orders_where_work_is_left() {
        let lines = overview(&figures());
        assert_eq!(lines[0], "# Overview");
        assert_eq!(lines[2], "4 requirements · 13 clauses · 1/4 requirements with every mark · 7 findings");
        let rows: Vec<&String> =
            lines.iter().filter(|l| MARKS.iter().any(|(letter, name)| l.starts_with(&format!("| {letter} {name} |")))).collect();
        assert_eq!(rows.len(), 5, "{rows:?}");
        assert!(rows[0].starts_with("| I implemented | 10/13 | 77% |"), "{}", rows[0]);
        assert!(rows[2].starts_with("| J judged | 2/13 | 15% |"), "{}", rows[2]);
        assert!(rows[4].starts_with("| P proved | 2/13 | 15% |"), "{}", rows[4]);
        let at = |id: &str| position(&lines, &format!("| {id} |"));
        assert!(at("REQ-WORST") < at("REQ-A") && at("REQ-A") < at("REQ-B") && at("REQ-B") < at("REQ-DONE"));
        assert!(lines[at("REQ-WORST")].ends_with("| 0/3 | 0/3 | 0/3 | 0/3 | 0/3 | 15 |"), "{}", lines[at("REQ-WORST")]);
        assert!(lines[at("REQ-A")].ends_with("| 4/4 | 4/4 | 0/4 | 2/4 | 0/4 | 10 |"), "{}", lines[at("REQ-A")]);
        let unknown = overview(&Figures { findings: None, ..figures() });
        assert!(unknown[2].ends_with("· – findings"), "{}", unknown[2]);
    }

    /// @tests REQ-FIGURE.overview_as_text
    #[test]
    fn a_section_with_nothing_to_say_is_left_out_and_one_with_something_is_written() {
        let bare = overview(&figures()).join("\n");
        for heading in ["Test runs on record", "Your unsaved edits", "The agent's waiting changes", "## Modules"] {
            assert!(!bare.contains(heading), "`{heading}` stood with nothing to say");
        }
        let empty = overview(&Figures::default()).join("\n");
        assert!(empty.contains("0 requirements · 0 clauses") && empty.contains("## Where work is left"));
        let full = overview(&Figures {
            runs: vec![RunRow { op: "REQ-A.a".into(), clauses: 2, seed: 62, cases: 3000, age: Some(7300) }],
            unsaved: vec![RiskRow { clause: "REQ-A.a".into(), marks: "A-T-".into(), file: "src/a.rs".into() }],
            agent: vec![Step { order: 1, kind: "modified".into(), file: "src/a.rs".into(), added: 3, removed: 1,
                clauses: vec![("REQ-A.a".into(), "A---".into())] }],
            modules: modules(&tree(), &[]),
            ..figures()
        });
        let text = full.join("\n");
        assert!(text.contains("| REQ-A.a | 2 | 3000 | 62 | 2 h ago |"), "{text}");
        assert!(text.contains("| REQ-A.a | A-T- | src/a.rs |"), "{text}");
        assert!(text.contains("| 1 | modified | src/a.rs | +3 −1 | REQ-A.a (A---) |"), "{text}");
        assert!(text.contains("| crates/a/src | 1 | – | – |"), "{text}");
    }

    /// @tests REQ-FIGURE.agent_steps_listed
    #[test]
    fn a_waiting_change_that_claims_nothing_says_so_in_the_list() {
        let lines = overview(&Figures {
            agent: vec![
                Step { order: 1, kind: "created".into(), file: "n.rs".into(), added: 5, removed: 0, clauses: vec![] },
                Step { order: 2, kind: "deleted".into(), file: "o.rs".into(), added: 0, removed: 2, clauses: vec![] },
            ],
            ..figures()
        });
        let first = position(&lines, "| 1 | created | n.rs | +5 −0 | – |");
        assert_eq!(position(&lines, "| 2 | deleted | o.rs | +0 −2 | – |"), first + 1, "arrival order");
    }

    /// @tests REQ-FIGURE.overview_as_text
    #[test]
    fn an_age_is_said_in_its_largest_whole_unit() {
        assert_eq!(ago(None), "–");
        assert_eq!(ago(Some(0)), "just now");
        assert_eq!(ago(Some(59)), "just now");
        assert_eq!(ago(Some(60)), "1 min ago");
        assert_eq!(ago(Some(3599)), "59 min ago");
        assert_eq!(ago(Some(3600)), "1 h ago");
        assert_eq!(ago(Some(86399)), "23 h ago");
        assert_eq!(ago(Some(86400)), "1 d ago");
        assert_eq!(ago(Some(200_000)), "2 d ago");
    }
}
