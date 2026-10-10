//! The editor, driven: a tree, a keyboard, and a screen.
//!
//! Everything else in this project checks an arrow by calling the function at
//! its tail. `REQ-DRIVE` exists because that left one translation per frontend
//! untested — the code that turns what a keyboard sent into what the keymap
//! calls it — and the editor was unusable for a reason that lived exactly
//! there. `assets/keymap.json` bound the leader to `Space`, `name_of` handed
//! over `" "`, and `crates/core/tests/shipped_keymap.rs` pressed `"Space"`
//! against the data and found it bound, because a test that supplies the name
//! itself can never discover that nobody produces it.
//!
//! So this suite supplies nothing. It opens a pseudo-terminal (`terminal.rs`),
//! runs the real binary on [`demo/`](../../../demo), writes the bytes a
//! keyboard writes, and reads the screen. What it compares against is
//! `surface::drive`, which is modelled in `formal/TraceLean/Drive.lean` and
//! differentially tested — so the prediction is not this suite's opinion about
//! what should happen.

mod terminal;

use std::path::{Path, PathBuf};

use tracelean_core::surface::drive::drive;
use tracelean_core::surface::keymap::{self, Keymap};
use terminal::Terminal;

/// The keymap the binary ships, loaded the way the binary loads it. A suite
/// that built its own keymap would be checking a keymap nobody runs.
fn shipped() -> Keymap {
    let text = std::fs::read_to_string(project_root().join("assets/keymap.json"))
        .expect("the project ships a keymap");
    keymap::load(&text, &keymap::actions()).expect("the shipped keymap loads")
}

fn project_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap().parent().unwrap().to_path_buf()
}

/// The tree the documentation tells a person to open.
///
/// `demo_is_what_is_driven`: a suite that built a tree for itself would drive a
/// tree nobody has seen, and would keep passing after the thing a person is
/// told to do stopped working.
fn demo() -> PathBuf {
    let tree = project_root().join("demo");
    assert!(tree.join("src/celsius.rs").is_file(), "the demo tree is not where the docs say");
    tree
}

/// The frontend, opened on the demo tree at a fixed size.
///
/// On a copy of it, still named `demo`, without `.tracelean/`: what a person
/// left there by opening it — tabs, a sandbox they started — is theirs, and a
/// suite that read it would pass or fail by who last used the demo, and would
/// write over it.
fn open() -> Terminal {
    static COPIES: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let n = COPIES.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    let copy = std::env::temp_dir().join(format!("tracelean-driving-{}-{n}", std::process::id())).join("demo");
    let _ = std::fs::remove_dir_all(&copy);
    copy_tree(&demo(), &copy);
    Terminal::open(&copy, 30, 100)
}

fn copy_tree(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).expect("the copy's folder is made");
    for entry in std::fs::read_dir(from).expect("the demo reads").flatten() {
        let path = entry.path();
        if entry.file_name() == ".tracelean" {
            continue;
        }
        if path.is_dir() {
            copy_tree(&path, &to.join(entry.file_name()));
        } else {
            std::fs::copy(&path, to.join(entry.file_name())).expect("a demo file copies");
        }
    }
}

/// The menu row a mode offers, as the core produces it: one entry a key,
/// entries separated by two spaces, which is how a terminal joins a menu buffer
/// into the one row it has to spare.
///
/// Built from the shipped keymap rather than written out, so a binding added to
/// the keymap is a binding this suite expects to see on the screen.
fn bar(keymap: &Keymap, mode: &str) -> Option<String> {
    let found = keymap.modes.iter().find(|(name, _)| name == mode)?;
    if mode == keymap.root {
        // The root mode is the editor, not a list: it offers no menu.
        return None;
    }
    Some(
        found
            .1
            .bindings
            .iter()
            .map(|(key, binding)| format!("{key}  {}", binding.description()))
            .collect::<Vec<_>>()
            .join("  "),
    )
}

fn shown(screen: &[String]) -> String {
    screen.join("\n")
}

/// The strip of what is opened: the row its first entry starts. Rows name
/// buffers by title — `3  requirements` — so a title is looked for here and not
/// anywhere on screen, where the stations bar says the same words.
fn strip_of(screen: &[String]) -> String {
    screen
        .iter()
        .find(|line| line.trim_start().starts_with("1  "))
        .cloned()
        .unwrap_or_default()
}

/// How many dividers the most divided row has, which is one fewer than the
/// panes laid side by side.
fn dividers(screen: &[String]) -> usize {
    screen.iter().map(|line| line.matches('│').count()).max().unwrap_or(0)
}

/// What the rightmost pane shows: each divided row, after its last divider.
fn side(screen: &[String]) -> String {
    screen
        .iter()
        .filter_map(|line| line.rfind('│').map(|at| &line[at + '│'.len_utf8()..]))
        .collect::<Vec<_>>()
        .join("\n")
}

/// Space opens the leader menu.
///
/// The one test that would have caught the bug, written the way it had to be
/// written to catch it: the byte a space bar sends, into a real terminal, with
/// nothing in between naming it.
///
/// @tests REQ-DRIVE.keys_arrive_as_bytes
/// @tests REQ-DRIVE.screen_answers_for_the_frontend
/// @tests REQ-MYTH.actions_reachable
/// @structural REQ-DRIVE.keys_arrive_as_bytes reason="a claim about how the frontend is reached rather than about any value — a model of a key press cannot say whether the press came from a keyboard or from the test that expected it"
/// @structural REQ-DRIVE.screen_answers_for_the_frontend reason="a claim about which of two accounts of a key press is believed, which is a property of the harness rather than of any value a function computes"
#[test]
fn pressing_the_space_bar_opens_the_leader_the_keymap_declares() {
    let keymap = shipped();
    let mut terminal = open();

    let opening = terminal.screen();
    let expected = bar(&keymap, "Leader").expect("the keymap declares a leader mode");
    assert!(
        !shown(&opening).contains(&expected),
        "the leader menu is on the screen before anybody pressed anything"
    );

    let screen = terminal.press(" ");
    assert!(
        shown(&screen).contains(&expected),
        "the space bar did not open the leader menu.\nexpected to find:\n  {expected}\ngot:\n{}",
        shown(&screen)
    );
}

/// A session walks the modes the model predicts.
///
/// The keys are pressed as bytes; the modes are predicted by `drive`, which is
/// the function `formal/TraceLean/Drive.lean` models. Each screen is then held
/// to the menu that mode offers — so a frontend that reached the right mode and
/// drew the previous mode's menu fails here, and so does one that drew the
/// right menu from the wrong mode.
///
/// @tests REQ-DRIVE.keys_arrive_as_bytes
/// @tests REQ-DRIVE.screen_answers_for_the_frontend
/// @tests REQ-DRIVE.walk_follows_the_machine
/// @tests REQ-DRIVE.menu_is_the_bar_there
#[test]
fn a_session_of_keys_walks_the_modes_the_model_predicts() {
    let keymap = shipped();

    // Bytes a keyboard sends, and the names the keymap gives them. The two
    // lists are written out side by side on purpose: every place they differ is
    // a place a frontend has to translate, and translation is what this suite
    // is about.
    let pressed: &[(&str, &str)] = &[
        (" ", "Space"),    // into the leader
        ("t", "t"),        // into trace
        ("\u{1b}", "Escape"), // back to the leader
        ("\u{1b}", "Escape"), // back to the root
        (" ", "Space"),    // in again, from the root this time
        ("d", "d"),        // differential testing
        ("\u{1b}", "Escape"),
        ("\u{1b}", "Escape"),
    ];

    let names: Vec<String> = pressed.iter().map(|(_, name)| name.to_string()).collect();
    let predicted = drive(&keymap, &keymap.root, &names);
    assert_eq!(predicted.len(), pressed.len());

    let mut terminal = open();
    for (index, ((bytes, name), step)) in pressed.iter().zip(&predicted).enumerate() {
        let screen = terminal.press(bytes);
        let seen = shown(&screen);

        match bar(&keymap, &step.mode) {
            Some(expected) => assert!(
                seen.contains(&expected),
                "press {index} ({name:?}) should have reached {:?} and shown its menu.\n\
                 expected to find:\n  {expected}\ngot:\n{seen}",
                step.mode
            ),
            None => {
                // The root offers no menu, so no other mode's menu may be on
                // the screen either — which is how "it went back" is read off a
                // screen rather than asked of the frontend.
                for (other, _) in &keymap.modes {
                    let Some(menu) = bar(&keymap, other) else { continue };
                    assert!(
                        !seen.contains(&menu),
                        "press {index} ({name:?}) should have reached the root, and the menu \
                         for {other:?} is still on the screen:\n{seen}"
                    );
                }
            }
        }
    }
}

/// Every mode the shipped keymap declares is reachable by pressing keys.
///
/// `REQ-MYTH.actions_reachable` is a claim about the keymap, and the keymap
/// suite proves it of the data. This proves it of the editor: for each mode,
/// the keys that reach it are pressed as bytes, and the mode's own menu has to
/// appear. Insert mode is the exception the keymap itself makes — it offers no
/// menu because in it a key is text — so it is reached and left rather than
/// read.
///
/// @tests REQ-DRIVE.keys_arrive_as_bytes
/// @tests REQ-MYTH.actions_reachable
#[test]
fn every_menu_the_keymap_declares_can_be_opened_from_the_keyboard() {
    let keymap = shipped();
    let mut checked = 0;

    for (mode, _) in &keymap.modes {
        let Some(expected) = bar(&keymap, mode) else { continue };
        let Some(route) = route_to(&keymap, mode) else {
            panic!("no key sequence reaches {mode:?}, which `actions_reachable` forbids")
        };
        if mode == "Insert" {
            continue;
        }

        let mut terminal = open();
        let mut screen = Vec::new();
        for key in &route {
            screen = terminal.press(&bytes_for(key));
        }
        assert!(
            shown(&screen).contains(&expected),
            "{route:?} should have opened {mode:?}.\nexpected to find:\n  {expected}\ngot:\n{}",
            shown(&screen)
        );
        checked += 1;
    }

    assert!(checked > 3, "only {checked} menus were opened; the walk is wrong");
}

/// The bytes a keyboard sends for a key the keymap names.
///
/// The inverse of `keymap::typed`, and the reason this suite is worth having:
/// everywhere these two disagree is a place the frontend has to get right.
fn bytes_for(key: &str) -> String {
    match key {
        "Space" => " ".to_string(),
        "Escape" => "\u{1b}".to_string(),
        "Enter" => "\r".to_string(),
        "Tab" => "\t".to_string(),
        "Backspace" => "\u{7f}".to_string(),
        // Ctrl and a letter: the letter's place in the alphabet, as a
        // terminal sends it.
        held if held.len() == 3 && held.starts_with("C-") => {
            let letter = held.as_bytes()[2].to_ascii_lowercase();
            ((letter - b'a' + 1) as char).to_string()
        }
        other => other.to_string(),
    }
}

/// Every journey a person's task is written as, replayed here as keys into a
/// real terminal; the page replays the same file (`web/test/journeys.mjs`).
/// A journey that passes in one frontend and fails in the other is the
/// finding.
///
/// @tests REQ-LOOK.journeys_replay
/// @structural REQ-LOOK.journeys_replay reason="a claim that two running frontends answer the same keys alike, read off their screens; no value either side computes"
#[test]
fn every_journey_replays_in_the_terminal() {
    let written: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(project_root().join("tests/journeys.json")).unwrap()).unwrap();
    let journeys = written["journeys"].as_array().expect("a list of journeys");
    assert!(journeys.len() >= 3, "too few journeys to say much");
    for journey in journeys {
        let name = journey["name"].as_str().unwrap_or("?");
        let mut terminal = open();
        let mut screen = terminal.screen();
        for (at, step) in journey["steps"].as_array().unwrap().iter().enumerate() {
            if let Some(keys) = step["keys"].as_array() {
                for key in keys {
                    screen = terminal.press(&bytes_for(key.as_str().unwrap()));
                }
            } else if let Some(text) = step["type"].as_str() {
                screen = terminal.press(text);
            } else if let Some(text) = step["see"].as_str() {
                assert!(shown(&screen).contains(text), "`{name}`, step {at}: `{text}` is not on the screen:\n{}", shown(&screen));
            } else if let Some(text) = step["not"].as_str() {
                assert!(!shown(&screen).contains(text), "`{name}`, step {at}: `{text}` is on the screen:\n{}", shown(&screen));
            } else {
                panic!("`{name}`, step {at}: a step this suite cannot take: {step}");
            }
        }
    }
}

/// A key sequence from the root to a mode, found by walking the keymap.
///
/// Read out of the data rather than written down, so a keymap that moves a menu
/// under a different leader key is still driven.
fn route_to(keymap: &Keymap, target: &str) -> Option<Vec<String>> {
    let mut frontier = vec![(keymap.root.clone(), Vec::new())];
    let mut seen = vec![keymap.root.clone()];
    while let Some((mode, route)) = frontier.pop() {
        if mode == target {
            return Some(route);
        }
        let Some((_, found)) = keymap.modes.iter().find(|(name, _)| *name == mode) else {
            continue;
        };
        for (key, binding) in &found.bindings {
            let keymap::Binding::Enter { mode: next, .. } = binding else { continue };
            if seen.contains(next) {
                continue;
            }
            seen.push(next.clone());
            let mut onwards = route.clone();
            onwards.push(key.clone());
            frontier.push((next.clone(), onwards));
        }
    }
    None
}

/// The tree on the screen is the tree that was opened.
///
/// The cheapest test here and the one that fails first when the demo moves: the
/// listing a person is told they will see is the listing the frontend draws.
///
/// @tests REQ-DRIVE.demo_is_what_is_driven
/// @tests REQ-DRIVE.screen_answers_for_the_frontend
/// @structural REQ-DRIVE.demo_is_what_is_driven reason="a claim about which tree this repository ships and drives, which is a property of the tree rather than of any value"
#[test]
fn the_demo_tree_is_what_the_frontend_lists() {
    let terminal = open();
    let seen = shown(&terminal.screen());
    for entry in ["README.md", "celsius.rs", "journal.md", "round_trip.rs"] {
        assert!(seen.contains(entry), "the listing does not show {entry}:\n{seen}");
    }
    // A filename that is not ASCII, because a frontend counting bytes draws it
    // wrongly and says nothing.
    assert!(seen.contains("café.md"), "the listing mangles a non-ASCII name:\n{seen}");
}

/// A key nothing binds is refused out loud.
///
/// `REQ-MYTH.totality` says no key is silently swallowed. At the root that
/// means the editor says so, and the only place that claim can be checked is
/// the screen — which is where the space bar's failure was visible for as long
/// as it existed, with no test looking at it.
///
/// @tests REQ-DRIVE.screen_answers_for_the_frontend
/// @tests REQ-MYTH.totality
#[test]
fn a_key_nothing_binds_is_refused_where_a_person_can_see_it() {
    let mut terminal = open();
    let screen = terminal.press("z");
    let seen = shown(&screen);
    assert!(
        seen.contains('z') && seen.to_lowercase().contains("not bound"),
        "an unbound key vanished instead of being refused:\n{seen}"
    );
}

/// The frontend leaves when it is told to.
///
/// @tests REQ-DRIVE.keys_arrive_as_bytes
#[test]
fn q_closes_the_terminal_frontend() {
    let mut terminal = open();
    let _ = terminal.press("q");
    assert!(terminal.finished(), "the frontend is still running after `q`");
}

/// The walk the documentation tells a person to make actually opens a file.
///
/// `docs/09-using-the-tui.md` says: arrows to the file, then `Space f o`. That
/// is a sentence in a document until something presses those keys on that tree
/// and reads what came up — which is what `demo_is_what_is_driven` is for. The
/// first version of that page said *press Enter*, and nothing contradicted it,
/// because no test had ever opened a file the way a person does.
///
/// @tests REQ-DRIVE.demo_is_what_is_driven
/// @tests REQ-DRIVE.keys_arrive_as_bytes
/// @tests REQ-DRIVE.screen_answers_for_the_frontend
#[test]
fn the_documented_way_to_open_a_file_opens_it() {
    let mut terminal = open();

    // Down to `celsius.rs`, inside the open `src` folder. Read off the
    // opening screen rather than counted out here, so the test still means the
    // same thing when the demo tree gains a file.
    //
    // A screen row is not a buffer line: the bars sit above the panes, so the
    // distance to travel is measured from where the buffer starts on the
    // screen rather than from the top of it. That is read off the screen too —
    // the first row under the strip — so chrome added above costs this test
    // nothing.
    let listing = terminal.screen();
    let strip = listing
        .iter()
        .position(|line| line.trim_start().starts_with("1  "))
        .expect("the strip lists what is opened");
    let row = listing
        .iter()
        .position(|line| line.contains("celsius.rs"))
        .expect("the demo tree lists the file the documentation names");
    for _ in 0..row.saturating_sub(strip + 1) {
        terminal.press("\u{1b}[B"); // the byte sequence a Down arrow sends
    }

    terminal.press(" ");
    terminal.press("f");
    let screen = terminal.press("o");
    let seen = shown(&screen);

    assert!(
        seen.contains("to_fahrenheit"),
        "`Space f o` did not open the file under the cursor:\n{seen}"
    );
    // It opens in the document pane, beside the listing rather than instead of
    // it: the explorer is where it was, and the file is right of its divider.
    assert!(
        seen.contains("journal.md"),
        "opening a file took the explorer off the screen:\n{seen}"
    );
    assert!(
        screen.iter().any(|line| matches!(
            (line.find('│'), line.find("to_fahrenheit")),
            (Some(divider), Some(file)) if divider < file
        )),
        "the file did not open in the document pane, beside the explorer:\n{seen}"
    );
}

/// Splitting the screen puts two panes on it, and a key reaches the one with
/// the focus.
///
/// Read off the screen rather than asked of the editor. Every part of the
/// arrangement is modelled and differentially tested, and all of that would
/// still hold of a frontend that laid the panes out and drew one of them: the
/// join between a screen value and painted characters is exactly the join
/// nothing else in this project crosses.
///
/// @tests REQ-SCREEN.layout_tiles_the_region
/// @tests REQ-SCREEN.one_arrangement_path
/// @tests REQ-DRIVE.screen_answers_for_the_frontend
#[test]
fn splitting_the_screen_draws_two_panes() {
    let mut terminal = open();
    let before = terminal.screen();
    // The workbench opens on three panes: the explorer, the document, the side.
    assert_eq!(dividers(&before), 2, "the screen does not open on three panes:\n{}", shown(&before));

    // `Space w v` — the leader, the screen menu, split across.
    terminal.press(" ");
    terminal.press("w");
    let after = terminal.press("v");
    let seen = shown(&after);

    assert_eq!(
        dividers(&after),
        3,
        "splitting across drew no new divider, so the explorer was not split in two:\n{seen}"
    );
    // Both halves show the listing that was there, which is what
    // `split_keeps_the_buffer` says and what a person would notice first.
    //
    // The name that ends nearest the left edge, indentation and all, because
    // each half is half as wide as the explorer was and a row reaching further
    // is cut off in both.
    let listed = before
        .iter()
        .filter_map(|line| line.split('│').next().map(str::trim_end))
        .filter(|row| row.contains('.') && !row.contains(':'))
        .min_by_key(|row| row.chars().count())
        .and_then(|row| row.split_whitespace().last())
        .expect("the demo listing names a file")
        .to_string();
    let halves: usize =
        after.iter().map(|line| line.matches(&listed).count()).sum();
    assert!(
        halves >= 2,
        "`{listed}` appears {halves} time(s), so the split did not leave the buffer in both halves:\n{seen}"
    );
}

/// The stations are on the screen from the first frame, and one of them opens
/// from a bare keyboard.
///
/// `stations_are_constant` says a station is reachable from every state, and
/// the state that matters is the first one: the station that opens a project is
/// needed exactly when no project is open. A clause about a function cannot see
/// whether anything put that function's answer on the screen.
///
/// @tests REQ-SCREEN.stations_are_constant
/// @tests REQ-DRIVE.screen_answers_for_the_frontend
#[test]
fn a_station_is_on_the_screen_and_opens_from_the_keyboard() {
    let mut terminal = open();
    let opening = shown(&terminal.screen());
    for station in ["project", "trace", "sandbox", "design", "history"] {
        assert!(
            opening.contains(station),
            "`{station}` is not on the opening screen, so it is not reachable:\n{opening}"
        );
    }

    // `Space w t r` — the leader, the screen menu, the stations, requirements,
    // which are the design's.
    //
    // What this checks is that the station opened *its own* buffer — the
    // demo's requirements — rather than handing back the one that was already
    // there: every directory used to produce the whole tree, whatever was
    // asked for.
    terminal.press(" ");
    terminal.press("w");
    terminal.press("t");
    let screen = terminal.press("r");
    let seen = shown(&screen);
    assert!(
        strip_of(&screen).contains("  design"),
        "the requirements key did not open the design:\n{seen}"
    );
    // A station's buffer goes to the side panel, so that is where the whole
    // tree must not be.
    assert!(
        !side(&screen).contains("celsius.rs"),
        "the requirements station answered with the whole tree:\n{seen}"
    );
    assert!(
        side(&screen).contains("REQ-THERMO"),
        "the demo's requirements are not in the index:\n{seen}"
    );
}

/// The sandbox station opens, and what it shows says it is an estimate.
///
/// The demo tree holds no transcript and no price table, which is the ordinary
/// case rather than a degraded one: a tool that writes no record is fully
/// supported, and an estimate with nothing behind it is still an estimate and
/// still says so. A figure rendered as a bill is the thing `ARCH-HONEST`
/// forbids, and only a screen can show that it was not.
///
/// @tests REQ-SHOW.sandbox_from_observation
/// @tests REQ-COST.estimate_is_labelled
/// @tests REQ-TRANSCRIPT.absent_is_fine
#[test]
fn the_sandbox_station_opens_and_calls_its_figure_an_estimate() {
    let mut terminal = open();
    terminal.press(" ");
    terminal.press("w");
    terminal.press("t");
    let screen = terminal.press("s");
    let seen = shown(&screen);
    assert!(
        strip_of(&screen).contains("  sandbox"),
        "the sandbox station did not open its own buffer:\n{seen}"
    );
    assert!(
        seen.contains("estimated "),
        "the sandbox station showed no estimate:\n{seen}"
    );
    assert!(
        !seen.contains("billed") && !seen.contains("charged"),
        "the sandbox station rendered a reading of a log as an amount billed:\n{seen}"
    );
}

/// What is opened appears in the strip, and the strip is what the session
/// holds rather than a list kept beside it.
///
/// @tests REQ-SCREEN.strip_is_the_opened_set
/// @tests REQ-SCREEN.opened_outlives_shown
#[test]
fn the_strip_lists_what_was_opened_and_keeps_it() {
    let mut terminal = open();
    let before = terminal.screen();
    let rows = |screen: &[String]| {
        screen.iter().filter(|line| line.trim_start().starts_with("1  ")).count()
    };
    assert_eq!(rows(&before), 1, "there is no strip:\n{}", shown(&before));

    // Open a station, and the buffer it produced joins the opened set.
    terminal.press(" ");
    terminal.press("w");
    terminal.press("t");
    let after = terminal.press("d");
    let seen = shown(&after);
    assert!(
        strip_of(&after).contains("  design"),
        "the strip does not list the buffer that was just opened:\n{seen}"
    );

    // Opening the same station again holds one of it, not two — which is the
    // difference between a buffer and a panel rebuilt whenever it is shown.
    terminal.press(" ");
    terminal.press("w");
    terminal.press("t");
    let again = terminal.press("d");
    assert_eq!(
        strip_of(&again).matches("  design").count(),
        1,
        "opening twice held it twice:\n{}",
        shown(&again)
    );
}

/// Closing a pane gives its region back, and the screen stops being divided.
///
/// @tests REQ-SCREEN.close_collapses_the_pane
#[test]
fn closing_a_pane_gives_its_region_back() {
    let mut terminal = open();
    let opening = dividers(&terminal.screen());
    terminal.press(" ");
    terminal.press("w");
    let split = terminal.press("v");
    assert!(
        dividers(&split) > opening,
        "nothing was split, so there is nothing to close:\n{}",
        shown(&split)
    );

    terminal.press(" ");
    terminal.press("w");
    let closed = terminal.press("q");
    assert!(
        dividers(&closed) < dividers(&split),
        "closing left the screen as divided as it was:\n{}",
        shown(&closed)
    );
}

/// Each place is on the screen where it belongs, in rows of its own: the
/// stations, the strip, the panes at the rectangles the core's layout gives
/// them, the status line and the bar of what can be done.
///
/// The rectangles are the core's (`screen::place` over the workbench layout),
/// not this suite's idea of them; the dividers read back from the cells must
/// fall exactly on their edges.
///
/// @tests REQ-LOOK.regions_present
/// @structural REQ-LOOK.regions_present reason="a claim about where a running frontend drew each place, read off the cells it painted; no value either side computes"
#[test]
fn the_screen_shows_each_place_where_the_layout_puts_it() {
    use tracelean_core::surface::produce::menu_buffer;
    use tracelean_core::surface::screen::{place, workbench, Rect};

    let (rows, columns) = (30u64, 100u64);
    let terminal = open();
    let cells = tracelean_core::surface::cells::grid(terminal.painted(), rows, columns);
    let text = |row: usize| cells[row].iter().map(|c| c.text.as_str()).collect::<String>();
    let layout = workbench(menu_buffer("a".into(), vec![]), menu_buffer("b".into(), vec![]), menu_buffer("c".into(), vec![]))
        .layout;
    // The two bars above, and the status line, a blank row and the bar below.
    let region = Rect { left: 0, top: 0, width: columns, height: rows - 6 };
    let above = 2usize;
    let placed = place(region, layout);
    assert_eq!(placed.len(), 3, "the workbench is not three panes: {placed:?}");

    // Every station on the first row, inside the terminal's width; the strip
    // under them.
    for station in ["project", "trace", "sandbox", "design", "history"] {
        assert!(text(0).contains(station), "`{station}` is not on the stations row: {}", text(0));
    }
    assert!(text(1).trim_start().starts_with("1  "), "no strip row: {}", text(1));

    // Each pane's right edge, where it has a neighbour, is a divider down its
    // whole height and nowhere else.
    let mut edges = Vec::new();
    for (pane, at) in &placed {
        if at.left + at.width < region.width {
            edges.push((pane.clone(), (at.left + at.width - 1) as usize, at.top as usize + above, (at.top + at.height) as usize + above));
        }
    }
    for row in above..above + region.height as usize {
        let drawn: Vec<usize> = (0..columns as usize).filter(|x| cells[row][*x].text == "│").collect();
        let expected: Vec<usize> =
            edges.iter().filter(|(_, _, from, to)| (*from..*to).contains(&row)).map(|(_, x, _, _)| *x).collect();
        assert_eq!(drawn, expected, "row {row}'s dividers are not the layout's edges: {}", text(row));
    }
    for row in (above + region.height as usize)..rows as usize {
        assert!(!text(row).contains('│'), "a divider below the panes, row {row}: {}", text(row));
    }
    // The explorer shows the listing, inside its own rectangle.
    let (_, explorer) = placed.iter().find(|(pane, _)| pane == "explorer").expect("an explorer");
    let inside = (above..above + explorer.height as usize)
        .map(|row| cells[row][..explorer.width as usize].iter().map(|c| c.text.as_str()).collect::<String>())
        .collect::<Vec<_>>()
        .join("\n");
    assert!(inside.contains("celsius.rs"), "the listing is not in the explorer's rectangle:\n{inside}");

    // Below, after a blank row: the status line in reverse video, then the bar
    // of what can be done — empty here, where the cursor starts on nothing.
    let status = above + region.height as usize + 1;
    assert!(text(status - 1).trim().is_empty(), "no blank row above the status: {}", text(status - 1));
    assert!(cells[status].iter().all(|c| c.reverse || c.text == " "), "the status line is not in reverse: {}", text(status));
    assert!(cells[status].iter().any(|c| c.reverse), "no status line on row {status}: {}", text(status));
    assert!(!cells[status + 1].iter().any(|c| c.reverse), "the status runs into the bar: {}", text(status + 1));
}

/// Which pane a key goes to can be read off the screen: every divider the
/// focused pane shares with a neighbour is drawn in a style no other divider
/// has, and that holds for the last pane across, which has no divider of its
/// own.
///
/// Read from the cells the terminal received (`surface::cells::grid`), styles
/// and all — the text alone is the same whichever pane is focused.
///
/// @tests REQ-LOOK.focus_visible
/// @structural REQ-LOOK.focus_visible reason="a claim about how a running frontend's screen looks, read off the cells it painted; no value either side computes"
#[test]
fn the_focused_pane_is_marked_on_the_screen() {
    let mut terminal = open();
    // The workbench's three panes, focused left to right: the explorer, the
    // document, the side panel.
    for focused in 0..3 {
        let cells = tracelean_core::surface::cells::grid(terminal.painted(), 30, 100);
        // Each divider column, and the looks its cells are drawn in.
        let mut columns: std::collections::BTreeMap<usize, std::collections::BTreeSet<(Option<String>, bool)>> =
            Default::default();
        for row in &cells {
            for (x, cell) in row.iter().enumerate() {
                if cell.text == "│" {
                    columns.entry(x).or_default().insert((cell.fg.clone(), cell.bold));
                }
            }
        }
        let edges: Vec<usize> = columns.keys().copied().collect();
        assert_eq!(edges.len(), 2, "not three panes side by side: {edges:?}");
        // The focused pane's edges: the divider on its left and the one on its right.
        let mine: Vec<usize> = edges
            .iter()
            .enumerate()
            .filter(|(at, _)| *at + 1 == focused || *at == focused)
            .map(|(_, x)| *x)
            .collect();
        let marks: std::collections::BTreeSet<_> = mine.iter().flat_map(|x| columns[x].iter().cloned()).collect();
        let others: std::collections::BTreeSet<_> =
            edges.iter().filter(|x| !mine.contains(x)).flat_map(|x| columns[x].iter().cloned()).collect();
        assert_eq!(marks.len(), 1, "pane {focused}'s edges are drawn unevenly: {marks:?}");
        assert!(
            marks.is_disjoint(&others),
            "pane {focused} is focused and its edges look like the others: {marks:?} against {others:?}"
        );
        terminal.press(" ");
        terminal.press("w");
        terminal.press("l");
    }
}

/// `.` lists what can be done where the cursor is — the terminal's
/// right-click — on the screen, and a key picks from it.
///
/// @tests REQ-ACT.everything_is_offered
/// @tests REQ-DRIVE.screen_answers_for_the_frontend
#[test]
fn dot_lists_what_can_be_done_here() {
    let mut terminal = open();
    let screen = terminal.press(".");
    let seen = shown(&screen);
    assert!(seen.contains("New file") && seen.contains("Split right"), "`.` listed nothing:\n{seen}");
    let screen = terminal.press("\u{1b}");
    assert!(!shown(&screen).contains("Split right"), "Escape left the list up:\n{}", shown(&screen));
}

/// Ctrl+P asks for a file on the last row, shows what would open while the
/// name is typed, and Enter opens the best match; Ctrl+F asks for text the
/// same way.
///
/// @tests REQ-DRIVE.screen_answers_for_the_frontend
#[test]
fn ctrl_p_opens_a_file_by_a_few_letters_and_selects() {
    let mut terminal = open();
    terminal.press("\u{10}");
    let screen = terminal.press("cels");
    let seen = shown(&screen);
    assert!(seen.contains("open: cels") && seen.contains("src/celsius.rs"), "no file was offered:\n{seen}");
    let seen = shown(&terminal.press("\r"));
    assert!(seen.contains("to_fahrenheit"), "Enter did not open the file:\n{seen}");
    terminal.press("\u{6}");
    let seen = shown(&terminal.press("fahr"));
    assert!(seen.contains("find in this buffer (Tab switches): fahr"), "Ctrl+F asked nothing:\n{seen}");
    let seen = shown(&terminal.press("\u{1b}"));
    assert!(!seen.contains("find in"), "Escape left the question up:\n{seen}");
    // Shift+Right selects, drawn in reverse video (as the status line is
    // too); an arrow lets it go.
    let reversed = |terminal: &Terminal| terminal.painted().matches("\u{1b}[7m").count();
    let before = reversed(&terminal);
    terminal.press("\u{1b}[1;2C");
    terminal.press("\u{1b}[1;2C");
    assert!(reversed(&terminal) > before, "Shift+Right drew no selection");
    terminal.press("\u{1b}[C");
    assert_eq!(reversed(&terminal), before, "an arrow left the selection drawn");
    // Ctrl+R replaces: what, Enter, with what, Enter.
    terminal.press("\u{12}");
    terminal.press("fahrenheit");
    let seen = shown(&terminal.press("\r"));
    assert!(seen.contains("replace every `fahrenheit` with:"), "Ctrl+R asked nothing:\n{seen}");
    terminal.press("kelvin");
    let seen = shown(&terminal.press("\r"));
    assert!(seen.contains("to_kelvin") && !seen.contains("to_fahrenheit"), "nothing was replaced:\n{seen}");
}
