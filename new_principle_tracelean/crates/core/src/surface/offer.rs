//! What can be done here: the list a pointer's context menu shows.
//!
//! Every action this editor has is reachable by keys, but a key sequence is
//! something a person has to already know. A context menu is the same set
//! offered where the person is pointing, so it is computed here, from the
//! buffer, the position and the keymap — not by a frontend deciding what a
//! file row "should" allow. Each offer carries the keys that reach it, so the
//! menu also teaches the keyboard.
//!
//! An offer names an action the dispatcher already knows. What it adds is the
//! *target* — the path that was pointed at — and, when the action needs one,
//! the question to ask for its argument (a new name, a new path). Building the
//! focus from those is the shell's job, as it is for a click on a bar.
//!
//! @implements REQ-ACT.everything_is_offered

use serde::{Deserialize, Serialize};

use crate::surface::keymap::{Binding, Keymap};
use crate::surface::view::{actions_at, Buffer, BufferKind};

/// One entry of a context menu.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Offer {
    /// Which part of the menu it belongs to: what was pointed at, the buffer,
    /// the panes, or somewhere to go.
    pub group: String,
    /// What a person reads.
    pub label: String,
    /// The action the dispatcher performs.
    pub action: String,
    /// What the action is about, when that is something other than where the
    /// cursor is — the path of a row in a listing.
    pub target: Option<String>,
    /// The question to ask before performing it, when it needs an argument
    /// nobody pointed at. `None` performs it at once.
    pub asks: Option<String>,
    /// The keys that reach it from the root mode, written as a person types
    /// them: `Space f s`.
    pub keys: Option<String>,
}

/// The text of the span under a position, which is what the position is about.
fn under(buffer: &Buffer, offset: usize) -> Option<String> {
    let text: Vec<char> = buffer.text.chars().collect();
    buffer
        .spans
        .iter()
        .find(|span| span.start <= offset && offset < span.stop)
        .map(|span| text[span.start.min(text.len())..span.stop.min(text.len())].iter().collect())
}

/// The keys that dispatch `action`, found by walking the keymap from its root.
///
/// The shortest sequence wins, so an action bound twice is offered by its
/// quicker binding.
pub fn keys_for(keymap: &Keymap, action: &str) -> Option<String> {
    let mut frontier: Vec<(String, Vec<String>)> = vec![(keymap.root.clone(), Vec::new())];
    let mut seen = vec![keymap.root.clone()];
    while !frontier.is_empty() {
        let mut next = Vec::new();
        for (mode, route) in frontier {
            let Some((_, found)) = keymap.modes.iter().find(|(name, _)| *name == mode) else {
                continue;
            };
            for (key, binding) in &found.bindings {
                let mut onwards = route.clone();
                onwards.push(key.clone());
                match binding {
                    Binding::Dispatch { action: bound, .. } if bound == action => {
                        return Some(onwards.join(" "));
                    }
                    Binding::Enter { mode: entered, .. } if !seen.contains(entered) => {
                        seen.push(entered.clone());
                        next.push((entered.clone(), onwards));
                    }
                    _ => {}
                }
            }
        }
        frontier = next;
    }
    None
}

/// What every bound action is called and the keys that reach it, as a
/// pointer's hover says it: `open — Space f o`. From the keymap's own
/// descriptions, so a tooltip and the which-key menu use the same words.
pub fn described(keymap: &Keymap) -> std::collections::BTreeMap<String, String> {
    let mut out = std::collections::BTreeMap::new();
    for (_, mode) in &keymap.modes {
        for (_, binding) in &mode.bindings {
            if let Binding::Dispatch { action, description } = binding {
                if out.contains_key(action) {
                    continue;
                }
                let said = match keys_for(keymap, action) {
                    Some(keys) => format!("{description} — {keys}"),
                    None => description.clone(),
                };
                out.insert(action.clone(), said);
            }
        }
    }
    out
}

fn offer(
    keymap: &Keymap,
    group: &str,
    label: String,
    action: &str,
    target: Option<String>,
    asks: Option<String>,
) -> Offer {
    Offer {
        group: group.to_string(),
        label,
        action: action.to_string(),
        target,
        asks,
        keys: keys_for(keymap, action),
    }
}

/// Everything that can be done at `offset` in `buffer`.
///
/// Three parts, in this order: what the span under the position offers, what
/// the buffer as a whole offers, and the places that are always reachable. No
/// action appears that the dispatcher does not know, and every action a span
/// declares at the position appears — so the menu is never less than what a
/// key on that position could do.
///
/// @implements REQ-ACT.everything_is_offered
pub fn offers(buffer: &Buffer, offset: usize, keymap: &Keymap) -> Vec<Offer> {
    let mut out = Vec::new();
    let here = under(buffer, offset);

    // What was pointed at.
    for action in actions_at(buffer.clone(), offset) {
        let name = here.clone().unwrap_or_default();
        let label = match action.as_str() {
            "file.open" => format!("Open {name}"),
            "screen.show" => format!("Show {name}"),
            "observe.diff" => format!("Review the change to {name}"),
            "trace.requirement" => format!("Open {name}"),
            "observe.accept_file" => "Accept this file".into(),
            "observe.reject_file" => "Reject this file".into(),
            other => other.to_string(),
        };
        // A button's text is not what it is about; the row it is on is, and
        // that is the shell's to resolve.
        let target = match action.as_str() {
            "observe.accept_file" | "observe.reject_file" => None,
            _ => here.clone(),
        };
        out.push(offer(keymap, "here", label, &action, target, None));
        if action == "observe.diff" {
            out.push(offer(keymap, "here", format!("Accept {name} only"), "observe.accept_file", here.clone(), None));
            out.push(offer(keymap, "here", format!("Reject {name} only"), "observe.reject_file", here.clone(), None));
        }
    }
    if let (BufferKind::Directory { .. }, Some(path)) = (&buffer.kind, &here) {
        out.push(offer(
            keymap,
            "here",
            format!("Rename {path}…"),
            "file.rename",
            Some(path.clone()),
            Some(format!("New name for {path}")),
        ));
        out.push(offer(keymap, "here", format!("Delete {path}"), "file.delete", Some(path.clone()), None));
        out.push(offer(keymap, "here", "Copy the path".into(), "file.copy_path", Some(path.clone()), None));
    }

    // What the buffer offers.
    out.push(offer(keymap, "file", "Copy this line".into(), "file.copy_line", None, None));
    out.push(offer(keymap, "file", "Copy all of it".into(), "file.copy", None, None));
    match &buffer.kind {
        BufferKind::File { path } => {
            out.push(offer(keymap, "file", "Go to the definition".into(), "file.definition", None, None));
            out.push(offer(keymap, "file", "Where this name is used".into(), "file.references", None, None));
            out.push(offer(keymap, "file", "Save".into(), "file.save", None, None));
            out.push(offer(keymap, "file", "Undo".into(), "history.undo", None, None));
            out.push(offer(keymap, "file", "Redo".into(), "history.redo", None, None));
            out.push(offer(
                keymap,
                "file",
                "Evidence for this anchor".into(),
                "trace.evidence",
                None,
                None,
            ));
            out.push(offer(
                keymap,
                "file",
                format!("Rename {path}…"),
                "file.rename",
                Some(path.clone()),
                Some(format!("New name for {path}")),
            ));
            out.push(offer(keymap, "file", format!("Delete {path}"), "file.delete", Some(path.clone()), None));
            out.push(offer(keymap, "file", "Copy the path".into(), "file.copy_path", Some(path.clone()), None));
        }
        BufferKind::Directory { path } => {
            out.push(offer(
                keymap,
                "file",
                "New file…".into(),
                "file.new",
                None,
                Some("Path of the new file".into()),
            ));
            // Another project: `file.open` on a folder's absolute path, which
            // the shell opens as the tree it works on.
            out.push(offer(
                keymap,
                "file",
                "Open another folder…".into(),
                "file.open",
                Some(path.clone()),
                Some("Folder to open, as an absolute path".into()),
            ));
        }
        BufferKind::Review { .. } | BufferKind::Record { .. } => {
            if matches!(&buffer.kind, BufferKind::Record { title } if title == "sandbox") {
                out.push(offer(keymap, "file", "New sandbox".into(), "sandbox.new", None, None));
                out.push(offer(keymap, "file", "Copy the sandbox command".into(), "sandbox.copy", None, None));
                out.push(offer(keymap, "file", "Look for changes".into(), "observe.start", None, None));
            }
            if matches!(&buffer.kind, BufferKind::Record { title } if title == "observed" || title == "sandbox")
                || matches!(buffer.kind, BufferKind::Review { .. })
            {
                out.push(offer(keymap, "file", "Accept the agent's changes".into(), "observe.accept", None, None));
                out.push(offer(keymap, "file", "Reject the agent's changes".into(), "observe.reject", None, None));
            }
            if let BufferKind::Review { target } = &buffer.kind {
                out.push(offer(keymap, "file", format!("Accept {target} only"), "observe.accept_file", Some(target.clone()), None));
                out.push(offer(keymap, "file", format!("Reject {target} only"), "observe.reject_file", Some(target.clone()), None));
            }
            if matches!(&buffer.kind, BufferKind::Record { title } if title == "sandbox") {
                out.push(offer(keymap, "file", "End the sandbox".into(), "sandbox.end", None, None));
            }
        }
        BufferKind::Menu { title } if title == "requirements" => {
            out.push(offer(
                keymap,
                "file",
                "New requirement…".into(),
                "trace.new_requirement",
                None,
                Some("Identifier of the new requirement, as REQ-NAME".into()),
            ));
        }
        BufferKind::Menu { .. } => {}
    }

    // The panes.
    out.push(offer(keymap, "panes", "Split right".into(), "screen.split.across", None, None));
    out.push(offer(keymap, "panes", "Split down".into(), "screen.split.down", None, None));
    out.push(offer(keymap, "panes", "Close this buffer".into(), "screen.close", None, None));

    // Places that are always reachable.
    for (action, label) in [
        ("screen.station.project", "Project files"),
        ("screen.station.requirements", "Requirements"),
        ("screen.station.design", "Refinement graph"),
        ("screen.station.sandbox", "Sandbox"),
        ("observe.start", "Look for an agent's changes"),
        ("trace.check", "Check the tree"),
        ("trace.findings", "Findings"),
        ("history.tree", "History"),
    ] {
        out.push(offer(keymap, "go", label.into(), action, None, None));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::surface::view::directory_buffer;

    fn keymap() -> Keymap {
        let text = include_str!("../../../../assets/keymap.json");
        crate::surface::keymap::load(text, &crate::surface::keymap::actions()).unwrap()
    }

    #[test]
    fn every_bound_action_is_described_with_its_keys() {
        let said = described(&keymap());
        assert_eq!(said.get("file.open").map(String::as_str), Some("open — Space f o"));
        assert!(crate::surface::keymap::ACTIONS.iter().all(|a| said.contains_key(*a)), "an action has no words");
    }

    /// @tests REQ-ACT.everything_is_offered
    #[test]
    fn a_listing_row_offers_open_rename_and_delete_for_that_path() {
        let listing = directory_buffer(".".into(), vec![(0, "src/a.rs".into())]);
        let offered = offers(&listing, 1, &keymap());
        let about: Vec<(&str, Option<&str>)> = offered
            .iter()
            .filter(|o| o.group == "here")
            .map(|o| (o.action.as_str(), o.target.as_deref()))
            .collect();
        assert_eq!(
            about,
            vec![
                ("file.open", Some("src/a.rs")),
                ("file.rename", Some("src/a.rs")),
                ("file.delete", Some("src/a.rs")),
                ("file.copy_path", Some("src/a.rs")),
            ]
        );
        assert!(offered.iter().any(|o| o.action == "file.new" && o.asks.is_some()));
    }

    /// Every action a span declares at the position is offered, and every
    /// offer is one the dispatcher knows.
    ///
    /// @tests REQ-ACT.everything_is_offered
    #[test]
    fn nothing_declared_is_missing_and_nothing_offered_is_unknown() {
        let listing = directory_buffer(".".into(), vec![(0, "a".into()), (0, "b".into())]);
        let known = crate::surface::keymap::actions();
        for offset in 0..listing.text.chars().count() {
            let offered = offers(&listing, offset, &keymap());
            for action in actions_at(listing.clone(), offset) {
                assert!(offered.iter().any(|o| o.action == action), "{action} at {offset}");
            }
            for o in &offered {
                assert!(known.contains(&o.action), "{} is not an action", o.action);
            }
        }
    }

    #[test]
    fn an_offer_carries_the_keys_that_reach_it() {
        assert_eq!(keys_for(&keymap(), "file.save").as_deref(), Some("Space f s"));
        assert_eq!(keys_for(&keymap(), "no.such.action"), None);
    }
}
