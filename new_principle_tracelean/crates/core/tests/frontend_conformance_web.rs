//! The web frontend, checked against the buffers it is given.
//!
//! The same harness that checks the terminal one, pointed at a different
//! process. Nothing in `drt::frontend` knows what a terminal or a web view is,
//! which is the point of putting a frontend behind a line protocol: a second
//! frontend costs a `RunnerSpec` and no new checking machinery.
//!
//! What this establishes and what it does not: the text the page shows and the
//! actions it offers are the buffer's. Whether a button is where a person would
//! look for it is a judgement, and the evidence ladder has a rung for that.

use tracelean_core::drt::frontend::{capture, check, Screen, Verdict};
use tracelean_core::drt::run::RunnerSpec;
use tracelean_core::observe::transcript::Event;
use tracelean_core::surface::produce::{
    file_buffer, menu_buffer, record_buffer, review_buffer, Mark, MenuEntry,
};
use tracelean_core::evidence::Level;
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

/// A buffer with grades in its spans.
///
/// The first line is two spans of the *same* grade, side by side, and that is
/// the whole point: they are equal roles in two different span values. A
/// frontend comparing roles by identity sees two regions where there is one,
/// and the text it draws is identical either way.
fn graded() -> Buffer {
    file_buffer(
        "rollup.txt".into(),
        "L3 REQ-VIEW\nL4 REQ-COST".into(),
        vec![
            Mark { start: 0, stop: 5, role: Role::Level { grade: Level::L3 } },
            Mark { start: 5, stop: 11, role: Role::Level { grade: Level::L3 } },
            Mark { start: 12, stop: 23, role: Role::Level { grade: Level::L4 } },
        ],
    )
}

/// Core-produced buffers of every kind, including the shapes that break a
/// renderer written in a language whose strings are UTF-16.
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
        // A character outside the basic plane: two UTF-16 code units, one
        // character. A renderer slicing by code unit halves it.
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
        // The stations. This is the buffer the window paints as emblems rather
        // than as words, so it is the one that exercises
        // `presentation_may_be_symbolic` — and what the harness reads back has
        // to be these rows' own text.
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

/// And the text it puts on the page is the buffer's, read off the page rather
/// than taken on the frontend's word.
///
/// The stations are among the buffers, and the window paints those as emblems
/// rather than as words. What comes back is still `project`, `sandbox`,
/// `requirements`, `design` — because what is read off a screen is the
/// accessible name of each region and not the glyph, which is the amendment
/// stated as a run.
///
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

/// A frontend that names a region after the glyph it painted is caught.
///
/// Without this the symbolic permission would be unfalsifiable: a harness that
/// has only ever read correct names would pass a frontend that returned the
/// buffer's text no matter what it drew. `--mislabel` names each painted region
/// after its own painting, which is exactly the mistake the amendment allows
/// room for, and the capture must fail on it.
///
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

/// The harness has teeth against this frontend too.
///
/// A suite that has only ever seen a correct frontend would pass against a
/// check that returned nothing. So the web frontend draws wrongly on demand,
/// and both ways of being wrong are caught and named apart.
///
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

/// A span is one region, however many characters it covers.
///
/// The one thing about this frontend that the text it draws cannot show. A role
/// carries a payload now, and `===` on an object compares identity — so a
/// frontend that compared roles the obvious way would cut every character of a
/// graded span into its own region, draw exactly the same text, pass every
/// other check in this file, and put one button under each letter.
///
/// Two lines, eleven characters of one grade each. Two regions, not
/// twenty-two.
///
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
