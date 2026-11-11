//! Plays the tutorial's agent exercise on a copy of the demo and prints what the editor shows.
//! Run: DEMO_COPY=<dir> cargo test -p tracelean-editor --test tutorial_walk -- --ignored --nocapture
use std::path::PathBuf;
use tracelean_core::surface::keymap;
use tracelean_core::surface::view::plain_text;
use tracelean_editor::Editor;

const KEYMAP: &str = include_str!("../../../assets/keymap.json");

fn act(editor: &mut Editor, action: &str) {
    let intent = tracelean_core::surface::act::dispatch(action.into(), editor.focus(), editor.workspace(), editor.waiting());
    editor.perform(intent);
}

fn status(editor: &Editor) -> String {
    plain_text(editor.status.clone()).join(" | ")
}

#[test]
#[ignore = "prints; needs DEMO_COPY"]
fn the_tutorials_agent_exercise_on_the_demo() {
    let root = PathBuf::from(std::env::var("DEMO_COPY").expect("DEMO_COPY"));
    let km = keymap::load(KEYMAP, &keymap::actions()).unwrap();
    let mut editor = Editor::open(root.clone(), km);

    // undo: two edits, undo twice, type something else
    act(&mut editor, "file.open");
    println!("== tree marks at start: {:?}", editor.tree_marks());

    act(&mut editor, "sandbox.new");
    let work = PathBuf::from(&editor.session.as_ref().expect("session").work);
    std::fs::OpenOptions::new().append(true).open(work.join("src/main.rs")).and_then(|mut f| std::io::Write::write_all(&mut f, b"// the agent was here\n")).unwrap();
    std::fs::write(work.join("src/kelvin.rs"), "pub fn to_kelvin(c: i64) -> i64 { c + 273 }\n").unwrap();
    std::fs::remove_file(work.join("src/blank.rs")).unwrap();
    println!("== tick noticed: {}", editor.tick());
    println!("== tree marks after agent: {:?}", editor.tree_marks().into_iter().filter(|(p, _)| p.contains("src/")).collect::<Vec<_>>());
    for pane in editor.laid_out(editor.region) {
        if pane.buffer.id.starts_with("dir:") {
            println!("== explorer after the agent:\n{}", plain_text(pane.buffer.clone()).join("\n"));
        }
    }
    act(&mut editor, "history.tree");
    println!("== history tree:\n{}", plain_text(editor.buffer()).join("\n"));
    println!("== status: {}", status(&editor));
    act(&mut editor, "sandbox.copy");
    println!("== copy: {:?}", editor.clipboard);
    editor.chord("C-z");
    println!("== after Ctrl+Z (history focused) status: {}", status(&editor));
    act(&mut editor, "history.undo");
    println!("== after history.undo status: {}", status(&editor));
    println!("== blank.rs back in the sandbox: {}", work.join("src/blank.rs").exists());
    act(&mut editor, "history.redo");
    println!("== after history.redo status: {}", status(&editor));
    for pane in editor.laid_out(editor.region) {
        if pane.buffer.id.starts_with("dir:") {
            println!("== explorer after redo:\n{}", plain_text(pane.buffer.clone()).join("\n"));
        }
    }
    println!("== marks after undo: {:?}", editor.tree_marks().into_iter().filter(|(p, _)| p.contains("src/")).collect::<Vec<_>>());
    println!("== main.rs in sandbox: {:?}", std::fs::read_to_string(work.join("src/main.rs")).map(|s| s.ends_with("agent was here\n")));
    act(&mut editor, "observe.accept");
    println!("== after accept: {}", status(&editor));
    println!("== kelvin in project: {}", root.join("src/kelvin.rs").exists());
    act(&mut editor, "sandbox.end");
    println!("== ended: {}", status(&editor));
}

#[test]
#[ignore = "prints; needs DEMO_COPY"]
fn the_tutorials_undo_exercise_on_the_demo() {
    let root = PathBuf::from(std::env::var("DEMO_COPY").expect("DEMO_COPY"));
    let km = keymap::load(KEYMAP, &keymap::actions()).unwrap();
    let mut editor = Editor::open(root.clone(), km);
    editor.perform(tracelean_core::surface::act::Intent::Display {
        what: tracelean_core::surface::view::BufferKind::File { path: "src/main.rs".into() },
    });
    editor.key("i");
    editor.insert("// one\n");
    editor.chord("C-s");
    println!("== saved: {}", status(&editor));
    editor.insert("// two\n");
    editor.chord("C-s");
    editor.chord("C-z");
    println!("== ctrl+z 1: {}", status(&editor));
    editor.chord("C-z");
    println!("== ctrl+z 2: {}", status(&editor));
    editor.insert("// other\n");
    editor.chord("C-s");
    act(&mut editor, "history.tree");
    println!("== tree:\n{}", plain_text(editor.buffer()).join("\n"));
    println!("== file on disk:\n{}", std::fs::read_to_string(root.join("src/main.rs")).unwrap());
}
