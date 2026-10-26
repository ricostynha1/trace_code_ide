mod harness;
use tracelean_core::drt::frontend::{capture, check, Screen, Verdict};
use tracelean_core::drt::run::RunnerSpec;
use tracelean_core::surface::view::{
    directory_buffer, Breach, Buffer, BufferKind, Role, Span,
};


fn frontend(flags: &[&str]) -> RunnerSpec {
    let mut cmd = vec![env!("CARGO_BIN_EXE_tracelean-render-text").to_string()];
    cmd.extend(flags.iter().map(|f| f.to_string()));
    RunnerSpec { cmd, cwd: None }
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

fn buffers() -> Vec<Buffer> {
    let mut out = vec![
        directory_buffer("src".into(), vec![]),
        directory_buffer(
            "src".into(),
            vec![(0, "main.rs".into()), (1, "deep.rs".into()), (0, "é.rs".into())],
        ),
        of_kind(BufferKind::File { path: "a.rs".into() }, "fn main() {}", &["file.save"]),
        of_kind(BufferKind::File { path: "b.rs".into() }, "", &["file.save"]),
        of_kind(BufferKind::File { path: "c.rs".into() }, "one\ntwo\n", &["file.save"]),
        of_kind(
            BufferKind::Review { target: "a.rs".into() },
            "+ added\n- removed",
            &["observe.accept", "observe.reject"],
        ),
        of_kind(BufferKind::Menu { title: "leader".into() }, "t trace\nd drt", &["drt.run"]),
        of_kind(BufferKind::Record { title: "log".into() }, "the agent wrote a.rs", &["observe.diff"]),
    ];

    out.push(Buffer {
        id: "two-spans".into(),
        kind: BufferKind::Record { title: "two".into() },
        text: "alpha beta gamma".into(),
        spans: vec![
            Span { start: 0, stop: 5, role: Role::Entry, actions: vec!["file.open".into()] },
            Span {
                start: 6,
                stop: 10,
                role: Role::Requirement,
                actions: vec!["trace.check".into(), "trace.evidence".into()],
            },
        ],
    });

    out.push(tracelean_core::surface::produce::menu_buffer(
        "stations".into(),
        tracelean_core::surface::screen::station_entries(),
    ));
    out
}

/// @tests REQ-VIEW.frontend_is_checkable
/// @tests REQ-VIEW.rendering_is_total
/// @structural REQ-VIEW.frontend_is_checkable reason="a claim about a separate process answering over a pipe, which is not a value either side computes"
#[test]
fn a_frontend_that_draws_the_buffer_passes() {
    let report = check(&frontend(&[]), buffers()).expect("the frontend answers");
    assert!(
        report.conformant(),
        "the reference frontend failed: {:#?}",
        report.first_failure()
    );
    assert_eq!(report.checked.len(), buffers().len());
}

/// @tests REQ-VIEW.frontend_adds_nothing
/// @tests REQ-VIEW.text_is_the_content
/// @tests REQ-VIEW.rendering_is_total
#[test]
fn a_frontend_that_embroiders_invents_drops_or_refuses_is_caught() {
    let report = check(&frontend(&["--embroider"]), buffers()).expect("answers");
    let first = report.first_failure().expect("embroidery is caught");
    match &first.verdict {
        Verdict::Breached { breaches } => assert!(
            breaches.iter().any(|b| matches!(b, Breach::LineDiffers { .. })),
            "{breaches:#?}"
        ),
        other => panic!("embroidery was not reported as a breach: {other:?}"),
    }

    let report = check(&frontend(&["--invent"]), buffers()).expect("answers");
    let first = report.first_failure().expect("an invented action is caught");
    match &first.verdict {
        Verdict::Breached { breaches } => assert!(
            breaches.iter().any(|b| matches!(b, Breach::ActionInvented { .. })),
            "{breaches:#?}"
        ),
        other => panic!("an invented action was not reported as a breach: {other:?}"),
    }

    let report = check(&frontend(&["--drop"]), buffers()).expect("answers");
    let first = report.first_failure().expect("a dropped action is caught");
    match &first.verdict {
        Verdict::Breached { breaches } => assert!(
            breaches.iter().any(|b| matches!(b, Breach::ActionDropped { .. })),
            "{breaches:#?}"
        ),
        other => panic!("a dropped action was not reported as a breach: {other:?}"),
    }

    let report = check(&frontend(&["--refuse"]), buffers()).expect("answers");
    let first = report.first_failure().expect("a refusal is caught");
    assert!(
        matches!(first.verdict, Verdict::Refused { .. }),
        "a refusal was not reported as one: {:?}",
        first.verdict
    );
    assert!(
        matches!(first.buffer.kind, BufferKind::Menu { .. }),
        "the wrong buffer was blamed: {:?}",
        first.buffer.kind
    );
}

/// @tests REQ-VIEW.screen_is_readable
/// @tests REQ-VIEW.text_is_the_content
/// @structural REQ-VIEW.screen_is_readable reason="a claim about reading a process's screen, which is not a value either side computes"
#[test]
fn what_a_frontend_paints_is_read_from_the_screen_not_from_its_report() {
    let report = capture(&frontend(&["--paint"]), &Screen::default(), buffers())
        .expect("the frontend paints");
    assert!(
        report.conformant(),
        "the reference frontend painted the wrong screen: {:#?}",
        report.first_failure()
    );
    assert_eq!(report.checked.len(), buffers().len());

    let report = capture(&frontend(&["--paint", "--refuse"]), &Screen::default(), buffers())
        .expect("it paints");
    let first = report.first_failure().expect("a refusal is caught on screen");
    assert!(matches!(first.verdict, Verdict::Refused { .. }), "{:?}", first.verdict);
    assert!(matches!(first.buffer.kind, BufferKind::Menu { .. }), "{:?}", first.buffer.kind);
}

/// @tests REQ-VIEW.screen_is_readable
/// @tests REQ-VIEW.frontend_adds_nothing
/// @structural REQ-VIEW.screen_is_readable reason="a claim about two processes disagreeing with each other, which no single value expresses"
#[test]
fn a_frontend_that_reports_one_thing_and_paints_another_is_caught_only_on_screen() {
    let reported = check(&frontend(&["--two-faced"]), buffers()).expect("answers");
    assert!(
        reported.conformant(),
        "the control is meant to pass the report check: {:#?}",
        reported.first_failure()
    );

    let painted = capture(&frontend(&["--paint", "--two-faced"]), &Screen::default(), buffers())
        .expect("paints");
    let first = painted.first_failure().expect("the screen gives it away");
    match &first.verdict {
        Verdict::Breached { breaches } => assert!(
            breaches.iter().any(|b| matches!(b, Breach::LineDiffers { .. })),
            "{breaches:#?}"
        ),
        other => panic!("a false report was not caught on screen: {other:?}"),
    }
}

/// @tests REQ-DRT-COVER.law_coverage
#[test]
fn the_buffers_shown_reach_the_shapes_that_break_renderers() {
    use tracelean_core::surface::view::{actions_at, plain_text};

    let all = buffers();
    let mut kinds: std::collections::BTreeSet<&'static str> = std::collections::BTreeSet::new();
    let (mut empty, mut trailing, mut wide, mut multi_span, mut uncovered) =
        (0u64, 0u64, 0u64, 0u64, 0u64);
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
            multi_span += 1;
        }
        let covered = plain_text(buffer.clone()).join("\n").chars().count();
        if (0..covered).any(|offset| actions_at(buffer.clone(), offset).is_empty()) {
            uncovered += 1;
        }
    }
    assert_eq!(kinds.len(), 5, "not every buffer kind was shown");
    harness::covered(
        "REQ-VIEW.frontend_adds_nothing",
        &[
            ("a buffer with no text at all", empty),
            ("text ending in a newline", trailing),
            ("a character that is not one byte", wide),
            ("more than one span", multi_span),
            ("a position no span covers", uncovered),
        ],
    );
}
