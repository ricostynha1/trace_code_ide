//! There is one thing on screen, and it is a buffer.
//!
//! `everything_is_a_buffer` is a claim about this tree's vocabulary rather than
//! about a value: a file, a directory listing, a diff under review, a menu and a
//! transcript are all the same type, and nothing in the core offers a second way
//! to say what is shown. That is checked by building one of each and by reading
//! the surface layer, which is what `@structural` records (ADR-0012).

use std::path::{Path, PathBuf};

use tracelean_core::surface::keymap;
use tracelean_core::surface::view::{
    actions_at, directory_buffer, faults, plain_text, Buffer, BufferKind, Role, Span,
};

fn project_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap().parent().unwrap().to_path_buf()
}

fn of_kind(kind: BufferKind, text: &str, actions: &[&str]) -> Buffer {
    Buffer {
        id: format!("{kind:?}"),
        kind,
        text: text.to_string(),
        spans: vec![Span {
            start: 0,
            stop: text.chars().count(),
            role: Role::Entry,
            actions: actions.iter().map(|a| a.to_string()).collect(),
        }],
    }
}

/// Every shown thing is a buffer, and every buffer answers the same questions.
///
/// @tests REQ-VIEW.everything_is_a_buffer
/// @structural REQ-VIEW.everything_is_a_buffer reason="a claim about this tree's vocabulary — that one type carries every shown thing and the core offers no second one — which is a property of the code rather than a value any function computes"
#[test]
fn a_file_a_listing_a_review_a_menu_and_a_record_are_all_buffers() {
    let buffers = vec![
        of_kind(BufferKind::File { path: "src/main.rs".into() }, "fn main() {}", &["file.save"]),
        directory_buffer("src".into(), vec![(0, "main.rs".into()), (1, "deep.rs".into())]),
        of_kind(
            BufferKind::Review { target: "src/main.rs".into() },
            "+ added a line\n- removed a line",
            &["observe.accept", "observe.reject"],
        ),
        of_kind(BufferKind::Menu { title: "leader".into() }, "t  trace\nd  drt", &["drt.run"]),
        of_kind(
            BufferKind::Record { title: "transcript".into() },
            "the agent wrote src/main.rs",
            &["observe.diff"],
        ),
    ];

    for buffer in &buffers {
        // Each one describes its own text, renders, and answers what can be done.
        assert_eq!(faults(buffer.clone()), vec![], "{:?}", buffer.kind);
        assert!(!plain_text(buffer.clone()).is_empty(), "{:?} rendered nothing", buffer.kind);
        assert!(
            !actions_at(buffer.clone(), 0).is_empty(),
            "{:?} offers nothing at its first position",
            buffer.kind
        );

        // Every action a buffer names is one the keymap can dispatch. An action
        // only the representation knows about is one no key reaches, and an
        // action only the keymap knows about is one no buffer offers.
        for action in actions_at(buffer.clone(), 0) {
            assert!(
                keymap::ACTIONS.contains(&action.as_str()),
                "{:?} names `{action}`, which the keymap does not have",
                buffer.kind
            );
        }
    }

    // And nothing in the surface layer offers a second way to say what is shown.
    // A frontend-shaped type here would be a view model, which is the thing that
    // rotted in the tree this port came from.
    let root = project_root();
    for file in ["keymap.rs", "lsp.rs", "mod.rs", "produce.rs"] {
        let source =
            std::fs::read_to_string(root.join("crates/core/src/surface").join(file)).unwrap();
        let library = &source[..source.find("#[cfg(test)]").unwrap_or(source.len())];
        for needle in ["struct Screen", "struct Widget", "struct Pane", "struct View", "fn render"] {
            assert!(
                !library.contains(needle),
                "`{needle}` in surface/{file}: a second answer to what is on screen"
            );
        }
    }
}

/// Every frontend in this tree renders buffers and builds none.
///
/// A frontend that constructs a `Buffer` has become a second producer, which is
/// the failure `REQ-SHOW.core_produces` exists to prevent: two answers to what
/// is on screen, one of them maintained. Checkable by reading, because building
/// one means naming the type.
///
/// @tests REQ-SHOW.core_produces
#[test]
fn no_frontend_builds_a_buffer_of_its_own() {
    let root = project_root();
    let mut checked = 0;
    for directory in ["crates/core/src/bin", "crates/tui/src", "crates/desktop/src"] {
        let Ok(entries) = std::fs::read_dir(root.join(directory)) else {
            // The terminal frontend does not exist yet; when it does, it is
            // held to this without anybody remembering to add it here.
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some("rs") {
                continue;
            }
            let source = std::fs::read_to_string(&path).unwrap();
            let library = &source[..source.find("#[cfg(test)]").unwrap_or(source.len())];
            checked += 1;
            for (line_no, line) in library.lines().enumerate() {
                let line = line.trim();
                // A frontend *calling* a producer is the point; what it may not
                // do is construct the value itself. Returning what a producer
                // returned is a call, so a signature naming the type is not a
                // construction.
                if line.starts_with("//") || line.contains("-> Buffer") || line.contains("-> Span") {
                    continue;
                }
                for needle in ["Buffer {", "Span {"] {
                    assert!(
                        !line.contains(needle),
                        "{}:{}: a frontend constructing `{needle}` is a second producer",
                        path.display(),
                        line_no + 1
                    );
                }
            }
        }
    }
    assert!(checked > 0, "no frontend source was read, so this checked nothing");

    // The web frontend is held to the same rule in its own language: a `Buffer`
    // is a type it receives, never one it makes.
    let web = root.join("web/src");
    if let Ok(entries) = std::fs::read_dir(&web) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some("ts") {
                continue;
            }
            let source = std::fs::read_to_string(&path).unwrap();
            for (line_no, line) in source.lines().enumerate() {
                let line = line.trim();
                if line.starts_with("//") || line.contains("Buffer;") || line.contains(": Buffer") {
                    continue;
                }
                assert!(
                    !line.contains("const buffer: Buffer = {"),
                    "{}:{}: the web frontend is building a buffer",
                    path.display(),
                    line_no + 1
                );
            }
        }
    }
}

/// The window can reach the editor.
///
/// The page is the strict half of `one_representation`: it renders a buffer the
/// core produced and produces none, so with no way to ask for one it renders
/// nothing at all. That is what an opened window showed — *No editor is
/// attached* — and neither the editor, the producers nor the page were wrong.
/// The bridge the page looks for was simply not injected, because Tauri injects
/// it only when the configuration asks.
///
/// So the two are read back together: the name the page reaches for, and the
/// setting that puts it there. Nothing a value can express — the failure is an
/// agreement between a TypeScript file and a JSON file, which is why this is
/// checked by reading (ADR-0012) and why the claim is annotated here rather
/// than in a language TraceLean has no grammar for (ADR-0008).
///
/// @tests REQ-VIEW.one_representation
#[test]
fn the_window_is_configured_to_hand_the_page_the_bridge_it_looks_for() {
    let root = project_root();

    let page = std::fs::read_to_string(root.join("web/src/app.ts")).expect("the page");
    assert!(
        page.contains("__TAURI__"),
        "the page no longer looks for a bridge; this test is checking the wrong name"
    );

    let config =
        std::fs::read_to_string(root.join("crates/desktop/tauri.conf.json")).expect("the window");
    let asked: serde_json::Value = serde_json::from_str(&config).expect("the window's config");
    assert_eq!(
        asked["app"]["withGlobalTauri"],
        serde_json::Value::Bool(true),
        "the window does not inject `__TAURI__`, so the page it loads can reach no editor and \
         renders nothing"
    );

    // A page that cannot reach an editor says so rather than drawing something
    // it made up, which is the same rule stated for the case where there is no
    // window at all.
    assert!(
        page.contains("No editor is attached"),
        "the page invents something when it has no editor to ask"
    );
}
