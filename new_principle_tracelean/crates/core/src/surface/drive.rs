use serde::{Deserialize, Serialize};

use super::keymap::{next_mode, which_key, Keymap};

/// @implements REQ-DRIVE.session_is_a_value
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Step {
    pub key: String,
    pub mode: String,
    pub menu: Vec<(String, String)>,
}

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

/// @implements REQ-DRIVE.walk_follows_the_machine
pub fn modes_visited(steps: &[Step]) -> Vec<String> {
    steps.iter().map(|step| step.mode.clone()).collect()
}

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

    #[test]
    fn a_key_nothing_binds_does_not_move_the_walk() {
        let map = keymap();
        let steps = drive(&map, "Normal", &keys(&[" "]));
        assert_eq!(modes_visited(&steps), vec!["Normal"]);
        assert_eq!(steps[0].menu, which_key(&map, "Normal"));
        assert!(
            !steps[0].menu.iter().any(|(key, _)| key == "t"),
            "an unbound key opened the leader menu"
        );
    }
}
