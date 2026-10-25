use tracelean_core::drt::frontend::{capture, check, painted_screens, Screen, Verdict};
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

/// @tests REQ-LOOK.roles_drawn_in_theme_colours
#[test]
fn the_terminal_frontend_paints_each_role_in_the_themes_colour() {
    use tracelean_core::evidence::Mark as Kind;
    use tracelean_core::surface::cells::{drawn_wrong, look_of};
    use tracelean_core::surface::view::TokenKind;
    use tracelean_core::trace::annotation::Role as Claimed;

    let theme: serde_json::Value = serde_json::from_str(include_str!("../../../assets/theme.json")).unwrap();
    let mut shown = buffers();
    shown.push(file_buffer(
        "b.rs".into(),
        "P- T- +x -y fn tests\nplain".into(),
        vec![
            Mark { start: 0, stop: 1, role: Role::Mark { mark: Kind::Proved, held: true } },
            Mark { start: 1, stop: 2, role: Role::Mark { mark: Kind::Judged, held: false } },
            Mark { start: 3, stop: 4, role: Role::Mark { mark: Kind::Tested, held: true } },
            Mark { start: 6, stop: 8, role: Role::Added },
            Mark { start: 9, stop: 11, role: Role::Removed },
            Mark { start: 12, stop: 14, role: Role::Token { kind: TokenKind::Keyword } },
            Mark { start: 15, stop: 20, role: Role::Claim { role: Claimed::Tests } },
            Mark { start: 0, stop: 20, role: Role::Heading },
        ],
    ));
    let screens = painted_screens(&frontend(&["--paint"]), &Screen::default(), shown.clone()).expect("it paints");
    let mut reached = std::collections::BTreeSet::new();
    for (buffer, painted) in shown.into_iter().zip(screens) {
        let looks = buffer.spans.iter().map(|s| look_of(&theme, s.role)).collect::<Vec<_>>();
        reached.extend(looks.iter().filter(|l| l.fg.is_some()).map(|l| format!("{:?}", l.role)));
        let rows = buffer.text.matches('\n').count() as u64 + 2;
        let columns = buffer.text.lines().map(|l| l.chars().count()).max().unwrap_or(0) as u64 + 1;
        let wrong = drawn_wrong(buffer.clone(), painted, rows, columns, looks);
        assert!(wrong.is_empty(), "{:?} painted out of its look: {:#?}", buffer.kind, &wrong[..wrong.len().min(3)]);
    }
    assert!(reached.len() >= 8, "only {} coloured roles were painted: {reached:?}", reached.len());
}

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
