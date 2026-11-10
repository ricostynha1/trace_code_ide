use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};

use tracelean_core::evidence::Marks;
use tracelean_core::surface::keymap::{self, Keymap};
use tracelean_core::surface::view::plain_text;
use tracelean_editor::{impact_of, Editor};

const KEYMAP: &str = include_str!("../../../assets/keymap.json");

fn keymap() -> Keymap {
    keymap::load(KEYMAP, &keymap::actions()).expect("the shipped keymap loads")
}

static NEXT: AtomicUsize = AtomicUsize::new(0);

/// A project of its own for each call, so tests running in parallel never share a folder.
fn project(files: &[(&str, &str)]) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "tracelean-figures-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::SeqCst)
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("a directory");
    for (name, content) in files {
        let path = dir.join(name);
        std::fs::create_dir_all(path.parent().expect("a parent")).expect("a directory");
        std::fs::write(path, content).expect("a file");
    }
    dir
}

const REQ_A: (&str, &str) = ("reqs/a.md", "---\nid: REQ-A\ntitle: Middle\nclauses:\n  a: It is so.\n---\n");

fn text(editor: &Editor) -> String {
    plain_text(editor.buffer()).join("\n")
}

/// The offset of the first row of the tutorial that starts with `mark`.
fn row_with(editor: &Editor, mark: char) -> Option<usize> {
    let mut at = 0;
    for line in plain_text(editor.buffer()) {
        if line.starts_with(mark) {
            return Some(at);
        }
        at += line.chars().count() + 1;
    }
    None
}

/// @tests REQ-GUIDE.sections_unfold
/// @tests REQ-GUIDE.tutorial_in_the_editor
#[test]
fn a_folded_tutorial_section_unfolds_and_folds_again() {
    let root = project(&[REQ_A]);
    let mut editor = Editor::open(root.clone(), keymap());
    editor.act_here("help.tutorial");
    let before = text(&editor);
    assert!(before.contains("TraceLean in five minutes"), "the tutorial was not shown");

    editor.offset = row_with(&editor, '▸').expect("the tutorial has a folded section");
    editor.act_here("help.more");
    let open = text(&editor);
    assert!(open.chars().count() > before.chars().count(), "unfolding did not add the section's lines");
    assert!(open.lines().any(|l| l.starts_with('▾')), "the row did not become open");
    assert!(open.lines().filter(|l| l.starts_with('▸')).count() < before.lines().filter(|l| l.starts_with('▸')).count());

    editor.offset = row_with(&editor, '▾').expect("an open row");
    editor.act_here("help.more");
    assert_eq!(text(&editor), before, "folding again did not restore the tutorial");
    assert!(!root.join(".tracelean").exists() || std::fs::read_dir(root.join(".tracelean")).unwrap().next().is_none(),
        "showing the tutorial wrote to the project");
    let _ = std::fs::remove_dir_all(&root);
}

/// @tests REQ-LOOK.menu_bar_lists_leader
#[test]
fn the_menu_bar_has_a_group_per_leader_entry_and_only_real_actions() {
    use keymap::Binding;
    let km = keymap();
    let root = project(&[REQ_A]);
    let editor = Editor::open(root.clone(), keymap());
    let groups = editor.leader();

    let leader = km
        .mode(&km.root)
        .and_then(|m| {
            m.bindings.iter().find_map(|(_, b)| match b {
                Binding::Enter { mode, .. } if km.mode(mode).is_some_and(|m| m.parent.is_some()) => Some(mode.clone()),
                _ => None,
            })
        })
        .expect("the keymap has a leader");
    let entered: Vec<String> = km
        .mode(&leader)
        .expect("the leader mode")
        .bindings
        .iter()
        .filter_map(|(_, b)| match b {
            Binding::Enter { description, .. } => Some(description.clone()),
            _ => None,
        })
        .collect();
    assert!(!entered.is_empty());
    for name in &entered {
        assert!(groups.iter().any(|(group, _)| group == name), "no group for the leader entry `{name}`");
    }
    let known = keymap::actions();
    let mut count = 0;
    for (group, entries) in &groups {
        for (keys, _, action) in entries {
            count += 1;
            assert!(known.iter().any(|a| a == action), "`{group}` offers `{action}` ({keys}), which is not an action");
        }
    }
    assert!(count > 0, "the groups hold no entries");
    let _ = std::fs::remove_dir_all(&root);
}

/// @tests REQ-FIGURE.impact_before_saving
#[test]
fn impact_is_once_per_file_for_changed_files_only_and_sorted() {
    let claim = |file: &str, req: &str, clause: &str| (file.to_string(), req.to_string(), Some(clause.to_string()));
    let claims = vec![
        claim("b.rs", "REQ-Z", "z"),
        claim("a.rs", "REQ-A", "a"),
        claim("a.rs", "REQ-A", "a"),
        claim("untouched.rs", "REQ-Q", "q"),
        claim("a.rs", "REQ-A", "a2"),
    ];
    let mut held = BTreeMap::new();
    held.insert(
        ("REQ-A".to_string(), Some("a".to_string())),
        Marks { annotated: true, tested: true, ..Marks::NONE },
    );
    let changed = vec!["a.rs".to_string(), "b.rs".to_string()];
    let got = impact_of(&changed, claims.clone(), &held);

    let listed: Vec<(&str, Option<&str>, &str, &str)> =
        got.iter().map(|i| (i.req_id.as_str(), i.clause.as_deref(), i.file.as_str(), i.marks.as_str())).collect();
    assert_eq!(
        listed,
        vec![
            ("REQ-A", Some("a"), "a.rs", "A-T-"),
            ("REQ-A", Some("a2"), "a.rs", "----"),
            ("REQ-Z", Some("z"), "b.rs", "----"),
        ]
    );
    assert!(impact_of(&[], claims, &held).is_empty(), "nothing changed, yet something was listed");
}

/// @tests REQ-FIGURE.card_on_hover
#[test]
fn a_card_holds_the_clause_its_words_and_who_claims_it() {
    let root = project(&[
        REQ_A,
        ("src/a.rs", "/// @implements REQ-A.a\npub fn a() {}\n"),
    ]);
    let editor = Editor::open(root.clone(), keymap());
    let card = editor.card("REQ-A.a").expect("a card for a known clause");
    assert_eq!(card.id, "REQ-A");
    assert_eq!(card.title, "Middle");
    assert_eq!(card.clauses.len(), 1);
    assert_eq!(card.clauses[0].clause, "a");
    assert_eq!(card.clauses[0].text, "It is so.");
    assert!(
        card.claims.iter().any(|(role, file, _)| role == "implements" && file == "src/a.rs"),
        "the claiming file is missing: {:?}",
        card.claims
    );
    assert!(editor.card("REQ-NOPE.x").is_none(), "an unknown id got a card");
    let _ = std::fs::remove_dir_all(&root);
}

/// @tests REQ-FIGURE.runs_listed
#[test]
fn each_compared_operation_with_a_run_on_record_is_listed() {
    let record = |clause: &str| {
        format!(
            r#"{{"key":{{"reqId":"REQ-A","clause":"{clause}","bond":"modelImpl"}},"level":"L3","detail":{{"drt":{{"seed":62,"cases":3000,"op":"REQ-A.a"}}}},"linkHash":"h","inputs":[]}}"#
        )
    };
    let (one, two) = (record("a"), record("b"));
    let root = project(&[
        REQ_A,
        (".tracelean/evidence/REQ-A.a.ModelImpl.json", one.as_str()),
        (".tracelean/evidence/REQ-A.b.ModelImpl.json", two.as_str()),
        (".tracelean/evidence/REQ-A.c.RequirementModel.json", r#"{"key":{},"detail":{}}"#),
    ]);
    let editor = Editor::open(root.clone(), keymap());
    let runs = editor.runs();
    assert_eq!(runs.len(), 1, "one operation was compared: {runs:?}");
    assert_eq!(runs[0].op, "REQ-A.a");
    assert_eq!(runs[0].clauses, 2);
    assert_eq!(runs[0].seed, 62);
    assert_eq!(runs[0].cases, 3000);
    assert!(runs[0].age.is_some(), "the time it was recorded is missing");
    let _ = std::fs::remove_dir_all(&root);
}

const TREE: [(&str, &str); 5] = [
    ("Cargo.toml", "[package]\nname = \"demo\"\n"),
    REQ_A,
    ("src/a.rs", "/// @implements REQ-A.a\npub fn a() {}\n"),
    ("src/inner/b.rs", "use crate::missing::x;\npub fn b() { demo::inner::b2(); }\n"),
    ("src/inner/c.rs", "pub fn b2() {}\n"),
];

/// @tests REQ-MODULES.one_per_directory
/// @tests REQ-MODULES.serves_from_claims
/// @tests REQ-MODULES.shown_as_text
#[test]
fn the_editor_gives_the_modules_of_its_tree_with_what_each_serves() {
    let root = project(&TREE);
    let editor = Editor::open(root.clone(), keymap());
    let found = editor.modules();
    let names: Vec<&str> = found.iter().map(|m| m.name.as_str()).collect();
    assert_eq!(names, ["src", "src/inner"]);
    assert_eq!(found[0].serves, ["REQ-A"]);
    assert!(found[1].serves.is_empty(), "a module with no claim served something: {:?}", found[1]);
    assert_eq!(found[1].files, 2);
    let _ = std::fs::remove_dir_all(&root);
}

/// @tests REQ-FIGURE.overview_as_text
/// @tests REQ-FIGURE.exported_as_markdown
/// @tests REQ-MODULES.shown_as_text
#[test]
fn the_overview_as_markdown_names_the_requirement_and_the_modules() {
    let root = project(&TREE);
    let editor = Editor::open(root.clone(), keymap());
    let markdown = editor.overview_markdown();
    assert!(markdown.starts_with("# Overview\n"), "{markdown}");
    assert!(markdown.ends_with('\n'));
    assert!(markdown.contains("| REQ-A | 1/1 |"), "the requirement is missing:\n{markdown}");
    assert!(markdown.contains("## Modules"), "{markdown}");
    assert!(markdown.contains("| src | 1 | – | REQ-A |"), "{markdown}");
    assert!(markdown.contains("| src/inner | 2 | src | – |"), "{markdown}");
    assert!(markdown.contains("Claiming no requirement: src/inner"), "{markdown}");
    assert!(!markdown.contains("The agent's waiting changes"), "an agent section with no agent");
    let _ = std::fs::remove_dir_all(&root);
}

/// @tests REQ-FIGURE.overview_as_text
/// @tests REQ-MODULES.shown_as_text
#[test]
fn the_overview_and_modules_stations_open_as_documents() {
    use tracelean_core::surface::screen::DOCUMENT;
    let root = project(&TREE);
    let mut editor = Editor::open(root.clone(), keymap());
    for (action, title, expect) in [
        ("screen.station.overview", "record:overview", "# Overview"),
        ("screen.station.modules", "record:modules", "## Modules"),
    ] {
        editor.act_here(action);
        assert_eq!(editor.screen.focus, DOCUMENT, "`{action}` did not land in the document pane");
        let shown = editor.buffer();
        assert_eq!(shown.id, title, "`{action}` opened another buffer");
        assert!(text(&editor).contains(expect), "`{action}` showed:\n{}", text(&editor));
    }
    assert!(text(&editor).contains("| src/inner | 2 | src |"));
    let _ = std::fs::remove_dir_all(&root);
}

/// @tests REQ-FIGURE.agent_steps_listed
#[test]
fn with_no_agent_session_nothing_is_waiting() {
    let root = project(&TREE);
    let editor = Editor::open(root.clone(), keymap());
    assert!(editor.agent_steps().is_empty());
    assert!(editor.figures().agent.is_empty());
    let _ = std::fs::remove_dir_all(&root);
}

/// @tests REQ-FIGURE.agent_steps_listed
#[test]
fn an_agents_waiting_changes_are_listed_per_file_with_the_clauses_each_claims() {
    let root = project(&TREE);
    let mut editor = Editor::open(root.clone(), keymap());
    editor.act_here("sandbox.new");
    let work = PathBuf::from(&editor.session.as_ref().expect("a sandbox session").work);
    std::fs::write(work.join("src/a.rs"), "/// @implements REQ-A.a\npub fn a() { 1 }\npub fn more() {}\n").unwrap();
    std::fs::write(work.join("src/new.rs"), "pub fn n() {}\n").unwrap();
    std::fs::remove_file(work.join("src/inner/c.rs")).unwrap();
    assert!(editor.tick(), "the editor did not notice the agent's changes");

    let steps = editor.agent_steps();
    let listed: Vec<(usize, &str, &str)> = steps.iter().map(|s| (s.order, s.kind.as_str(), s.file.as_str())).collect();
    assert_eq!(steps.len(), 3, "{listed:?}");
    let by = |file: &str| steps.iter().find(|s| s.file == file).unwrap_or_else(|| panic!("no step for {file}: {listed:?}"));
    assert_eq!(by("src/a.rs").kind, "modified");
    assert_eq!((by("src/a.rs").added, by("src/a.rs").removed), (2, 1));
    assert_eq!(by("src/a.rs").clauses, [("REQ-A.a".to_string(), "A---".to_string())]);
    assert_eq!(by("src/new.rs").kind, "created");
    assert_eq!((by("src/new.rs").added, by("src/new.rs").removed), (1, 0));
    assert!(by("src/new.rs").clauses.is_empty());
    assert_eq!(by("src/inner/c.rs").kind, "deleted");
    assert_eq!((by("src/inner/c.rs").added, by("src/inner/c.rs").removed), (0, 1));
    let mut order: Vec<usize> = steps.iter().map(|s| s.order).collect();
    order.sort();
    assert_eq!(order, [1, 2, 3]);
    assert!(steps.windows(2).all(|pair| pair[0].order < pair[1].order), "not in arrival order: {listed:?}");

    let markdown = editor.overview_markdown();
    assert!(markdown.contains("## The agent's waiting changes"), "{markdown}");
    assert!(markdown.contains("src/a.rs | +2 −1 | REQ-A.a (A---)"), "{markdown}");
    let _ = std::fs::remove_dir_all(&root);
}
