//! What is opened and what is shown, checked against the model.
//!
//! Ten bindings over one value: whether a screen holds together, where its panes
//! go, showing and opening a buffer, splitting, closing, resizing, moving the
//! focus, and the two menus the session always has.
//!
//! **The layout's schema is nested by hand rather than declared recursively.**
//! `drt::schema` has no recursive form — there is no way to say "a layout, and
//! inside it, layouts". So `layout(depth)` builds the grammar three levels deep
//! and stops. That is a real limit on what these runs cover: a layout nested
//! four deep is never generated, and no floor here can claim otherwise. The fix
//! is a recursive schema, which is `REQ-DRT-SCHEMA`'s work and not this
//! clause's; until then the limit is written here rather than left to be
//! discovered.

mod harness;

use std::collections::BTreeMap;

use tracelean_core::drt::gen;
use tracelean_core::drt::run::{run, RunOptions};
use tracelean_core::drt::schema::Schema;
use tracelean_core::surface::screen::{
    close_buffer, coherent, distinct_panes, focus_step, open_buffer, pane_ids, panes, place,
    resize_focus, show_buffer, shown_buffers, split_focus, strip, Axis, Direction, Layout, Rect,
    Screen,
};
use tracelean_core::surface::view::Buffer;

mod support;

// ───────────────────────────────────────────────────────────── the schema

fn strukt(fields: &[(&str, Schema)]) -> Schema {
    Schema::Struct {
        fields: fields.iter().map(|(name, s)| (name.to_string(), s.clone())).collect(),
    }
}

fn axis() -> Schema {
    Schema::simple_enum(&["across", "down"])
}

fn direction() -> Schema {
    Schema::simple_enum(&["left", "right", "up", "down"])
}

/// Small regions, and zero among them.
///
/// A terminal is eighty by twenty-four and a layout bug does not care; what
/// finds one is a region too small to give every pane a character, which is why
/// the sizes here are single digits rather than realistic.
fn rect() -> Schema {
    strukt(&[
        ("left", Schema::Nat { max: Some(2), edges: vec![0] }),
        ("top", Schema::Nat { max: Some(2), edges: vec![0] }),
        ("width", Schema::Nat { max: Some(9), edges: vec![0, 1, 2] }),
        ("height", Schema::Nat { max: Some(9), edges: vec![0, 1, 2] }),
    ])
}

/// Identities one character long, drawn from a three-letter alphabet.
///
/// Realistic names would make this suite worthless. A focus matches a pane, and
/// a pane's buffer is one the session opened, only when two independently drawn
/// strings are equal — and with twelve random characters to choose from that
/// almost never happens, so almost every generated screen would be an
/// incoherent one and the agreeing case would go untested. Short names make the
/// coincidences the law is about ordinary.
fn pane_id() -> Schema {
    // A zero maximum on the generated half, so the alphabet is the three named
    // letters and the empty name — which is itself a case worth having, since a
    // pane with no name is one a focus can still match.
    Schema::Str { max_len: Some(0), examples: vec!["a".into(), "b".into(), "c".into()] }
}

fn buffer_id() -> Schema {
    // `é` is here for the strip: its rows are numbered by character offset, and
    // a frontend counting UTF-16 code units or bytes would place the span
    // differently from the model.
    Schema::Str { max_len: Some(0), examples: vec!["x".into(), "y".into(), "é".into()] }
}

/// Identities drawn from the workbench's names and one other, so that a home
/// pane is placed in some screens and gone from others.
fn home_pane_id() -> Schema {
    Schema::Str {
        max_len: Some(0),
        examples: vec!["a".into(), "explorer".into(), "document".into(), "side".into()],
    }
}

/// A layout, nested to a fixed depth. See the note at the top of this file.
fn layout(depth: usize) -> Schema {
    layout_named(depth, pane_id)
}

fn layout_named(depth: usize, pane_id: fn() -> Schema) -> Schema {
    let mut variants = BTreeMap::new();
    variants.insert(
        "pane".to_string(),
        Some(Box::new(strukt(&[("id", pane_id()), ("buffer", buffer_id())]))),
    );
    if depth > 0 {
        variants.insert(
            "split".to_string(),
            Some(Box::new(strukt(&[
                ("axis", axis()),
                (
                    "parts",
                    Schema::List {
                        inner: Box::new(Schema::Tuple {
                            items: vec![
                                // Zero among the weights: a part weighing
                                // nothing is the case that makes a share
                                // allocation divide by nothing.
                                Schema::Nat { max: Some(4), edges: vec![0, 1] },
                                layout_named(depth - 1, pane_id),
                            ],
                        }),
                        max_len: Some(3),
                    },
                ),
            ]))),
        );
    }
    Schema::Enum { variants }
}

fn buffer() -> Schema {
    let mut kinds = BTreeMap::new();
    kinds.insert(
        "file".to_string(),
        Some(Box::new(strukt(&[(
            "path",
            // One name at several depths, for tabs that must tell
            // same-named files apart by their folders.
            Schema::Str {
                max_len: Some(4),
                examples: vec!["a.rs".into(), "x/a.rs".into(), "y/x/a.rs".into(), "z/x/a.rs".into(), "/a.rs".into()],
            },
        )]))),
    );
    kinds.insert(
        "menu".to_string(),
        Some(Box::new(strukt(&[(
            "title",
            Schema::Str { max_len: Some(4), examples: vec!["leader".into()] },
        )]))),
    );
    strukt(&[
        ("id", buffer_id()),
        ("kind", Schema::Enum { variants: kinds }),
        ("text", Schema::Str { max_len: None, examples: vec!["".into(), "one\ntwo".into()] }),
        (
            "spans",
            Schema::List {
                inner: Box::new(strukt(&[
                    ("start", Schema::Nat { max: Some(4), edges: vec![0] }),
                    ("stop", Schema::Nat { max: Some(4), edges: vec![0] }),
                    ("role", Schema::simple_enum(&["plain", "entry", "path"])),
                    (
                        "actions",
                        Schema::List {
                            inner: Box::new(Schema::Str {
                                max_len: None,
                                examples: vec!["file.open".into()],
                            }),
                            max_len: Some(1),
                        },
                    ),
                ])),
                max_len: Some(1),
            },
        ),
    ])
}

fn screen() -> Schema {
    strukt(&[
        ("opened", Schema::List { inner: Box::new(buffer()), max_len: Some(3) }),
        ("layout", layout(2)),
        ("focus", pane_id()),
        ("nextPane", Schema::Nat { max: Some(3), edges: vec![0] }),
    ])
}

/// A screen whose panes may carry the workbench's names.
fn home_screen() -> Schema {
    strukt(&[
        ("opened", Schema::List { inner: Box::new(buffer()), max_len: Some(3) }),
        ("layout", layout_named(2, home_pane_id)),
        ("focus", home_pane_id()),
        ("nextPane", Schema::Nat { max: Some(3), edges: vec![0] }),
    ])
}

fn kind() -> Schema {
    let text = || Schema::Str { max_len: Some(3), examples: vec![".".into(), "a.rs".into(), "requirement R".into(), "judge R".into(), "keys".into()] };
    let mut kinds = BTreeMap::new();
    for (name, field) in
        [("file", "path"), ("directory", "path"), ("review", "target"), ("menu", "title"), ("record", "title")]
    {
        kinds.insert(name.to_string(), Some(Box::new(strukt(&[(field, text())]))));
    }
    Schema::Enum { variants: kinds }
}

fn one_screen() -> Schema {
    strukt(&[("screen", screen())])
}

// ───────────────────────────────────────────────────────────── running one

fn check(clause: &str, function: &str, entry: &str, arguments: &[&str], schema: Schema, seed: u64) {
    let op = format!("REQ-SCREEN.{clause}");
    let scratch = harness::scratch(&format!("screen-{clause}"));
    let implementation = harness::rust_runner("REQ-SCREEN", clause, entry, &scratch);
    let model = harness::lean_runner("TraceLean.Screen", function, &op, arguments, &scratch);

    let result = run(
        &op,
        &schema,
        &model,
        &implementation,
        RunOptions { seed, cases: 2_000, shrink_rounds: 100 },
    )
    .expect("both runners answer");

    support::agreed(&result);

    let _ = std::fs::remove_dir_all(&scratch);
}

// ─────────────────────────────────────────────── reading generated values

fn generated(schema: &Schema, seed: u64, cases: usize) -> Vec<serde_json::Value> {
    let mut rng = gen::Rng::new(seed);
    (0..cases).map(|_| gen::value(schema, &mut rng)).collect()
}

fn screen_of(value: &serde_json::Value) -> Screen {
    serde_json::from_value(value["screen"].clone()).expect("the schema generates what parses")
}

fn splits_within(layout: &Layout) -> Vec<&Vec<(u64, Layout)>> {
    match layout {
        Layout::Pane { .. } => Vec::new(),
        Layout::Split { parts, .. } => {
            let mut out = vec![parts];
            for (_, inner) in parts {
                out.extend(splits_within(inner));
            }
            out
        }
    }
}

/// Whether the placed rectangles tile the region: every cell inside it covered
/// exactly once, and nothing placed outside it.
///
/// This is the half a differential run cannot reach. Comparing Lean against
/// Rust establishes that the two agree; it says nothing about whether what they
/// agree on is a tiling, and two implementations of the same wrong arithmetic
/// would agree perfectly. So the law itself is checked here, over the same
/// generated regions.
fn tiles(rect: Rect, placed: &[(String, Rect)]) -> bool {
    for x in rect.left..rect.left + rect.width {
        for y in rect.top..rect.top + rect.height {
            let covering = placed
                .iter()
                .filter(|(_, r)| {
                    r.left <= x && x < r.left + r.width && r.top <= y && y < r.top + r.height
                })
                .count();
            if covering != 1 {
                return false;
            }
        }
    }
    placed.iter().all(|(_, r)| {
        r.width == 0
            || r.height == 0
            || (rect.left <= r.left
                && r.left + r.width <= rect.left + rect.width
                && rect.top <= r.top
                && r.top + r.height <= rect.top + rect.height)
    })
}

/// Every weight in a layout, outermost first.
fn weights_of(layout: &Layout) -> Vec<u64> {
    match layout {
        Layout::Pane { .. } => Vec::new(),
        Layout::Split { parts, .. } => {
            let mut out: Vec<u64> = parts.iter().map(|(weight, _)| *weight).collect();
            for (_, inner) in parts {
                out.extend(weights_of(inner));
            }
            out
        }
    }
}

/// The parts of the split that directly contains `pane`, and its index among
/// them — the divider a resize would move.
///
/// Replicated here from the implementation's private `index_holding` rather
/// than exported: a counting helper that called the code under test would count
/// what that code believes rather than what the value is.
fn enclosing(pane: &str, layout: &Layout) -> Option<(usize, Vec<u64>)> {
    let Layout::Split { parts, .. } = layout else { return None };
    let at = parts.iter().position(|(_, inner)| pane_ids(inner).iter().any(|id| id == pane));
    match at {
        None => None,
        Some(index) => {
            let inner = &parts[index].1;
            if matches!(inner, Layout::Split { .. }) && parts.len() == 1 {
                enclosing(pane, inner)
            } else {
                Some((index, parts.iter().map(|(weight, _)| *weight).collect()))
            }
        }
    }
}

// ─────────────────────────────────────────────────── whether it holds together

/// @drt REQ-SCREEN.every_pane_is_opened
/// @tests REQ-SCREEN.every_pane_is_opened
/// @tests REQ-SCREEN.focus_is_placed
/// @tests REQ-SCREEN.panes_are_distinct
/// @tests REQ-SCREEN.screen_is_a_value
#[test]
#[ignore = "builds a Lean package and a Rust crate; run with --ignored"]
fn model_and_implementation_agree_on_whether_a_screen_holds_together() {
    check(
        "every_pane_is_opened",
        "TraceLean.Screen.coherent",
        "crates/core/src/surface/screen.rs::coherent",
        &["screen"],
        one_screen(),
        41,
    );
}

/// @tests REQ-DRT-COVER.law_coverage
#[test]
fn generation_reaches_every_way_a_screen_falls_apart() {
    let (mut whole, mut unopened, mut adrift, mut twinned) = (0u64, 0u64, 0u64, 0u64);
    for value in generated(&one_screen(), 41, 2_000) {
        let s = screen_of(&value);
        let opened: Vec<String> = s.opened.iter().map(|b| b.id.clone()).collect();
        if coherent(s.clone()) {
            whole += 1;
        }
        if !shown_buffers(&s.layout).iter().all(|b| opened.contains(b)) {
            unopened += 1;
        }
        if !pane_ids(&s.layout).contains(&s.focus) {
            adrift += 1;
        }
        if !distinct_panes(&s.layout) {
            twinned += 1;
        }
    }
    support::covered(
        "REQ-SCREEN.every_pane_is_opened",
        &[
            ("a coherent screen", whole),
            ("a pane showing a buffer nothing opened", unopened),
            ("a focus on no pane at all", adrift),
            ("two panes sharing an identity", twinned),
        ],
    );
}

// ─────────────────────────────────────────────────────────────── placing

fn rect_and_layout() -> Schema {
    strukt(&[("rect", rect()), ("layout", layout(2))])
}

/// @drt REQ-SCREEN.layout_tiles_the_region
/// @tests REQ-SCREEN.layout_tiles_the_region
#[test]
#[ignore = "builds a Lean package and a Rust crate; run with --ignored"]
fn model_and_implementation_agree_on_where_the_panes_go() {
    check(
        "layout_tiles_the_region",
        "TraceLean.Screen.place",
        "crates/core/src/surface/screen.rs::place",
        &["rect", "layout"],
        rect_and_layout(),
        43,
    );
}

/// @tests REQ-DRT-COVER.law_coverage
#[test]
fn generation_reaches_the_regions_that_cannot_be_divided_evenly() {
    let (mut divided, mut nested, mut weightless, mut cramped, mut nothing) =
        (0u64, 0u64, 0u64, 0u64, 0u64);
    for value in generated(&rect_and_layout(), 43, 2_000) {
        let rect: Rect = serde_json::from_value(value["rect"].clone()).expect("a rect");
        let layout: Layout = serde_json::from_value(value["layout"].clone()).expect("a layout");
        let splits = splits_within(&layout);
        if !splits.is_empty() {
            divided += 1;
        }
        if splits.len() > 1 {
            nested += 1;
        }
        if splits.iter().any(|parts| parts.iter().any(|(weight, _)| *weight == 0)) {
            weightless += 1;
        }
        let count = pane_ids(&layout).len() as u64;
        if count > rect.width.min(rect.height) {
            cramped += 1;
        }
        if rect.width == 0 || rect.height == 0 {
            nothing += 1;
        }
        // The clause itself, over every generated region. Every pane gets a
        // rectangle, and the rectangles tile the region — which is what
        // `layout_tiles_the_region` says and what no comparison of two
        // implementations could establish.
        let placed = place(rect, layout.clone());
        assert_eq!(placed.len(), pane_ids(&layout).len());
        // A layout holding no pane places nothing, and there is no tiling to
        // speak of — which is what the clause says and why it says it.
        if !placed.is_empty() {
            assert!(tiles(rect, &placed), "{rect:?} not tiled by {placed:?}");
        }
    }
    support::covered(
        "REQ-SCREEN.layout_tiles_the_region",
        &[
            ("a region divided at least once", divided),
            ("a split nested inside a split", nested),
            ("a part weighing nothing", weightless),
            ("a region too small for its parts", cramped),
            ("a region of no extent", nothing),
        ],
    );
}

// ──────────────────────────────────────────────────── showing and opening

fn id_and_screen() -> Schema {
    strukt(&[("id", buffer_id()), ("screen", screen())])
}

/// @drt REQ-SCREEN.opened_outlives_shown
/// @tests REQ-SCREEN.opened_outlives_shown
#[test]
#[ignore = "builds a Lean package and a Rust crate; run with --ignored"]
fn model_and_implementation_agree_on_showing_a_buffer() {
    check(
        "opened_outlives_shown",
        "TraceLean.Screen.showBuffer",
        "crates/core/src/surface/screen.rs::show_buffer",
        &["id", "screen"],
        id_and_screen(),
        47,
    );
}

/// @tests REQ-DRT-COVER.law_coverage
#[test]
fn generation_reaches_showing_what_is_held_and_what_is_not() {
    let (mut held, mut absent, mut already) = (0u64, 0u64, 0u64);
    for value in generated(&id_and_screen(), 47, 2_000) {
        let id: String = serde_json::from_value(value["id"].clone()).expect("an id");
        let s = screen_of(&value);
        if s.opened.iter().any(|b| b.id == id) {
            held += 1;
        } else {
            absent += 1;
        }
        let showing = panes(&s.layout).into_iter().find(|(pane, _)| *pane == s.focus);
        if showing.map(|(_, buffer)| buffer == id).unwrap_or(false) {
            already += 1;
        }
        // The clause: showing moves what a pane points at and produces nothing,
        // so the session holds exactly what it held.
        let after = show_buffer(id.clone(), s.clone());
        assert_eq!(after.opened, s.opened);
        assert_eq!(pane_ids(&after.layout), pane_ids(&s.layout));
    }
    support::covered(
        "REQ-SCREEN.opened_outlives_shown",
        &[
            ("shown a buffer the session holds", held),
            ("shown a buffer nothing opened", absent),
            ("shown the buffer already in the focused pane", already),
        ],
    );
}

fn buffer_and_screen() -> Schema {
    strukt(&[("buffer", buffer()), ("screen", screen())])
}

/// @drt REQ-SCREEN.station_opens_a_buffer
/// @tests REQ-SCREEN.station_opens_a_buffer
#[test]
#[ignore = "builds a Lean package and a Rust crate; run with --ignored"]
fn model_and_implementation_agree_on_opening_a_buffer() {
    check(
        "station_opens_a_buffer",
        "TraceLean.Screen.openBuffer",
        "crates/core/src/surface/screen.rs::open_buffer",
        &["buffer", "screen"],
        buffer_and_screen(),
        53,
    );
}

/// @tests REQ-DRT-COVER.law_coverage
#[test]
fn generation_reaches_opening_something_new_and_something_held() {
    let (mut fresh, mut again) = (0u64, 0u64);
    for value in generated(&buffer_and_screen(), 53, 2_000) {
        let id: String =
            serde_json::from_value(value["buffer"]["id"].clone()).expect("a buffer id");
        let s = screen_of(&value);
        if s.opened.iter().any(|b| b.id == id) {
            again += 1;
        } else {
            fresh += 1;
        }
        // The clause: opening shows the buffer and changes the layout in no
        // other way, and a buffer already held is not held twice.
        let offered: Buffer =
            serde_json::from_value(value["buffer"].clone()).expect("a buffer");
        let after = open_buffer(offered, s.clone());
        assert_eq!(pane_ids(&after.layout), pane_ids(&s.layout));
        if s.opened.iter().any(|b| b.id == id) {
            assert_eq!(after.opened, s.opened, "what was held is what is kept");
        } else {
            assert_eq!(after.opened.len(), s.opened.len() + 1);
        }
    }
    support::covered(
        "REQ-SCREEN.station_opens_a_buffer",
        &[("opened something new", fresh), ("opened something already held", again)],
    );
}

// ──────────────────────────────────────────────────────────── the workbench

fn three_buffers() -> Schema {
    strukt(&[("listing", buffer()), ("document", buffer()), ("side", buffer())])
}

/// @drt REQ-SCREEN.workbench_has_three_places
/// @tests REQ-SCREEN.workbench_has_three_places
#[test]
#[ignore = "builds a Lean package and a Rust crate; run with --ignored"]
fn model_and_implementation_agree_on_the_workbench() {
    check(
        "workbench_has_three_places",
        "TraceLean.Screen.workbench",
        "crates/core/src/surface/screen.rs::workbench",
        &["listing", "document", "side"],
        three_buffers(),
        83,
    );
}

/// @tests REQ-DRT-COVER.law_coverage
#[test]
fn generation_reaches_three_buffers_and_one_offered_twice() {
    use tracelean_core::surface::screen::{workbench, EXPLORER};
    let (mut distinct, mut shared) = (0u64, 0u64);
    for value in generated(&three_buffers(), 83, 2_000) {
        let buffer = |name: &str| -> Buffer {
            serde_json::from_value(value[name].clone()).expect("a buffer")
        };
        let (listing, document, side) = (buffer("listing"), buffer("document"), buffer("side"));
        let ids = [&listing.id, &document.id, &side.id];
        let unique = ids.iter().collect::<std::collections::BTreeSet<_>>().len();
        if unique == 3 {
            distinct += 1;
        } else {
            shared += 1;
        }
        // The clause: three places, the listing in the first, focus on it, and
        // every buffer held once.
        let opened = workbench(listing.clone(), document, side);
        assert_eq!(pane_ids(&opened.layout).len(), 3);
        assert_eq!(panes(&opened.layout)[0], (EXPLORER.to_string(), listing.id));
        assert_eq!(opened.focus, EXPLORER);
        assert_eq!(opened.opened.len(), unique);
        assert!(coherent(opened));
    }
    support::covered(
        "REQ-SCREEN.workbench_has_three_places",
        &[
            ("three different buffers", distinct),
            ("one buffer offered for two places", shared),
        ],
    );
}

fn kind_and_screen() -> Schema {
    strukt(&[("kind", kind()), ("screen", home_screen())])
}

/// @drt REQ-SCREEN.buffer_goes_home
/// @tests REQ-SCREEN.buffer_goes_home
#[test]
#[ignore = "builds a Lean package and a Rust crate; run with --ignored"]
fn model_and_implementation_agree_on_where_a_buffer_goes() {
    check(
        "buffer_goes_home",
        "TraceLean.Screen.destination",
        "crates/core/src/surface/screen.rs::destination",
        &["kind", "screen"],
        kind_and_screen(),
        89,
    );
}

/// @tests REQ-DRT-COVER.law_coverage
#[test]
fn generation_reaches_a_home_that_is_placed_and_one_that_is_gone() {
    use tracelean_core::surface::screen::{destination, home_of};
    use tracelean_core::surface::view::BufferKind;
    let (mut placed, mut gone) = (0u64, 0u64);
    let mut kinds = std::collections::BTreeSet::new();
    for value in generated(&kind_and_screen(), 89, 2_000) {
        let kind: BufferKind = serde_json::from_value(value["kind"].clone()).expect("a kind");
        let s = screen_of(&value);
        kinds.insert(format!("{:?}", std::mem::discriminant(&kind)));
        let home = home_of(&kind);
        let there = destination(kind.clone(), s.clone());
        // The clause: home while it is placed, the focus once it is not.
        if pane_ids(&s.layout).iter().any(|pane| pane == home) {
            placed += 1;
            assert_eq!(there, home);
        } else {
            gone += 1;
            assert_eq!(there, s.focus);
        }
    }
    support::covered(
        "REQ-SCREEN.buffer_goes_home",
        &[
            ("a kind whose home is placed", placed),
            ("a kind whose home is gone", gone),
            ("each of the five kinds", kinds.len() as u64),
        ],
    );
}

// ─────────────────────────────────────────── splitting, closing, resizing

fn axis_and_screen() -> Schema {
    strukt(&[("axis", axis()), ("screen", screen())])
}

/// @drt REQ-SCREEN.split_keeps_the_buffer
/// @tests REQ-SCREEN.split_keeps_the_buffer
#[test]
#[ignore = "builds a Lean package and a Rust crate; run with --ignored"]
fn model_and_implementation_agree_on_splitting_a_pane() {
    check(
        "split_keeps_the_buffer",
        "TraceLean.Screen.splitFocus",
        "crates/core/src/surface/screen.rs::split_focus",
        &["axis", "screen"],
        axis_and_screen(),
        59,
    );
}

/// @tests REQ-DRT-COVER.law_coverage
#[test]
fn generation_reaches_splitting_a_pane_that_is_there_and_one_that_is_not() {
    let (mut present, mut missing, mut inside) = (0u64, 0u64, 0u64);
    for value in generated(&axis_and_screen(), 59, 2_000) {
        let s = screen_of(&value);
        if pane_ids(&s.layout).contains(&s.focus) {
            present += 1;
            if matches!(s.layout, Layout::Split { .. }) {
                inside += 1;
            }
        } else {
            missing += 1;
        }
        // The clause: the buffer that was there is in both halves, so neither
        // is empty — and a focus on no pane splits nothing.
        let axis: Axis = serde_json::from_value(value["axis"].clone()).expect("an axis");
        let before = panes(&s.layout);
        let after = split_focus(axis, s.clone());
        match before.iter().find(|(pane, _)| *pane == s.focus) {
            None => assert_eq!(panes(&after.layout), before),
            Some((_, buffer)) => {
                assert_eq!(panes(&after.layout).len(), before.len() + 1);
                let showing =
                    panes(&after.layout).into_iter().filter(|(_, b)| b == buffer).count();
                assert!(showing >= 2, "both halves show what the pane showed");
            }
        }
    }
    support::covered(
        "REQ-SCREEN.split_keeps_the_buffer",
        &[
            ("split a pane that exists", present),
            ("split with the focus on no pane", missing),
            ("split a pane inside a split", inside),
        ],
    );
}

fn target_and_screen() -> Schema {
    strukt(&[("buffer", buffer_id()), ("screen", screen())])
}

/// @drt REQ-SCREEN.close_collapses_the_pane
/// @tests REQ-SCREEN.close_collapses_the_pane
#[test]
#[ignore = "builds a Lean package and a Rust crate; run with --ignored"]
fn model_and_implementation_agree_on_closing_a_buffer() {
    check(
        "close_collapses_the_pane",
        "TraceLean.Screen.closeBuffer",
        "crates/core/src/surface/screen.rs::close_buffer",
        &["buffer", "screen"],
        target_and_screen(),
        61,
    );
}

/// @tests REQ-DRT-COVER.law_coverage
#[test]
fn generation_reaches_closing_the_last_buffer_and_one_of_many() {
    let (mut several, mut last, mut everywhere, mut unshown) = (0u64, 0u64, 0u64, 0u64);
    for value in generated(&target_and_screen(), 61, 2_000) {
        let target: String = serde_json::from_value(value["buffer"].clone()).expect("an id");
        let s = screen_of(&value);
        let held = s.opened.iter().any(|b| b.id == target);
        if held && s.opened.len() > 1 {
            several += 1;
        }
        if held && s.opened.len() == 1 {
            last += 1;
        }
        let shown = shown_buffers(&s.layout);
        if !shown.is_empty() && shown.iter().all(|b| *b == target) {
            everywhere += 1;
        }
        if !shown.contains(&target) {
            unshown += 1;
        }
        // The clause: the buffer leaves the opened set and every pane showing
        // it, and no region is left without a pane. A close that was refused —
        // the last opened buffer — changes nothing at all.
        let after = close_buffer(target.clone(), s.clone());
        if after != s {
            assert!(!after.opened.iter().any(|b| b.id == target));
            assert!(!shown_buffers(&after.layout).contains(&target));
            assert!(!panes(&after.layout).is_empty(), "no region without a pane");
        }
    }
    support::covered(
        "REQ-SCREEN.close_collapses_the_pane",
        &[
            ("closed one of several", several),
            ("closed the last opened buffer", last),
            ("closed a buffer filling every pane", everywhere),
            ("closed a buffer nothing showed", unshown),
        ],
    );
}

fn amount_and_screen() -> Schema {
    strukt(&[
        ("amount", Schema::Int { min: Some(-4), max: Some(4) }),
        ("screen", screen()),
    ])
}

/// @drt REQ-SCREEN.resize_moves_one_divider
/// @tests REQ-SCREEN.resize_moves_one_divider
/// @tests REQ-SCREEN.resize_has_a_floor
#[test]
#[ignore = "builds a Lean package and a Rust crate; run with --ignored"]
fn model_and_implementation_agree_on_resizing_a_divider() {
    check(
        "resize_moves_one_divider",
        "TraceLean.Screen.resizeFocus",
        "crates/core/src/surface/screen.rs::resize_focus",
        &["amount", "screen"],
        amount_and_screen(),
        67,
    );
}

/// @tests REQ-DRT-COVER.law_coverage
#[test]
fn generation_reaches_the_floor_and_the_far_side_of_a_split() {
    let (mut grew, mut shrank, mut floored, mut last, mut nothing) = (0u64, 0u64, 0u64, 0u64, 0u64);
    for value in generated(&amount_and_screen(), 67, 2_000) {
        let amount: i64 = serde_json::from_value(value["amount"].clone()).expect("an amount");
        let s = screen_of(&value);
        if amount > 0 {
            grew += 1;
        }
        if amount < 0 {
            shrank += 1;
        }
        match enclosing(&s.focus, &s.layout) {
            None => nothing += 1,
            Some((index, weights)) => {
                if index + 1 == weights.len() && weights.len() > 1 {
                    last += 1;
                }
                // The room the divider has, which is what the floor cuts an
                // over-long drag down to.
                let (here, next) = if index + 1 < weights.len() {
                    (weights[index], weights[index + 1])
                } else if index > 0 {
                    (weights[index], weights[index - 1])
                } else {
                    (0, 0)
                };
                let room = if amount >= 0 { next.saturating_sub(1) } else { here.saturating_sub(1) };
                if amount != 0 && room < amount.unsigned_abs() {
                    floored += 1;
                }
            }
        }
        // The clause, in two halves. Weight moves between the two sides of one
        // divider: the total is unchanged and at most two parts differ, so a
        // third pane cannot have drifted. And the floor holds: nothing that had
        // a weight is left with none.
        let after = resize_focus(amount, s.clone());
        let was = weights_of(&s.layout);
        let now = weights_of(&after.layout);
        assert_eq!(was.len(), now.len());
        assert_eq!(was.iter().sum::<u64>(), now.iter().sum::<u64>(), "weight is moved, not made");
        assert!(
            was.iter().zip(&now).filter(|(a, b)| a != b).count() <= 2,
            "a resize moves one divider"
        );
        for (before, after) in was.iter().zip(&now) {
            assert!(*before == 0 || *after >= 1, "a resize does not take a pane below one");
        }
    }
    support::covered(
        "REQ-SCREEN.resize_moves_one_divider",
        &[
            ("grew the focused pane", grew),
            ("shrank the focused pane", shrank),
            ("stopped at the floor", floored),
            ("resized the last part of a split", last),
            ("resized with nothing to move against", nothing),
        ],
    );
}

// ─────────────────────────────────────────────────────────── one way in

fn arrangement() -> Schema {
    let mut variants = BTreeMap::new();
    variants.insert("split".to_string(), Some(Box::new(strukt(&[("axis", axis())]))));
    variants.insert("close".to_string(), None);
    variants.insert("focus".to_string(), Some(Box::new(strukt(&[("dir", direction())]))));
    variants.insert("focusPane".to_string(), Some(Box::new(strukt(&[("pane", pane_id())]))));
    variants.insert(
        "resize".to_string(),
        Some(Box::new(strukt(&[("amount", Schema::Int { min: Some(-3), max: Some(3) })]))),
    );
    variants
        .insert("showBuffer".to_string(), Some(Box::new(strukt(&[("buffer", buffer_id())]))));
    Schema::Enum { variants }
}

fn arrangement_rect_screen() -> Schema {
    strukt(&[("how", arrangement()), ("rect", roomy_rect()), ("screen", screen())])
}

/// @drt REQ-SCREEN.one_arrangement_path
/// @tests REQ-SCREEN.one_arrangement_path
#[test]
#[ignore = "builds a Lean package and a Rust crate; run with --ignored"]
fn model_and_implementation_agree_on_every_change_to_the_arrangement() {
    check(
        "one_arrangement_path",
        "TraceLean.Screen.arrange",
        "crates/core/src/surface/screen.rs::arrange",
        &["how", "rect", "screen"],
        arrangement_rect_screen(),
        83,
    );
}

/// @tests REQ-DRT-COVER.law_coverage
#[test]
fn generation_reaches_every_kind_of_arrangement() {
    use tracelean_core::surface::screen::{arrange, Arrangement};
    let (mut split, mut closed, mut moved, mut resized, mut shown, mut inert) =
        (0u64, 0u64, 0u64, 0u64, 0u64, 0u64);
    for value in generated(&arrangement_rect_screen(), 83, 2_000) {
        let how: Arrangement = serde_json::from_value(value["how"].clone()).expect("a change");
        let rect: Rect = serde_json::from_value(value["rect"].clone()).expect("a rect");
        let s = screen_of(&value);
        match how {
            Arrangement::Split { .. } => split += 1,
            Arrangement::Close => closed += 1,
            Arrangement::Focus { .. } | Arrangement::FocusPane { .. } => moved += 1,
            Arrangement::Resize { .. } => resized += 1,
            Arrangement::ShowBuffer { .. } => shown += 1,
        }
        let after = arrange(how.clone(), rect, s.clone());
        if after == s {
            inert += 1;
        }
        // The clause: every change routes to the one function that makes it, so
        // arranging is exactly what calling that function does. A frontend
        // doing its own arithmetic would differ from this, and that is the
        // drift the clause exists to stop.
        let directly = match how {
            Arrangement::Split { axis } => split_focus(axis, s.clone()),
            Arrangement::Close => {
                match panes(&s.layout)
                    .into_iter()
                    .find(|(pane, _)| *pane == s.focus)
                    .map(|(_, buffer)| buffer)
                {
                    None => s.clone(),
                    Some(buffer) => close_buffer(buffer, s.clone()),
                }
            }
            Arrangement::Focus { dir } => {
                let mut stepped = s.clone();
                stepped.focus = focus_step(rect, s.clone(), dir);
                stepped
            }
            Arrangement::FocusPane { pane } => {
                if pane_ids(&s.layout).contains(&pane) {
                    let mut landed = s.clone();
                    landed.focus = pane;
                    landed
                } else {
                    s.clone()
                }
            }
            Arrangement::Resize { amount } => resize_focus(amount, s.clone()),
            Arrangement::ShowBuffer { buffer } => show_buffer(buffer, s.clone()),
        };
        assert_eq!(after, directly, "an arrangement is what the one function does");
    }
    support::covered(
        "REQ-SCREEN.one_arrangement_path",
        &[
            ("split the focused pane", split),
            ("closed what the focus showed", closed),
            ("moved the focus", moved),
            ("resized", resized),
            ("showed an opened buffer", shown),
            ("an arrangement that changed nothing", inert),
        ],
    );
}

// ───────────────────────────────────────────────────── moving the focus

/// A region with room in it, for the one binding a flat region tells nothing
/// about.
///
/// A move lands on a pane that overlaps the focused one across the direction
/// travelled, and a region of no height has no overlap to find — so a third of
/// the cases under the ordinary `rect` could never move anywhere, and the
/// interesting half of this law would go untested. The degenerate region is not
/// lost: `place` is bound over it, where what it does is the point.
fn roomy_rect() -> Schema {
    strukt(&[
        ("left", Schema::Nat { max: Some(2), edges: vec![0] }),
        ("top", Schema::Nat { max: Some(2), edges: vec![0] }),
        ("width", Schema::Nat { max: Some(9), edges: vec![1, 2, 4] }),
        ("height", Schema::Nat { max: Some(9), edges: vec![1, 2, 4] }),
    ])
}

fn rect_screen_direction() -> Schema {
    strukt(&[("rect", roomy_rect()), ("screen", screen()), ("dir", direction())])
}

/// @drt REQ-SCREEN.focus_follows_geometry
/// @tests REQ-SCREEN.focus_follows_geometry
#[test]
#[ignore = "builds a Lean package and a Rust crate; run with --ignored"]
fn model_and_implementation_agree_on_where_a_move_lands() {
    check(
        "focus_follows_geometry",
        "TraceLean.Screen.focusStep",
        "crates/core/src/surface/screen.rs::focus_step",
        &["rect", "screen", "dir"],
        rect_screen_direction(),
        71,
    );
}

/// @tests REQ-DRT-COVER.law_coverage
#[test]
fn generation_reaches_a_neighbour_an_edge_and_a_pane_nothing_places() {
    let (mut moved, mut stayed, mut many, mut adrift) = (0u64, 0u64, 0u64, 0u64);
    for value in generated(&rect_screen_direction(), 71, 2_000) {
        let rect: Rect = serde_json::from_value(value["rect"].clone()).expect("a rect");
        let dir: Direction = serde_json::from_value(value["dir"].clone()).expect("a direction");
        let s = screen_of(&value);
        let placed = pane_ids(&s.layout);
        if !placed.contains(&s.focus) {
            adrift += 1;
        }
        if placed.len() >= 3 {
            many += 1;
        }
        let landed = focus_step(rect, s.clone(), dir);
        if landed == s.focus {
            stayed += 1;
        } else {
            moved += 1;
            // The clause: a move lands on a pane, and on one beyond the
            // focused pane in the direction travelled — never on one behind it.
            //
            // Asserted only where the panes are distinct, and that is the
            // clause `panes_are_distinct` exists for rather than a convenience
            // here: with two panes carrying one identity, "the pane to the
            // right of a" names two rectangles and the question has no answer
            // to check.
            assert!(placed.contains(&landed));
            if distinct_panes(&s.layout) {
                let where_it_was = place(rect, s.layout.clone());
                let from = where_it_was.iter().find(|(id, _)| *id == s.focus).map(|(_, r)| *r);
                let to = where_it_was.iter().find(|(id, _)| *id == landed).map(|(_, r)| *r);
                if let (Some(from), Some(to)) = (from, to) {
                    let ahead = match dir {
                        Direction::Left => to.left + to.width <= from.left,
                        Direction::Right => from.left + from.width <= to.left,
                        Direction::Up => to.top + to.height <= from.top,
                        Direction::Down => from.top + from.height <= to.top,
                    };
                    assert!(ahead, "a move goes the way it was asked to");
                }
            }
        }
    }
    support::covered(
        "REQ-SCREEN.focus_follows_geometry",
        &[
            ("moved to a neighbour", moved),
            ("stayed at the edge", stayed),
            ("three or more panes placed", many),
            ("asked from a pane the layout does not place", adrift),
        ],
    );
}

// ──────────────────────────────────────────────── the strip and stations

/// @drt REQ-SCREEN.strip_is_the_opened_set
/// @tests REQ-SCREEN.strip_is_the_opened_set
#[test]
#[ignore = "builds a Lean package and a Rust crate; run with --ignored"]
fn model_and_implementation_agree_on_the_strip() {
    check(
        "strip_is_the_opened_set",
        "TraceLean.Screen.strip",
        "crates/core/src/surface/screen.rs::strip",
        &["screen"],
        one_screen(),
        73,
    );
}

/// @tests REQ-DRT-COVER.law_coverage
#[test]
fn generation_reaches_an_empty_strip_and_a_name_outside_ascii() {
    let (mut several, mut empty, mut wide) = (0u64, 0u64, 0u64);
    for value in generated(&one_screen(), 73, 2_000) {
        let s = screen_of(&value);
        if s.opened.len() >= 2 {
            several += 1;
        }
        if s.opened.is_empty() {
            empty += 1;
        }
        if s.opened.iter().any(|b| !b.id.is_ascii()) {
            wide += 1;
        }
        // The clause: the strip is the opened set — one row each, in the order
        // they were opened, each carrying the action that shows it. A row is
        // its number, then the buffer's title (and, for a file whose name
        // another shares, the folders that tell it apart).
        let bar = strip(s.clone());
        assert_eq!(bar.spans.len(), s.opened.len());
        assert_eq!(bar.text.split('\n').count().max(1), s.opened.len().max(1));
        for (row, held) in bar.text.split('\n').zip(&s.opened) {
            let title = tracelean_core::surface::screen::title_of(held);
            let named = row.split_once("  ").map_or("", |(_, rest)| rest);
            assert!(named == title || named.starts_with(&format!("{title} · ")), "the row names the buffer it shows: {row:?} {title:?}");
        }
        assert!(bar.spans.iter().all(|span| span.actions == vec!["screen.show".to_string()]));
    }
    support::covered(
        "REQ-SCREEN.strip_is_the_opened_set",
        &[
            ("several buffers opened", several),
            ("nothing opened at all", empty),
            ("a buffer whose identity is not plain ascii", wide),
        ],
    );
}

/// @drt REQ-SCREEN.stations_are_constant
/// @tests REQ-SCREEN.stations_are_constant
#[test]
#[ignore = "builds a Lean package and a Rust crate; run with --ignored"]
fn model_and_implementation_agree_on_the_stations() {
    check(
        "stations_are_constant",
        "TraceLean.Screen.stations",
        "crates/core/src/surface/screen.rs::stations",
        &["screen"],
        one_screen(),
        79,
    );
}

/// @tests REQ-DRT-COVER.law_coverage
#[test]
fn generation_asks_for_the_stations_from_every_kind_of_screen() {
    use tracelean_core::surface::screen::stations;
    let cases = generated(&one_screen(), 79, 2_000);
    let first = stations(screen_of(&cases[0]));
    let mut asked = 0u64;
    for value in &cases {
        // The clause itself, over the generator's whole output rather than a
        // pair somebody chose: no screen changes the answer.
        assert_eq!(stations(screen_of(value)), first);
        asked += 1;
    }
    support::covered("REQ-SCREEN.stations_are_constant", &[("asked of some screen", asked)]);
}
