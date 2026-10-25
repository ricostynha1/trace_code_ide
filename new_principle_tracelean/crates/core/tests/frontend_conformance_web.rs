use tracelean_core::drt::frontend::{capture, check, Screen, Verdict};
use tracelean_core::drt::run::RunnerSpec;
use tracelean_core::observe::transcript::Event;
use tracelean_core::surface::produce::{
    file_buffer, menu_buffer, record_buffer, review_buffer, Mark, MenuEntry,
};
use tracelean_core::evidence::Mark as Kind;
use tracelean_core::surface::screen::station_entries;
use tracelean_core::surface::view::{directory_buffer, Breach, Buffer, Role};

fn project_root() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .to_path_buf()
}

fn frontend(flags: &[&str]) -> RunnerSpec {
    let mut cmd = vec![
        "node".to_string(),
        project_root().join("web/src/frontend.ts").display().to_string(),
    ];
    cmd.extend(flags.iter().map(|f| f.to_string()));
    RunnerSpec { cmd, cwd: None }
}

fn graded() -> Buffer {
    let tested = Role::Mark { mark: Kind::Tested, held: true };
    file_buffer(
        "rollup.txt".into(),
        "-T REQ-VIEW\n-P REQ-COST".into(),
        vec![
            Mark { start: 0, stop: 5, role: tested },
            Mark { start: 5, stop: 11, role: tested },
            Mark { start: 12, stop: 23, role: Role::Mark { mark: Kind::Proved, held: true } },
        ],
    )
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
        file_buffer(
            "score.txt".into(),
            "𝄞 clef\nsecond".into(),
            vec![Mark { start: 0, stop: 6, role: Role::Heading }],
        ),
        review_buffer("a.rs".into(), "one\ntwo\nthree".into(), "one\nTWO\nthree".into()),
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
            vec![Event { kind: "wrote".into(), text: "a.rs".into() }],
        ),
        graded(),
        menu_buffer("stations".into(), station_entries()),
    ]
}

/// @tests REQ-VIEW.frontend_is_checkable
/// @tests REQ-VIEW.rendering_is_total
/// @structural REQ-VIEW.frontend_is_checkable reason="a claim about a second frontend, in another language, answering over the same protocol — not a value either side computes"
#[test]
#[ignore = "runs the web frontend under node; run with --ignored"]
fn the_web_frontend_draws_every_buffer_it_is_given() {
    let report = check(&frontend(&["--protocol"]), buffers()).expect("it answers");
    assert!(report.conformant(), "the web frontend failed: {:#?}", report.first_failure());
    for checked in &report.checked {
        assert_eq!(checked.verdict, Verdict::Conformant, "{:?}", checked.buffer.kind);
    }
}

/// @tests REQ-VIEW.screen_is_readable
/// @tests REQ-VIEW.presentation_may_be_symbolic
/// @structural REQ-VIEW.screen_is_readable reason="a claim about reading what a process rendered, which is not a value either side computes"
/// @structural REQ-VIEW.presentation_may_be_symbolic reason="a claim about what a second process painted and what its painting is named, which is not a value either side computes"
#[test]
#[ignore = "runs the web frontend under node; run with --ignored"]
fn what_the_web_frontend_renders_is_the_buffer() {
    let report =
        capture(&frontend(&["--paint"]), &Screen::default(), buffers()).expect("it renders");
    assert!(
        report.conformant(),
        "the web frontend rendered the wrong text: {:#?}",
        report.first_failure()
    );
}

/// @tests REQ-VIEW.presentation_may_be_symbolic
#[test]
#[ignore = "runs the web frontend under node; run with --ignored"]
fn a_symbol_named_after_itself_is_caught() {
    let report =
        capture(&frontend(&["--paint", "--mislabel"]), &Screen::default(), buffers())
            .expect("it renders");
    assert!(
        !report.conformant(),
        "a frontend that named a station after its own emblem was not caught"
    );
}

/// @tests REQ-VIEW.frontend_adds_nothing
/// @tests REQ-VIEW.text_is_the_content
#[test]
#[ignore = "runs the web frontend under node; run with --ignored"]
fn a_web_frontend_that_embroiders_or_invents_is_caught() {
    let report = check(&frontend(&["--protocol", "--embroider"]), buffers()).expect("answers");
    let first = report.first_failure().expect("embroidery is caught");
    match &first.verdict {
        Verdict::Breached { breaches } => assert!(
            breaches.iter().any(|b| matches!(b, Breach::LineDiffers { .. })),
            "{breaches:#?}"
        ),
        other => panic!("embroidery was not reported as a breach: {other:?}"),
    }

    let report = check(&frontend(&["--protocol", "--invent"]), buffers()).expect("answers");
    let first = report.first_failure().expect("an invented action is caught");
    match &first.verdict {
        Verdict::Breached { breaches } => assert!(
            breaches.iter().any(|b| matches!(b, Breach::ActionInvented { .. })),
            "{breaches:#?}"
        ),
        other => panic!("an invented action was not reported as a breach: {other:?}"),
    }
}

/// @tests REQ-VIEW.structure_over_text
/// @structural REQ-VIEW.structure_over_text reason="a claim about how a second process divided a line, which is not a value either side computes"
#[test]
#[ignore = "runs the web frontend under node; run with --ignored"]
fn the_web_frontend_draws_a_span_as_one_region() {
    use tracelean_core::drt::run::Runner;

    let mut runner = Runner::start(&frontend(&["--regions"])).expect("the frontend starts");
    let reply = runner
        .ask(1, "REQ-VIEW.structure_over_text", &serde_json::to_value(graded()).unwrap())
        .expect("it answers");
    assert_eq!(
        reply.output,
        Some(serde_json::json!([1, 1])),
        "a graded line was cut into more regions than it has spans: {:?}",
        reply
    );
}
