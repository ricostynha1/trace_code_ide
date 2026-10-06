//! The keymap the project ships, checked as data rather than as a fixture.
//!
//! `REQ-MYTH` is about a keymap a user edits. Laws that only ever run against
//! fixtures written next to them are laws about the fixtures, so the file in
//! `assets/keymap.json` is loaded here and put through the same validation a
//! user's own edit would go through.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use tracelean_core::surface::keymap::{self, Binding, Keymap, LoadError, Outcome, LEAVE};

fn project_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap().parent().unwrap().to_path_buf()
}

fn shipped() -> (String, Keymap) {
    let text = std::fs::read_to_string(project_root().join("assets/keymap.json"))
        .expect("the project ships a keymap");
    let keymap = keymap::load(&text, &keymap::actions()).unwrap_or_else(|error| match error {
        LoadError::Unreadable { message } => {
            panic!("the shipped keymap is not readable: {message}")
        }
        LoadError::Invalid { problems } => {
            panic!("the shipped keymap does not hold together: {problems:#?}")
        }
    });
    (text, keymap)
}

/// The shipped keymap loads, which means it satisfies every law at once: every
/// mode it enters exists, every action it names is in the registry, every
/// action in the registry is reachable, and leaving terminates.
///
/// @tests REQ-MYTH.keymap_is_data
/// @tests REQ-MYTH.modes_defined
/// @tests REQ-MYTH.actions_defined
/// @tests REQ-MYTH.actions_reachable
/// @tests REQ-MYTH.escape_terminates
#[test]
fn the_keymap_the_project_ships_holds_together() {
    let (_, keymap) = shipped();
    assert_eq!(keymap.root, "Normal");
    assert!(keymap.modes.len() > 5, "the shipped keymap is barely a keymap");

    // Every action the registry declares is dispatched by some binding, and
    // nothing else is dispatched. `validate` already refuses both, so this says
    // what the file is rather than re-checking it.
    let mut dispatched = BTreeSet::new();
    for (_, mode) in &keymap.modes {
        for (_, binding) in &mode.bindings {
            if let Binding::Dispatch { action, .. } = binding {
                dispatched.insert(action.clone());
            }
        }
    }
    assert_eq!(dispatched, keymap::actions());

    // Every binding says what it does, because the which-key bar has nothing
    // else to show.
    for (name, mode) in &keymap.modes {
        for (key, binding) in &mode.bindings {
            assert!(
                !binding.description().trim().is_empty(),
                "{name}/{key} has no description, so the which-key bar cannot describe it"
            );
        }
    }
}

/// Pressing keys against the shipped keymap: every key in every mode has an
/// outcome, and escape walks back to the root.
///
/// @tests REQ-MYTH.totality
/// @tests REQ-MYTH.escape_pops_one
/// @tests REQ-MYTH.escape_terminates
#[test]
fn every_key_in_the_shipped_keymap_does_something() {
    let (_, keymap) = shipped();

    let keys: Vec<String> = ('a'..='z')
        .chain('A'..='Z')
        .chain('0'..='9')
        .map(|c| c.to_string())
        .chain(["Space", LEAVE, "F1", "Tab", "å", "→"].map(String::from))
        .collect();

    for (name, _) in &keymap.modes {
        for key in &keys {
            let outcome = keymap::step(keymap.clone(), name.clone(), key.clone());
            match &outcome {
                Outcome::Enter { mode } => assert!(keymap.has_mode(mode), "{name}/{key} -> {mode}"),
                Outcome::Leave { mode } => assert!(keymap.has_mode(mode), "{name}/{key} -> {mode}"),
                Outcome::Dispatch { action } => {
                    assert!(keymap::actions().contains(action), "{name}/{key} -> {action}")
                }
                // Only a mode with no parent passes a key through. A mode that
                // has one was reached from a menu, and an unbound key there
                // goes back rather than disappearing, so a mistyped leader
                // never strands anyone. The parentless modes are the ones where
                // a key nothing binds still means something to the editor: the
                // root, where it is refused out loud, and `Insert`, where it is
                // text.
                Outcome::PassThrough => assert!(
                    keymap.mode(name).and_then(|m| m.parent.clone()).is_none(),
                    "{name}/{key} vanished"
                ),
            }
        }

        // Escape reaches the root, one level at a time.
        let mut mode = name.clone();
        let mut steps = 0;
        while mode != keymap.root {
            let parent = keymap.mode(&mode).and_then(|m| m.parent.clone());
            let towards = parent.clone().unwrap_or_else(|| keymap.root.clone());
            // Either the mode has a parent and escape leaves to it, or it has
            // none and binds escape itself. A parentless mode must bind it: the
            // alternative is escape being typed as text and no way out.
            let stepped = keymap::step(keymap.clone(), mode.clone(), LEAVE.to_string());
            assert!(
                stepped == Outcome::Leave { mode: towards.clone() }
                    || stepped == Outcome::Enter { mode: towards.clone() },
                "escape from {mode} did not move exactly one level: {stepped:?}"
            );
            mode = keymap::next_mode(keymap.clone(), mode.clone(), LEAVE.to_string());
            steps += 1;
            assert!(steps < 20, "escape from {name} did not reach the root");
        }
    }
}

/// The which-key bar for the shipped keymap is computed from it.
///
/// @tests REQ-MYTH.whichkey_is_a_query
#[test]
fn the_which_key_bar_is_the_shipped_keymap_read_back() {
    let (_, keymap) = shipped();
    for (name, mode) in &keymap.modes {
        let shown = keymap::which_key(&keymap, name);
        // Every binding, plus the way out of a mode that has one.
        let expected = mode.bindings.len() + usize::from(mode.parent.is_some());
        assert_eq!(shown.len(), expected, "{name}");
        if mode.parent.is_some() {
            assert!(shown.iter().any(|(key, _)| key == LEAVE), "{name} shows no way out");
        }
        for (key, binding) in &mode.bindings {
            assert!(
                shown.contains(&(key.clone(), binding.description().to_string())),
                "{name}/{key} is bound and the bar does not show it"
            );
        }
    }
}

/// An edited keymap that does not hold together is refused at load, and text
/// that is not a keymap at all is a different failure.
///
/// @tests REQ-MYTH.keymap_is_data
/// @tests REQ-MYTH.actions_defined
#[test]
fn an_edit_that_breaks_the_keymap_is_refused_when_it_is_read() {
    let (text, _) = shipped();

    let typo = text.replace("history.undo", "history.undoo");
    match keymap::load(&typo, &keymap::actions()) {
        Err(LoadError::Invalid { problems }) => assert!(
            problems.iter().any(|p| format!("{p:?}").contains("history.undoo")),
            "the typo was not named: {problems:#?}"
        ),
        other => panic!("a keymap naming an action nobody has loaded: {other:?}"),
    }

    match keymap::load("{ not json", &keymap::actions()) {
        Err(LoadError::Unreadable { .. }) => {}
        other => panic!("unreadable text loaded: {other:?}"),
    }
}

/// Every key the shipped keymap binds is a name a frontend can produce.
///
/// `actions_reachable` is a claim about the keymap: from the root, some key
/// sequence reaches every action. That claim is true of the data and says
/// nothing about the editor, because the editor only reaches an action if the
/// frontend hands `step` the same string the file was written with. The space
/// bar is where the two came apart — `assets/keymap.json` spelled it `Space`,
/// both frontends handed over `" "`, `step` answered `PassThrough` for a bound
/// key, and the leader menu was unreachable while every law above still held.
///
/// So the name a frontend produces is a value too, and `keymap::typed` and
/// `keymap::NAMED` are where it is produced. A key in the shipped file that is
/// neither is a binding nobody can press.
///
/// @tests REQ-MYTH.actions_reachable
#[test]
fn every_key_the_shipped_keymap_binds_is_one_a_frontend_can_name() {
    let (_, keymap) = shipped();

    // The space is the character that cannot name itself, and the only one.
    assert_eq!(keymap::typed(' '), keymap::SPACE);
    for other in ['a', 'Z', '0', 'é', '→'] {
        assert_eq!(keymap::typed(other), other.to_string(), "{other} was renamed");
    }
    assert!(keymap::NAMED.contains(&keymap::SPACE), "the keymap spells a key no frontend reports");
    assert!(keymap::NAMED.contains(&LEAVE), "nothing names the key that leaves a mode");

    let mut bound = 0;
    for (mode, map) in &keymap.modes {
        for (key, _) in &map.bindings {
            bound += 1;
            if keymap::NAMED.contains(&key.as_str()) {
                continue;
            }
            let mut characters = key.chars();
            let (Some(character), None) = (characters.next(), characters.next()) else {
                panic!("{mode}/{key:?} is neither a named key nor a character, so nothing sends it")
            };
            assert_eq!(
                keymap::typed(character),
                *key,
                "{mode}/{key:?} is bound under a name no frontend produces: a frontend that \
                 received it would send {:?}",
                keymap::typed(character)
            );
        }
    }
    assert!(bound > 10, "only {bound} bindings were read; the walk is wrong");
}

/// Neither frontend names a key of its own.
///
/// Read rather than run: a key name is a string literal in a frontend, and the
/// failure is that it is not the keymap's string — which no value the frontend
/// computes can reveal, because the frontend and the keymap never meet except
/// at a key press a person makes. So the literals are read back and held to
/// `keymap::NAMED`, and the terminal frontend is required to get its characters
/// from `keymap::typed` rather than spelling them itself.
///
/// The web frontend is in a language TraceLean has no grammar for (ADR-0008),
/// so its claim is annotated here rather than in it.
///
/// @tests REQ-MYTH.actions_reachable
#[test]
fn no_frontend_names_a_key_of_its_own() {
    let root = project_root();
    let known: BTreeSet<&str> = keymap::NAMED.iter().copied().collect();

    // The terminal frontend: every literal in the function that names a key.
    let tui = std::fs::read_to_string(root.join("crates/tui/src/main.rs")).expect("the terminal");
    let body = between(&tui, "fn name_of(", "\n}").expect("the terminal frontend names keys");
    assert!(
        body.contains("keymap::typed("),
        "the terminal frontend spells a typed character itself, so it can disagree with the keymap"
    );
    for name in literals(body) {
        assert!(
            known.contains(name.as_str()),
            "the terminal frontend sends {name:?}, which the keymap cannot bind"
        );
    }

    // The web frontend: the set it filters key presses through.
    let web = std::fs::read_to_string(root.join("web/src/app.ts")).expect("the page");
    let set = between(&web, "const named = new Set([", "]);").expect("the page names keys");
    let named: BTreeSet<String> = literals(set).into_iter().collect();
    assert_eq!(
        named.iter().map(String::as_str).collect::<BTreeSet<&str>>(),
        known,
        "the page and the keymap do not agree on what the named keys are"
    );
    assert!(
        web.contains(&format!("\"{}\"", keymap::SPACE)),
        "the page never spells the space bar, so a browser's {:?} reaches the keymap unchanged",
        " "
    );
}

/// The text between two markers, exclusive — enough to read one function or one
/// list back out of a source file.
fn between<'a>(source: &'a str, from: &str, to: &str) -> Option<&'a str> {
    let start = source.find(from)? + from.len();
    let rest = &source[start..];
    Some(&rest[..rest.find(to)?])
}

/// Every double-quoted literal in a fragment of source, in order. Both
/// languages quote a key name the same way, which is why one reader does.
fn literals(source: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut rest = source;
    while let Some(open) = rest.find('"') {
        rest = &rest[open + 1..];
        let Some(close) = rest.find('"') else { break };
        out.push(rest[..close].to_string());
        rest = &rest[close + 1..];
    }
    out
}
