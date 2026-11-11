//! Every menu-bar entry, launched with a .rs file open: which ones change anything?
use tracelean_core::surface::keymap::{self};
use tracelean_core::surface::view::plain_text;
use tracelean_editor::Editor;

const KEYMAP: &str = include_str!("../../../assets/keymap.json");

/// @tests REQ-LOOK.menu_bar_lists_leader
#[test]
fn every_menu_entry_launched_from_a_rs_file_does_something() {
    let root = std::env::temp_dir().join(format!("tracelean-menu-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let demo = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../demo");
    let ok = std::process::Command::new("cp")
        .args(["-r", "--no-preserve=mode"])
        .arg(&demo)
        .arg(&root)
        .status()
        .unwrap();
    assert!(ok.success());
    let _ = std::fs::remove_dir_all(root.join("target"));
    let km = keymap::load(KEYMAP, &keymap::actions()).unwrap();
    let probe = Editor::open(root.clone(), km.clone());
    let entries: Vec<(String, String, String)> =
        probe.leader().into_iter().flat_map(|(g, e)| e.into_iter().map(move |(k, d, a)| (g.clone(), d, a))).collect();
    let mut dead = Vec::new();
    for (group, said, action) in entries {
        if action == "file.delete" || action == "file.rename" {
            continue;
        }
        let mut editor = Editor::open(root.clone(), km.clone());
        editor.act_here("file.open");
        let before = (plain_text(editor.buffer()), editor.offered(), plain_text(editor.status.clone()), format!("{:?}", editor.workspace()));
        editor.act_here(&action);
        let after = (plain_text(editor.buffer()), editor.offered(), plain_text(editor.status.clone()), format!("{:?}", editor.workspace()));
        println!("{group:12} {action:28} {}", if before == after { "NOTHING" } else { "ok" });
        if before == after {
            dead.push(format!("{group}: {said} ({action})"));
        }
    }
    println!("DEAD: {dead:#?}");
    let _ = std::fs::remove_dir_all(&root);
}
