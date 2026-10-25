//! @implements REQ-ACT.everything_is_offered

use serde::{Deserialize, Serialize};

use crate::surface::keymap::{Binding, Keymap};
use crate::surface::view::{actions_at, Buffer, BufferKind};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Offer {
    pub group: String,
    pub label: String,
    pub action: String,
    pub target: Option<String>,
    pub asks: Option<String>,
    pub keys: Option<String>,
}

fn under(buffer: &Buffer, offset: usize) -> Option<String> {
    let text: Vec<char> = buffer.text.chars().collect();
    buffer
        .spans
        .iter()
        .find(|span| span.start <= offset && offset < span.stop)
        .map(|span| text[span.start.min(text.len())..span.stop.min(text.len())].iter().collect())
}

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

fn description_of(keymap: &Keymap, action: &str) -> Option<String> {
    keymap.modes.iter().flat_map(|(_, mode)| mode.bindings.iter()).find_map(|(_, binding)| match binding {
        Binding::Dispatch { action: named, description } if named == action => {
            let mut chars = description.chars();
            chars.next().map(|first| first.to_uppercase().chain(chars).collect())
        }
        _ => None,
    })
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

/// @implements REQ-ACT.everything_is_offered
pub fn offers(buffer: &Buffer, offset: usize, keymap: &Keymap) -> Vec<Offer> {
    let mut out = Vec::new();
    let here = under(buffer, offset);

    for action in actions_at(buffer.clone(), offset) {
        let name = crate::surface::view::action_target(&action).map(str::to_string).or(here.clone()).unwrap_or_default();
        let label = match crate::surface::view::action_name(&action) {
            "design.toggle" => format!("Unfold or fold {name}"),
            "design.expand_all" => format!("Unfold everything under {name}"),
            "design.expand_everything" => "Unfold all of the design".into(),
            "design.fold_all" => "Fold the design to its roots".into(),
            "design.open_only" => "Show only what is still to do, or everything".into(),
            "file.open" => format!("Open {name}"),
            "screen.show" => format!("Show {name}"),
            "observe.diff" => format!("Review the change to {name}"),
            "trace.requirement" => format!("Open {name}"),
            "trace.context" => format!("Context for an agent to change {name}"),
            "context.toggle" => format!("Include or leave out {name}"),
            "context.copy" => "Copy the context for an agent".into(),
            "history.filter" => format!("Show {name} in the history"),
            "trace.evidence" => format!("Evidence recorded for {name}"),
            "trace.findings" => "What the checker found".into(),
            "trace.check" => "Check the tree".into(),
            "trace.rollup" => "Coverage, requirement by requirement".into(),
            "observe.accept" => "Accept the agent's change".into(),
            "observe.reject" => "Reject the agent's change".into(),
            "observe.accept_file" => "Accept this file".into(),
            "observe.reject_file" => "Reject this file".into(),
            other => description_of(keymap, other).unwrap_or_else(|| other.to_string()),
        };
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
    let a_row = actions_at(buffer.clone(), offset).iter().any(|a| a == "file.open");
    if let (BufferKind::Directory { .. }, Some(path), true) = (&buffer.kind, &here, a_row) {
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
        BufferKind::Menu { title } if title == "requirements" || title == "design" => {
            if title == "design" {
                out.push(offer(keymap, "file", "Unfold all of the design".into(), "design.expand_everything", None, None));
                out.push(offer(keymap, "file", "Fold the design to its roots".into(), "design.fold_all", None, None));
            }
            out.push(offer(
                keymap,
                "file",
                "Show only what is still to do, or everything".into(),
                "design.open_only",
                None,
                None,
            ));
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

    out.push(offer(keymap, "panes", "Split right".into(), "screen.split.across", None, None));
    out.push(offer(keymap, "panes", "Split down".into(), "screen.split.down", None, None));
    out.push(offer(keymap, "panes", "Close this buffer".into(), "screen.close", None, None));

    for (action, label) in [
        ("screen.station.project", "Project files"),
        ("screen.station.trace", "What this file claims"),
        ("screen.station.design", "Requirements, as the refinement graph"),
        ("screen.station.sandbox", "Sandbox"),
        ("observe.start", "Look for an agent's changes"),
        ("trace.check", "Check the tree"),
        ("trace.findings", "Findings"),
        ("history.tree", "History"),
    ] {
        out.push(offer(keymap, "go", label.into(), action, None, None));
    }
    out.retain(|o| known(&o.action));
    out
}

pub fn known(action: &str) -> bool {
    use crate::surface::act::{dispatch, Blocked, Focus, Intent};
    let nowhere = Focus { kind: BufferKind::Menu { title: String::new() }, offset: 0, under: None };
    !matches!(
        dispatch(action.to_string(), nowhere, Default::default(), Vec::new()),
        Intent::Refuse { why: Blocked::UnknownAction { .. } }
    )
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Offered {
    pub group: String,
    pub action: String,
    pub target: Option<String>,
    pub keys: Option<String>,
}

/// @drt REQ-ACT.everything_is_offered
pub fn offered_owned(buffer: Buffer, offset: usize, keymap: Keymap) -> Vec<Offered> {
    offers(&buffer, offset, &keymap)
        .into_iter()
        .map(|o| Offered { group: o.group, action: o.action, target: o.target, keys: o.keys })
        .collect()
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

    /// @tests REQ-ACT.everything_is_offered
    #[test]
    fn every_action_and_every_kind_of_buffer_is_offered_in_words() {
        use crate::surface::view::{Role, Span};
        let actions = [
            ("trace.requirement", "Open REQ-X"),
            ("trace.context", "Context for an agent to change REQ-X"),
            ("context.toggle", "Include or leave out REQ-X"),
            ("context.copy", "Copy the context for an agent"),
            ("history.filter", "Show REQ-X in the history"),
            ("trace.evidence", "Evidence recorded for REQ-X"),
            ("trace.findings", "What the checker found"),
            ("trace.check", "Check the tree"),
            ("trace.rollup", "Coverage, requirement by requirement"),
            ("observe.accept", "Accept the agent's change"),
            ("observe.reject", "Reject the agent's change"),
            ("observe.accept_file", "Accept this file"),
            ("observe.reject_file", "Reject this file"),
            ("file.save", "Save"),
        ];
        let buffer = |kind: BufferKind, action: &str| Buffer {
            id: "b".into(),
            kind,
            text: "REQ-X".into(),
            spans: vec![Span { start: 0, stop: 5, role: Role::Requirement, actions: vec![action.to_string()] }],
        };
        let record = |title: &str| BufferKind::Record { title: title.into() };
        for (action, words) in actions {
            let offered = offers(&buffer(record("x"), action), 0, &keymap());
            let here = offered.iter().find(|o| o.group == "here").expect("the span's action");
            assert!(here.label.starts_with(words), "{action}: `{}`", here.label);
        }

        let file_actions = |kind: BufferKind| -> Vec<String> {
            offers(&buffer(kind, "trace.check"), 0, &keymap())
                .into_iter()
                .filter(|o| o.group == "file")
                .map(|o| o.action)
                .collect()
        };
        let sandbox = file_actions(record("sandbox"));
        for action in ["sandbox.new", "sandbox.copy", "observe.start", "observe.accept", "observe.reject", "sandbox.end"] {
            assert!(sandbox.iter().any(|a| a == action), "a sandbox offers no {action}");
        }
        let review = file_actions(BufferKind::Review { target: "a.rs".into() });
        assert!(review.iter().any(|a| a == "observe.accept_file") && review.iter().any(|a| a == "observe.reject_file"));
        assert!(!file_actions(BufferKind::Menu { title: "keys".into() }).iter().any(|a| a == "trace.new_requirement"));
    }

    #[test]
    fn an_offer_carries_the_keys_that_reach_it() {
        assert_eq!(keys_for(&keymap(), "file.save").as_deref(), Some("Space f s"));
        assert_eq!(keys_for(&keymap(), "no.such.action"), None);
    }
}
