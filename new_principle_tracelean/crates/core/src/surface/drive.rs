//! Driving the editor: what a sequence of keys does.
//!
//! Every arrow of the surface was modelled and differentially tested, and the
//! editor still could not be used. A person does not press one key against one
//! mode; they press a sequence, and the sequence was the one thing nothing
//! stated. `REQ-DRIVE` states it here, as data in and data out, so the suite
//! that drives a real terminal has something to compare against that is not the
//! terminal's own account of itself.
//!
//! Nothing here decides what a key does. `keymap::next_mode` decides that and
//! `keymap::which_key` decides what is offered; this is the fold, which is the
//! part a frontend gets wrong.

use serde::{Deserialize, Serialize};

use super::keymap::{next_mode, which_key, Keymap};

/// What one key press did: the mode it left the machine in, and the bar there.
///
/// The key is carried so a step says which press it was without the reader
/// counting, which is what a divergence report needs.
///
/// @implements REQ-DRIVE.session_is_a_value
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Step {
    pub key: String,
    pub mode: String,
    pub menu: Vec<(String, String)>,
}

/// A session: one step per key, in order.
///
/// Each key is applied to the mode the previous one left, never to the mode the
/// session started in — that is `walk_follows_the_machine`. The bar reported
/// with a key is the bar of the mode that key *reached*, because that is what a
/// person sees after pressing it — that is `menu_is_the_bar_there`.
///
/// Total and pure: a session of no keys is no steps, and nothing outside the
/// arguments is read.
///
/// @implements REQ-DRIVE.session_is_a_value
/// @implements REQ-DRIVE.walk_follows_the_machine
/// @implements REQ-DRIVE.menu_is_the_bar_there
/// @drt REQ-DRIVE.session_is_a_value
pub fn drive(keymap: &Keymap, mode: &str, keys: &[String]) -> Vec<Step> {
    let mut at = mode.to_string();
    let mut steps = Vec::with_capacity(keys.len());
    for key in keys {
        at = next_mode(keymap.clone(), at, key.clone());
        steps.push(Step { key: key.clone(), mode: at.clone(), menu: which_key(keymap, &at) });
    }
    steps
}

/// The modes a session passed through, in order.
///
/// A projection rather than a second walk: a reader that computed the modes for
/// itself could disagree with the session it is reading, which is the drift this
/// requirement exists to catch.
///
/// @implements REQ-DRIVE.walk_follows_the_machine
pub fn modes_visited(steps: &[Step]) -> Vec<String> {
    steps.iter().map(|step| step.mode.clone()).collect()
}

/// `drive`, in the shape a conformance runner calls.
///
/// @implements REQ-DRIVE.session_is_a_value
/// @drt REQ-DRIVE.session_is_a_value
/// @drt REQ-DRIVE.walk_follows_the_machine
/// @drt REQ-DRIVE.menu_is_the_bar_there
pub fn drive_of(keymap: Keymap, mode: String, keys: Vec<String>) -> Vec<Step> {
    drive(&keymap, &mode, &keys)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::surface::keymap::{Binding, Mode};

    fn keymap() -> Keymap {
        Keymap {
            root: "Normal".into(),
            modes: vec![
                (
                    "Normal".into(),
                    Mode {
                        parent: None,
                        bindings: vec![(
                            "Space".into(),
                            Binding::Enter { mode: "Leader".into(), description: "leader".into() },
                        )],
                    },
                ),
                (
                    "Leader".into(),
                    Mode {
                        parent: Some("Normal".into()),
                        bindings: vec![(
                            "t".into(),
                            Binding::Dispatch {
                                action: "trace.check".into(),
                                description: "check".into(),
                            },
                        )],
                    },
                ),
            ],
        }
    }

    fn keys(of: &[&str]) -> Vec<String> {
        of.iter().map(|k| k.to_string()).collect()
    }

    #[test]
    fn pressing_nothing_does_nothing() {
        assert!(drive(&keymap(), "Normal", &[]).is_empty());
    }

    #[test]
    fn a_session_answers_once_per_key_and_walks_the_machine() {
        let steps = drive(&keymap(), "Normal", &keys(&["Space", "t", "Space"]));
        assert_eq!(steps.len(), 3);
        // Space enters the leader, `t` dispatches and so returns to the root,
        // and Space enters it again — the walk, not three answers to the same
        // question.
        assert_eq!(modes_visited(&steps), vec!["Leader", "Normal", "Leader"]);
    }

    #[test]
    fn a_step_shows_the_bar_of_the_mode_it_reached() {
        let map = keymap();
        let steps = drive(&map, "Normal", &keys(&["Space"]));
        assert_eq!(steps[0].menu, which_key(&map, "Leader"));
        assert!(
            steps[0].menu.iter().any(|(key, _)| key == "t"),
            "the leader's bar does not offer what the leader binds"
        );
    }

    /// The failure this requirement was written for: a key the keymap does not
    /// bind leaves the walk where it was, and a session says so rather than
    /// pretending the menu opened.
    #[test]
    fn a_key_nothing_binds_does_not_move_the_walk() {
        let map = keymap();
        let steps = drive(&map, "Normal", &keys(&[" "]));
        assert_eq!(modes_visited(&steps), vec!["Normal"]);
        // Still the root's own bar, which is how a session reports that nothing
        // opened: the leader's `t` is not on offer.
        assert_eq!(steps[0].menu, which_key(&map, "Normal"));
        assert!(
            !steps[0].menu.iter().any(|(key, _)| key == "t"),
            "an unbound key opened the leader menu"
        );
    }
}
