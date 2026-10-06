//! The terminal frontend, checked against the buffers it is given.
//!
//! This is the same harness that checks any other frontend, pointed at the real
//! binary rather than at a fixture: `check` reads what it says it drew, and
//! `capture` reads the screen it painted. Nothing here knows what a terminal is.
//!
//! The buffers are the ones the core produces, not hand-built ones — a frontend
//! that only ever sees tidy buffers is a frontend nobody has tested.

use tracelean_core::drt::frontend::{capture, check, Screen, Verdict};
use tracelean_core::drt::run::RunnerSpec;
use tracelean_core::observe::transcript::Event;
use tracelean_core::surface::keymap;
use tracelean_core::surface::produce::{
    file_buffer, menu_buffer, record_buffer, review_buffer, Mark, MenuEntry,
};
use tracelean_core::surface::view::{directory_buffer, Buffer, BufferKind, Role};

fn frontend(flags: &[&str]) -> RunnerSpec {
    let mut cmd = vec![env!("CARGO_BIN_EXE_tracelean-tui").to_string()];
    cmd.extend(flags.iter().map(|f| f.to_string()));
    RunnerSpec { cmd, cwd: None }
}

/// One buffer of every kind, every one of them produced by the core, and each
/// carrying a shape that breaks a careless renderer: no text at all, a trailing
/// newline, a character that is not one byte, marks that overlap, and a row
/// with nothing to offer.
fn buffers() -> Vec<Buffer> {
    vec![
        directory_buffer("src".into(), vec![]),
        directory_buffer(
            "src".into(),
            vec![(0, "main.rs".into()), (1, "deep.rs".into()), (0, "é.rs".into())],
        ),
        file_buffer("a.rs".into(), String::new(), vec![]),
        file_buffer(
            "a.rs".into(),
            "fn one() {}\nfn two() {}\n".into(),
            vec![
                Mark { start: 0, stop: 11, role: Role::Heading },
                Mark { start: 5, stop: 20, role: Role::Requirement },
                Mark { start: 12, stop: 23, role: Role::Heading },
            ],
        ),
        review_buffer("a.rs".into(), "one\ntwo\nthree".into(), "one\nTWO\nthree".into()),
        review_buffer("b.rs".into(), String::new(), String::new()),
        menu_buffer(
            "leader".into(),
            vec![
                MenuEntry { key: "t".into(), description: "trace".into(), action: None },
                MenuEntry {
                    key: "u".into(),
                    description: "undo é".into(),
                    action: Some("history.undo".into()),
                },
            ],
        ),
        record_buffer(
            "agent".into(),
            vec![
                Event { kind: "wrote".into(), text: "a.rs".into() },
                Event { kind: "ran".into(), text: "cargo test".into() },
            ],
        ),
    ]
}

/// What it says it drew is what the buffer says.
///
/// @tests REQ-VIEW.frontend_is_checkable
/// @tests REQ-VIEW.rendering_is_total
/// @structural REQ-VIEW.rendering_is_total reason="a claim about a separate process answering for every kind, which is not a value either side computes"
#[test]
fn the_terminal_frontend_draws_every_buffer_it_is_given() {
    let report = check(&frontend(&["--protocol"]), buffers()).expect("it answers");
    assert!(report.conformant(), "the terminal frontend failed: {:#?}", report.first_failure());
    assert_eq!(report.checked.len(), buffers().len());
    for checked in &report.checked {
        assert_eq!(checked.verdict, Verdict::Conformant, "{:?}", checked.buffer.kind);
    }
}

/// And what it paints is the same thing, read off the screen rather than taken
/// on its word.
///
/// @tests REQ-VIEW.screen_is_readable
/// @structural REQ-VIEW.screen_is_readable reason="a claim about reading a process's screen, which is not a value either side computes"
#[test]
fn what_the_terminal_frontend_paints_is_the_buffer() {
    let report =
        capture(&frontend(&["--paint"]), &Screen::default(), buffers()).expect("it paints");
    assert!(
        report.conformant(),
        "the terminal frontend painted the wrong screen: {:#?}",
        report.first_failure()
    );
}

/// Every action this frontend can offer is one the keymap dispatches.
///
/// An action only the representation knows about is one no key reaches; an
/// action only this frontend knows about is one nobody can trace.
///
/// @tests REQ-VIEW.frontend_adds_nothing
/// @structural REQ-VIEW.frontend_adds_nothing reason="a claim about the vocabulary a running process may use, read off what it offered"
#[test]
fn the_terminal_frontend_offers_no_action_the_keymap_lacks() {
    let report = check(&frontend(&["--protocol"]), buffers()).expect("it answers");
    let mut offered = 0;
    for checked in &report.checked {
        for action in tracelean_core::surface::view::declared_actions(&checked.buffer) {
            assert!(
                keymap::ACTIONS.contains(&action.as_str()),
                "`{action}` is offered and the keymap does not have it"
            );
            offered += 1;
        }
    }
    assert!(offered > 3, "only {offered} actions were reachable, so this checked little");
}

/// Every kind of buffer reaches the frontend, and the shapes that break
/// renderers reach it too.
///
/// @tests REQ-DRT-COVER.law_coverage
#[test]
fn the_buffers_shown_reach_every_kind_and_the_awkward_shapes() {
    let all = buffers();
    let mut kinds = std::collections::BTreeSet::new();
    let (mut empty, mut trailing, mut wide, mut many_spans, mut no_spans) = (0, 0, 0, 0, 0);
    for buffer in &all {
        kinds.insert(match buffer.kind {
            BufferKind::File { .. } => "file",
            BufferKind::Directory { .. } => "directory",
            BufferKind::Review { .. } => "review",
            BufferKind::Menu { .. } => "menu",
            BufferKind::Record { .. } => "record",
        });
        if buffer.text.is_empty() {
            empty += 1;
        }
        if buffer.text.ends_with('\n') {
            trailing += 1;
        }
        if buffer.text.chars().count() != buffer.text.len() {
            wide += 1;
        }
        if buffer.spans.len() > 1 {
            many_spans += 1;
        }
        if buffer.spans.is_empty() {
            no_spans += 1;
        }
    }
    assert_eq!(kinds.len(), 5, "not every kind was shown: {kinds:?}");
    assert!(empty > 0 && trailing > 0 && wide > 0 && many_spans > 0 && no_spans > 0,
        "a shape that breaks renderers was not reached: empty={empty} trailing={trailing} \
         wide={wide} many_spans={many_spans} no_spans={no_spans}");
}
