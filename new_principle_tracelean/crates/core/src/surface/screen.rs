use serde::{Deserialize, Serialize};

use crate::surface::produce::{menu_buffer, MenuEntry};
use crate::surface::view::{Buffer, BufferKind, Role};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Axis {
    Across,
    Down,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Direction {
    Left,
    Right,
    Up,
    Down,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Rect {
    pub left: u64,
    pub top: u64,
    pub width: u64,
    pub height: u64,
}

/// @implements REQ-SCREEN.screen_is_a_value
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum Layout {
    Pane { id: String, buffer: String },
    Split { axis: Axis, parts: Vec<(u64, Layout)> },
}

/// @implements REQ-SCREEN.screen_is_a_value
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Screen {
    pub opened: Vec<Buffer>,
    pub layout: Layout,
    pub focus: String,
    pub next_pane: u64,
}

pub fn panes(layout: &Layout) -> Vec<(String, String)> {
    match layout {
        Layout::Pane { id, buffer } => vec![(id.clone(), buffer.clone())],
        Layout::Split { parts, .. } => {
            let mut out = Vec::new();
            for (_, inner) in parts {
                out.extend(panes(inner));
            }
            out
        }
    }
}

pub fn pane_ids(layout: &Layout) -> Vec<String> {
    panes(layout).into_iter().map(|(id, _)| id).collect()
}

pub fn shown_buffers(layout: &Layout) -> Vec<String> {
    panes(layout).into_iter().map(|(_, buffer)| buffer).collect()
}

fn has_duplicate(names: &[String]) -> bool {
    names.iter().enumerate().any(|(at, name)| names[at + 1..].contains(name))
}

/// @implements REQ-SCREEN.panes_are_distinct
pub fn distinct_panes(layout: &Layout) -> bool {
    !has_duplicate(&pane_ids(layout))
}

/// @implements REQ-SCREEN.every_pane_is_opened
/// @implements REQ-SCREEN.focus_is_placed
/// @implements REQ-SCREEN.panes_are_distinct
/// @implements REQ-SCREEN.screen_is_a_value
/// @drt REQ-SCREEN.every_pane_is_opened
/// @drt REQ-SCREEN.focus_is_placed
/// @drt REQ-SCREEN.panes_are_distinct
/// @drt REQ-SCREEN.screen_is_a_value
pub fn coherent(screen: Screen) -> bool {
    let opened: Vec<String> = screen.opened.iter().map(|buffer| buffer.id.clone()).collect();
    shown_buffers(&screen.layout).iter().all(|buffer| opened.contains(buffer))
        && pane_ids(&screen.layout).contains(&screen.focus)
        && distinct_panes(&screen.layout)
}

fn running_cuts(extent: u64, total: u64, weights: &[u64]) -> Vec<u64> {
    let mut out = Vec::with_capacity(weights.len());
    let mut so_far = 0u64;
    for weight in weights {
        let upto = so_far + weight;
        let cut = if total == 0 { 0 } else { extent * upto / total };
        let previous = if total == 0 { 0 } else { extent * so_far / total };
        out.push(cut.saturating_sub(previous));
        so_far = upto;
    }
    out
}

/// @implements REQ-SCREEN.layout_tiles_the_region
pub fn shares(extent: u64, weights: Vec<u64>) -> Vec<u64> {
    let total: u64 = weights.iter().sum();
    if total == 0 {
        let ones: Vec<u64> = weights.iter().map(|_| 1).collect();
        running_cuts(extent, weights.len() as u64, &ones)
    } else {
        running_cuts(extent, total, &weights)
    }
}

fn basis_of(parts: &[(u64, Layout)]) -> Vec<u64> {
    let placing: Vec<u64> =
        parts.iter().map(|(_, inner)| u64::from(!panes(inner).is_empty())).collect();
    let weighted: Vec<u64> = parts
        .iter()
        .zip(&placing)
        .map(|((weight, _), places)| weight * places)
        .collect();
    if weighted.iter().sum::<u64>() == 0 {
        placing
    } else {
        weighted
    }
}

/// @implements REQ-SCREEN.layout_tiles_the_region
/// @drt REQ-SCREEN.layout_tiles_the_region
pub fn place(rect: Rect, layout: Layout) -> Vec<(String, Rect)> {
    match layout {
        Layout::Pane { id, .. } => vec![(id, rect)],
        Layout::Split { axis, parts } => {
            let extent = match axis {
                Axis::Across => rect.width,
                Axis::Down => rect.height,
            };
            let sizes = shares(extent, basis_of(&parts));
            let mut at = match axis {
                Axis::Across => rect.left,
                Axis::Down => rect.top,
            };
            let mut out = Vec::new();
            for (size, (_, inner)) in sizes.into_iter().zip(parts) {
                let here = match axis {
                    Axis::Across => {
                        Rect { left: at, top: rect.top, width: size, height: rect.height }
                    }
                    Axis::Down => {
                        Rect { left: rect.left, top: at, width: rect.width, height: size }
                    }
                };
                out.extend(place(here, inner));
                at += size;
            }
            out
        }
    }
}

fn is_opened(screen: &Screen, id: &str) -> bool {
    screen.opened.iter().any(|buffer| buffer.id == id)
}

fn set_buffer(pane: &str, buffer: &str, layout: Layout) -> Layout {
    match layout {
        Layout::Pane { id, buffer: shown } => {
            if id == pane {
                Layout::Pane { id, buffer: buffer.to_string() }
            } else {
                Layout::Pane { id, buffer: shown }
            }
        }
        Layout::Split { axis, parts } => Layout::Split {
            axis,
            parts: parts
                .into_iter()
                .map(|(weight, inner)| (weight, set_buffer(pane, buffer, inner)))
                .collect(),
        },
    }
}

/// @implements REQ-SCREEN.opened_outlives_shown
/// @drt REQ-SCREEN.opened_outlives_shown
pub fn show_buffer(id: String, screen: Screen) -> Screen {
    if is_opened(&screen, &id) {
        let layout = set_buffer(&screen.focus, &id, screen.layout);
        Screen { layout, ..screen }
    } else {
        screen
    }
}

/// @implements REQ-SCREEN.opened_outlives_shown
/// @implements REQ-SCREEN.station_opens_a_buffer
/// @drt REQ-SCREEN.station_opens_a_buffer
pub fn open_buffer(buffer: Buffer, screen: Screen) -> Screen {
    if is_opened(&screen, &buffer.id) {
        show_buffer(buffer.id.clone(), screen)
    } else {
        let id = buffer.id.clone();
        let mut opened = screen.opened;
        opened.push(buffer);
        show_buffer(id, Screen { opened, ..screen })
    }
}

pub const EXPLORER: &str = "explorer";
pub const DOCUMENT: &str = "document";
pub const SIDE: &str = "side";

fn hold(buffer: Buffer, mut opened: Vec<Buffer>) -> Vec<Buffer> {
    if !opened.iter().any(|held| held.id == buffer.id) {
        opened.push(buffer);
    }
    opened
}

/// @implements REQ-SCREEN.workbench_has_three_places
/// @drt REQ-SCREEN.workbench_has_three_places
pub fn workbench(listing: Buffer, document: Buffer, side: Buffer) -> Screen {
    let parts = vec![
        (1, Layout::Pane { id: EXPLORER.to_string(), buffer: listing.id.clone() }),
        (2, Layout::Pane { id: DOCUMENT.to_string(), buffer: document.id.clone() }),
        (1, Layout::Pane { id: SIDE.to_string(), buffer: side.id.clone() }),
    ];
    let opened = hold(side, hold(document, hold(listing, Vec::new())));
    Screen {
        opened,
        layout: Layout::Split { axis: Axis::Across, parts },
        focus: EXPLORER.to_string(),
        next_pane: 0,
    }
}

pub const DOCUMENT_RECORDS: [&str; 8] =
    ["requirement ", "judge ", "context ", "coverage ", "definitions of ", "uses of ", "search ", "keys"];

pub fn home_of(kind: &BufferKind) -> &'static str {
    match kind {
        BufferKind::Directory { .. } => EXPLORER,
        BufferKind::File { .. } | BufferKind::Review { .. } => DOCUMENT,
        BufferKind::Record { title } if DOCUMENT_RECORDS.iter().any(|lead| title.starts_with(lead)) => DOCUMENT,
        BufferKind::Menu { .. } | BufferKind::Record { .. } => SIDE,
    }
}

/// @implements REQ-SCREEN.buffer_goes_home
/// @drt REQ-SCREEN.buffer_goes_home
pub fn destination(kind: BufferKind, screen: Screen) -> String {
    let home = home_of(&kind);
    if pane_ids(&screen.layout).iter().any(|pane| pane == home) {
        home.to_string()
    } else {
        screen.focus
    }
}

fn split_at(pane: &str, fresh: &str, axis: Axis, layout: Layout) -> Option<Layout> {
    match layout {
        Layout::Pane { id, buffer } => {
            if id == pane {
                let second = Layout::Pane { id: fresh.to_string(), buffer: buffer.clone() };
                Some(Layout::Split {
                    axis,
                    parts: vec![(1, Layout::Pane { id, buffer }), (1, second)],
                })
            } else {
                None
            }
        }
        Layout::Split { axis: outer, parts } => {
            let mut done = false;
            let changed: Vec<(u64, Layout)> = parts
                .into_iter()
                .map(|(weight, inner)| {
                    if done {
                        return (weight, inner);
                    }
                    match split_at(pane, fresh, axis, inner.clone()) {
                        Some(split) => {
                            done = true;
                            (weight, split)
                        }
                        None => (weight, inner),
                    }
                })
                .collect();
            if done {
                Some(Layout::Split { axis: outer, parts: changed })
            } else {
                None
            }
        }
    }
}

/// @implements REQ-SCREEN.split_keeps_the_buffer
/// @implements REQ-SCREEN.panes_are_distinct
/// @drt REQ-SCREEN.split_keeps_the_buffer
pub fn split_focus(axis: Axis, screen: Screen) -> Screen {
    let taken = pane_ids(&screen.layout);
    let free = free_from(&taken, screen.next_pane, taken.len() + 1);
    match split_at(&screen.focus, &format!("pane{free}"), axis, screen.layout.clone()) {
        None => screen,
        Some(layout) => Screen { layout, next_pane: free + 1, ..screen },
    }
}

fn free_from(taken: &[String], start: u64, fuel: usize) -> u64 {
    let mut at = start;
    for _ in 0..fuel {
        if !taken.contains(&format!("pane{at}")) {
            return at;
        }
        at += 1;
    }
    at
}

fn without(buffer: &str, layout: Layout) -> Option<Layout> {
    match layout {
        Layout::Pane { id, buffer: shown } => {
            if shown == buffer {
                None
            } else {
                Some(Layout::Pane { id, buffer: shown })
            }
        }
        Layout::Split { axis, parts } => {
            let kept: Vec<(u64, Layout)> = parts
                .into_iter()
                .filter_map(|(weight, inner)| without(buffer, inner).map(|kept| (weight, kept)))
                .collect();
            match kept.len() {
                0 => None,
                1 => Some(kept.into_iter().next().expect("one part").1),
                _ => Some(Layout::Split { axis, parts: kept }),
            }
        }
    }
}

/// @implements REQ-SCREEN.close_collapses_the_pane
/// @drt REQ-SCREEN.close_collapses_the_pane
pub fn close_buffer(buffer: String, screen: Screen) -> Screen {
    let remaining: Vec<Buffer> =
        screen.opened.iter().filter(|held| held.id != buffer).cloned().collect();
    let Some(first) = remaining.first().cloned() else {
        return screen;
    };
    let layout = match without(&buffer, screen.layout.clone()) {
        Some(kept) => kept,
        None => Layout::Pane { id: screen.focus.clone(), buffer: first.id },
    };
    let focus = if pane_ids(&layout).contains(&screen.focus) {
        screen.focus.clone()
    } else {
        panes(&layout).first().map(|(id, _)| id.clone()).unwrap_or_default()
    };
    Screen { opened: remaining, layout, focus, ..screen }
}

fn holds_pane(pane: &str, layout: &Layout) -> bool {
    pane_ids(layout).iter().any(|id| id == pane)
}

fn shift_at(index: usize, amount: i64, mut parts: Vec<(u64, Layout)>) -> Vec<(u64, Layout)> {
    let (Some(here), Some(next)) =
        (parts.get(index).map(|part| part.0), parts.get(index + 1).map(|part| part.0))
    else {
        return parts;
    };
    let here_weight = here as i128 + amount as i128;
    let next_weight = next as i128 - amount as i128;
    if here_weight >= 1 && next_weight >= 1 {
        parts[index].0 = here_weight as u64;
        parts[index + 1].0 = next_weight as u64;
    }
    parts
}

fn index_holding(pane: &str, parts: &[(u64, Layout)]) -> Option<usize> {
    parts.iter().position(|(_, inner)| holds_pane(pane, inner))
}

fn resize_at(pane: &str, amount: i64, layout: Layout) -> Layout {
    match layout {
        Layout::Pane { id, buffer } => Layout::Pane { id, buffer },
        Layout::Split { axis, parts } => match index_holding(pane, &parts) {
            None => Layout::Split { axis, parts },
            Some(index) => {
                let nested = matches!(&parts[index].1, Layout::Split { axis: inner, .. } if *inner == axis);
                if nested {
                    Layout::Split {
                        axis,
                        parts: parts
                            .into_iter()
                            .enumerate()
                            .map(|(at, (weight, inner))| {
                                (weight, if at == index { resize_at(pane, amount, inner) } else { inner })
                            })
                            .collect(),
                    }
                } else if index + 1 < parts.len() {
                    Layout::Split { axis, parts: shift_at(index, amount, parts) }
                } else if index == 0 {
                    Layout::Split {
                        axis,
                        parts: parts
                            .into_iter()
                            .map(|(weight, inner)| (weight, resize_at(pane, amount, inner)))
                            .collect(),
                    }
                } else {
                    Layout::Split { axis, parts: shift_at(index - 1, -amount, parts) }
                }
            }
        },
    }
}

/// @implements REQ-SCREEN.resize_moves_one_divider
/// @implements REQ-SCREEN.resize_has_a_floor
/// @drt REQ-SCREEN.resize_moves_one_divider
/// @drt REQ-SCREEN.resize_has_a_floor
pub fn resize_focus(amount: i64, screen: Screen) -> Screen {
    let layout = resize_at(&screen.focus, amount, screen.layout);
    Screen { layout, ..screen }
}

fn overlaps(a: &Rect, b: &Rect, dir: Direction) -> bool {
    match dir {
        Direction::Left | Direction::Right => {
            a.top < b.top + b.height && b.top < a.top + a.height
        }
        Direction::Up | Direction::Down => {
            a.left < b.left + b.width && b.left < a.left + a.width
        }
    }
}

fn beyond(from: &Rect, other: &Rect, dir: Direction) -> bool {
    match dir {
        Direction::Left => other.left + other.width <= from.left,
        Direction::Right => from.left + from.width <= other.left,
        Direction::Up => other.top + other.height <= from.top,
        Direction::Down => from.top + from.height <= other.top,
    }
}

fn distance(from: &Rect, rect: &Rect, dir: Direction) -> u64 {
    match dir {
        Direction::Left => from.left.saturating_sub(rect.left + rect.width),
        Direction::Right => rect.left.saturating_sub(from.left + from.width),
        Direction::Up => from.top.saturating_sub(rect.top + rect.height),
        Direction::Down => rect.top.saturating_sub(from.top + from.height),
    }
}

fn earlier(one: &Rect, other: &Rect) -> bool {
    one.top < other.top || (one.top == other.top && one.left < other.left)
}

fn nearer<'a>(
    from: &Rect,
    dir: Direction,
    a: &'a (String, Rect),
    b: &'a (String, Rect),
) -> &'a (String, Rect) {
    let here = distance(from, &a.1, dir);
    let there = distance(from, &b.1, dir);
    if there < here {
        b
    } else if here < there {
        a
    } else if earlier(&b.1, &a.1) {
        b
    } else {
        a
    }
}

/// @implements REQ-SCREEN.focus_follows_geometry
/// @drt REQ-SCREEN.focus_follows_geometry
pub fn focus_step(rect: Rect, screen: Screen, dir: Direction) -> String {
    let placed = place(rect, screen.layout.clone());
    let Some(from) = placed.iter().find(|(id, _)| *id == screen.focus).map(|(_, r)| *r) else {
        return screen.focus;
    };
    let candidates: Vec<(String, Rect)> = placed
        .into_iter()
        .filter(|(id, other)| {
            *id != screen.focus && beyond(&from, other, dir) && overlaps(&from, other, dir)
        })
        .collect();
    let mut best: Option<&(String, Rect)> = None;
    for candidate in candidates.iter().rev() {
        best = Some(match best {
            None => candidate,
            Some(found) => nearer(&from, dir, candidate, found),
        });
    }
    match best {
        None => screen.focus,
        Some((id, _)) => id.clone(),
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum Arrangement {
    Split { axis: Axis },
    Close,
    Focus { dir: Direction },
    FocusPane { pane: String },
    Resize { amount: i64 },
    ShowBuffer { buffer: String },
}

fn focused_buffer(screen: &Screen) -> Option<String> {
    panes(&screen.layout)
        .into_iter()
        .find(|(pane, _)| *pane == screen.focus)
        .map(|(_, buffer)| buffer)
}

/// @implements REQ-SCREEN.one_arrangement_path
/// @drt REQ-SCREEN.one_arrangement_path
pub fn arrange(how: Arrangement, rect: Rect, screen: Screen) -> Screen {
    match how {
        Arrangement::Split { axis } => split_focus(axis, screen),
        Arrangement::Close => match focused_buffer(&screen) {
            None => screen,
            Some(buffer) => close_buffer(buffer, screen),
        },
        Arrangement::Focus { dir } => {
            let focus = focus_step(rect, screen.clone(), dir);
            Screen { focus, ..screen }
        }
        Arrangement::FocusPane { pane } => {
            if pane_ids(&screen.layout).contains(&pane) {
                Screen { focus: pane, ..screen }
            } else {
                screen
            }
        }
        Arrangement::Resize { amount } => resize_focus(amount, screen),
        Arrangement::ShowBuffer { buffer } => show_buffer(buffer, screen),
    }
}

fn folders_of(path: &str) -> Vec<&str> {
    let mut parts: Vec<&str> = path.split('/').collect();
    parts.pop();
    parts
}

fn distinguishing(path: &str, others: &[&str]) -> String {
    fn last<'a, 'b>(folders: &'b [&'a str], n: usize) -> &'b [&'a str] {
        &folders[folders.len().saturating_sub(n)..]
    }
    let mine = folders_of(path);
    let theirs: Vec<Vec<&str>> = others.iter().map(|other| folders_of(other)).collect();
    let n = (1..=mine.len())
        .find(|&n| theirs.iter().all(|folders| last(folders, n) != last(&mine, n)))
        .unwrap_or(mine.len());
    last(&mine, n).join("/")
}

fn strip_title(opened: &[Buffer], buffer: &Buffer) -> String {
    let BufferKind::File { path } = &buffer.kind else { return title_of(buffer) };
    let name = last_segment(path);
    let others: Vec<&str> = opened
        .iter()
        .filter_map(|other| match &other.kind {
            BufferKind::File { path: p } if p != path && last_segment(p) == name => Some(p.as_str()),
            _ => None,
        })
        .collect();
    let folders = distinguishing(path, &others);
    if others.is_empty() || folders.is_empty() {
        name
    } else {
        format!("{name} · {folders}")
    }
}

/// @implements REQ-SCREEN.strip_is_the_opened_set
/// @drt REQ-SCREEN.strip_is_the_opened_set
pub fn strip(screen: Screen) -> Buffer {
    strip_with_unsaved(screen, &[])
}

pub fn strip_with_unsaved(screen: Screen, unsaved: &[String]) -> Buffer {
    let entries = screen
        .opened
        .iter()
        .enumerate()
        .map(|(at, buffer)| {
            let dirty = matches!(&buffer.kind, BufferKind::File { path } if unsaved.contains(path));
            let title = strip_title(&screen.opened, buffer);
            MenuEntry {
                key: (at + 1).to_string(),
                description: if dirty { format!("{title} ●") } else { title },
                action: Some(format!("screen.show {}", buffer.id)),
            }
        })
        .collect();
    let mut rows = menu_buffer("opened".to_string(), entries);
    let focused = focused_buffer(&screen);
    for (span, buffer) in rows.spans.iter_mut().zip(&screen.opened) {
        if Some(&buffer.id) == focused.as_ref() {
            span.role = Role::Heading;
        }
    }
    rows
}

fn last_segment(path: &str) -> String {
    path.rsplit('/').next().unwrap_or(path).to_string()
}

pub fn title_of(buffer: &Buffer) -> String {
    match &buffer.kind {
        BufferKind::File { path } => last_segment(path),
        BufferKind::Directory { .. } => "files".to_string(),
        BufferKind::Review { target } => format!("review {}", last_segment(target)),
        BufferKind::Menu { title } | BufferKind::Record { title } => title.clone(),
    }
}

pub fn station_entries() -> Vec<MenuEntry> {
    [
        ("project", "Open a project"),
        ("trace", "What this file claims, and what else claims it"),
        ("sandbox", "Watch a sandboxed agent"),
        ("design", "Requirements, as the refinement graph"),
        ("history", "The undo tree"),
        ("findings", "What the checker found, and what to create"),
        ("definitions", "What can be configured: colours, keys, languages"),
    ]
    .into_iter()
    .map(|(key, description)| MenuEntry {
        key: key.to_string(),
        description: description.to_string(),
        action: Some(format!("screen.station.{key}")),
    })
    .collect()
}

/// @implements REQ-SCREEN.stations_are_constant
/// @drt REQ-SCREEN.stations_are_constant
pub fn stations(screen: Screen) -> Buffer {
    let _ = screen;
    menu_buffer("stations".to_string(), station_entries())
}

/// @implements REQ-SCREEN.station_produces_a_buffer
/// @drt REQ-SCREEN.station_produces_a_buffer
pub fn station_kind(station: String) -> Option<BufferKind> {
    match station.as_str() {
        "project" => Some(BufferKind::Directory { path: ".".to_string() }),
        "design" => Some(BufferKind::Menu { title: "design".to_string() }),
        "sandbox" => Some(BufferKind::Record { title: "sandbox".to_string() }),
        "history" => Some(BufferKind::Record { title: "history".to_string() }),
        "trace" => Some(BufferKind::Record { title: "trace".to_string() }),
        "findings" => Some(BufferKind::Record { title: "findings".to_string() }),
        "definitions" => Some(BufferKind::Record { title: "definitions".to_string() }),
        _ => None,
    }
}

pub const GOALS: &str = "record:proof";

fn is_lean(id: &str) -> bool {
    id.starts_with("file:") && id.ends_with(".lean")
}

/// @implements REQ-LSP.goals_open_with_the_file
/// @drt REQ-LSP.goals_open_with_the_file
pub fn opens_goals(kind: BufferKind, opened: Vec<String>) -> bool {
    matches!(&kind, BufferKind::File { path } if path.ends_with(".lean")) && !opened.iter().any(|id| id == GOALS)
}

/// @implements REQ-LSP.goals_close_with_the_file
/// @drt REQ-LSP.goals_close_with_the_file
pub fn closed_with(before: Vec<String>, after: Vec<String>) -> Vec<String> {
    let closed: Vec<&String> = before.iter().filter(|id| !after.contains(id)).collect();
    if closed.iter().any(|id| *id == GOALS) {
        after.into_iter().filter(|id| is_lean(id)).collect()
    } else if closed.iter().any(|id| is_lean(id)) && !after.iter().any(|id| is_lean(id)) && after.iter().any(|id| id == GOALS) {
        vec![GOALS.to_string()]
    } else {
        Vec::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn buffer(id: &str) -> Buffer {
        Buffer {
            id: id.to_string(),
            kind: BufferKind::File { path: id.to_string() },
            text: String::new(),
            spans: Vec::new(),
        }
    }

    fn screen(ids: &[&str]) -> Screen {
        Screen {
            opened: ids.iter().map(|id| buffer(id)).collect(),
            layout: Layout::Pane { id: "pane0".into(), buffer: ids[0].to_string() },
            focus: "pane0".into(),
            next_pane: 1,
        }
    }

    fn region() -> Rect {
        Rect { left: 0, top: 0, width: 80, height: 24 }
    }

    #[test]
    fn shares_tile_the_extent() {
        for weights in [vec![1, 1, 1], vec![3, 1], vec![7, 11, 2], vec![0, 0], vec![5]] {
            let got: u64 = shares(80, weights.clone()).iter().sum();
            assert_eq!(got, 80, "weights {weights:?}");
        }
    }

    #[test]
    fn a_split_covers_its_region_without_gap() {
        let layout = Layout::Split {
            axis: Axis::Across,
            parts: vec![
                (1, Layout::Pane { id: "a".into(), buffer: "one".into() }),
                (2, Layout::Pane { id: "b".into(), buffer: "two".into() }),
            ],
        };
        let placed = place(region(), layout);
        assert_eq!(placed[0].1.left, 0);
        assert_eq!(placed[0].1.left + placed[0].1.width, placed[1].1.left);
        assert_eq!(placed[1].1.left + placed[1].1.width, 80);
    }

    #[test]
    fn splitting_keeps_the_buffer_in_both_halves() {
        let after = split_focus(Axis::Across, screen(&["one"]));
        assert_eq!(shown_buffers(&after.layout), vec!["one", "one"]);
        assert!(distinct_panes(&after.layout));
        assert_eq!(after.next_pane, 2);
    }

    /// @tests REQ-SCREEN.panes_are_distinct
    #[test]
    fn splitting_mints_a_name_nobody_has_even_when_the_counter_lags() {
        let behind = Screen { next_pane: 0, ..screen(&["one"]) };
        assert!(coherent(behind.clone()), "the state this is about is a coherent one");
        let after = split_focus(Axis::Across, behind);
        assert!(
            distinct_panes(&after.layout),
            "a split minted an identity a pane already had: {:?}",
            pane_ids(&after.layout)
        );
    }

    #[test]
    fn showing_an_unopened_buffer_changes_nothing() {
        let before = screen(&["one"]);
        assert_eq!(show_buffer("two".into(), before.clone()), before);
    }

    #[test]
    fn opening_twice_holds_one_buffer() {
        let before = screen(&["one"]);
        let once = open_buffer(buffer("two"), before);
        let twice = open_buffer(buffer("two"), once.clone());
        assert_eq!(twice.opened.len(), 2);
        assert_eq!(twice, once);
    }

    #[test]
    fn closing_the_last_buffer_is_refused() {
        let before = screen(&["one"]);
        assert_eq!(close_buffer("one".into(), before.clone()), before);
    }

    #[test]
    fn closing_collapses_the_split_it_emptied() {
        let opened = screen(&["one", "two"]);
        let split = split_focus(Axis::Across, opened);
        let showing_two = show_buffer("two".into(), split);
        let after = close_buffer("two".into(), showing_two);
        assert!(matches!(after.layout, Layout::Pane { .. }));
        assert!(coherent(after));
    }

    /// @tests REQ-SCREEN.resize_has_a_floor
    #[test]
    fn resizing_cannot_take_a_pane_below_one() {
        let split = split_focus(Axis::Across, screen(&["one"]));
        let shrunk = resize_focus(-50, split);
        let Layout::Split { parts, .. } = &shrunk.layout else { panic!("a split") };
        assert_eq!((parts[0].0, parts[1].0), (1, 1));
    }

    /// @tests REQ-SCREEN.resize_has_a_floor
    #[test]
    fn a_resize_that_would_cross_the_floor_is_refused() {
        let two = |left: u64, right: u64| Screen {
            layout: Layout::Split {
                axis: Axis::Across,
                parts: vec![
                    (left, Layout::Pane { id: "a".into(), buffer: "one".into() }),
                    (right, Layout::Pane { id: "b".into(), buffer: "one".into() }),
                ],
            },
            focus: "a".into(),
            ..screen(&["one"])
        };
        assert_eq!(resize_focus(3, two(5, 2)), two(5, 2));
        assert_eq!(resize_focus(1, two(5, 2)), two(6, 1));
        assert_eq!(resize_focus(3, two(5, 0)), two(5, 0));
        assert_eq!(resize_focus(-2, two(5, 0)), two(3, 2));
    }

    #[test]
    fn resizing_moves_weight_between_two_parts_only() {
        let layout = Layout::Split {
            axis: Axis::Across,
            parts: vec![
                (5, Layout::Pane { id: "a".into(), buffer: "one".into() }),
                (5, Layout::Pane { id: "b".into(), buffer: "one".into() }),
                (5, Layout::Pane { id: "c".into(), buffer: "one".into() }),
            ],
        };
        let before = Screen { layout, focus: "a".into(), ..screen(&["one"]) };
        let after = resize_focus(2, before);
        let Layout::Split { parts, .. } = &after.layout else { panic!("a split") };
        assert_eq!(parts.iter().map(|part| part.0).collect::<Vec<_>>(), vec![7, 3, 5]);
    }

    #[test]
    fn the_last_part_resizes_against_the_divider_before_it() {
        let layout = Layout::Split {
            axis: Axis::Across,
            parts: vec![
                (5, Layout::Pane { id: "a".into(), buffer: "one".into() }),
                (5, Layout::Pane { id: "b".into(), buffer: "one".into() }),
            ],
        };
        let before = Screen { layout, focus: "b".into(), ..screen(&["one"]) };
        let after = resize_focus(2, before);
        let Layout::Split { parts, .. } = &after.layout else { panic!("a split") };
        assert_eq!(parts.iter().map(|part| part.0).collect::<Vec<_>>(), vec![3, 7]);
    }

    #[test]
    fn focus_moves_to_the_neighbour_and_stops_at_the_edge() {
        let layout = Layout::Split {
            axis: Axis::Across,
            parts: vec![
                (1, Layout::Pane { id: "a".into(), buffer: "one".into() }),
                (1, Layout::Pane { id: "b".into(), buffer: "one".into() }),
            ],
        };
        let at_a = Screen { layout, focus: "a".into(), ..screen(&["one"]) };
        assert_eq!(focus_step(region(), at_a.clone(), Direction::Right), "b");
        assert_eq!(focus_step(region(), at_a.clone(), Direction::Left), "a");
        assert_eq!(focus_step(region(), at_a, Direction::Up), "a");
    }

    #[test]
    fn the_strip_is_the_opened_set_in_order() {
        let opened = screen(&["one", "two", "three"]);
        let bar = strip(opened);
        assert_eq!(bar.text, "1  one\n2  two\n3  three");
        let actions: Vec<Vec<String>> = bar.spans.iter().map(|span| span.actions.clone()).collect();
        assert_eq!(
            actions,
            vec![vec!["screen.show one".to_string()], vec!["screen.show two".to_string()], vec![
                "screen.show three".to_string()
            ]]
        );
    }

    #[test]
    fn files_sharing_a_name_say_their_folder() {
        let file = |path: &str| of_kind(&format!("file:{path}"), BufferKind::File { path: path.into() });
        let mut opened = screen(&["x"]);
        opened.opened = vec![file("crates/tui/src/main.rs"), file("crates/desktop/src/main.rs"), file("src/lib.rs"), file("main.rs")];
        assert_eq!(strip(opened).text, "1  main.rs · tui/src\n2  main.rs · desktop/src\n3  lib.rs\n4  main.rs");
        assert_eq!(distinguishing("a/x/m.rs", &["b/x/m.rs", "m.rs"]), "a/x");
    }

    #[test]
    fn the_stations_do_not_depend_on_the_screen() {
        assert_eq!(stations(screen(&["one"])), stations(screen(&["other"])));
    }

    fn of_kind(id: &str, kind: BufferKind) -> Buffer {
        Buffer { kind, ..buffer(id) }
    }

    /// @tests REQ-SCREEN.workbench_has_three_places
    #[test]
    fn the_workbench_is_three_panes_side_by_side() {
        let listing = of_kind("dir", BufferKind::Directory { path: ".".into() });
        let side = of_kind("reqs", BufferKind::Menu { title: "requirements".into() });
        let opened = workbench(listing, buffer("empty"), side);
        assert!(coherent(opened.clone()));
        assert_eq!(
            panes(&opened.layout),
            vec![
                (EXPLORER.to_string(), "dir".to_string()),
                (DOCUMENT.to_string(), "empty".to_string()),
                (SIDE.to_string(), "reqs".to_string()),
            ]
        );
        assert_eq!(opened.focus, EXPLORER);
        let widths: Vec<u64> =
            place(region(), opened.layout).into_iter().map(|(_, at)| at.width).collect();
        assert_eq!(widths, vec![20, 40, 20]);
        let same = workbench(buffer("x"), buffer("x"), buffer("y"));
        assert_eq!(same.opened.len(), 2);
    }

    /// @tests REQ-SCREEN.buffer_goes_home
    #[test]
    fn a_buffer_goes_to_its_home_pane_while_there_is_one() {
        let bench = workbench(buffer("a"), buffer("b"), buffer("c"));
        let file = BufferKind::File { path: "x.rs".into() };
        let listing = BufferKind::Directory { path: "src".into() };
        let station = BufferKind::Menu { title: "requirements".into() };
        assert_eq!(destination(file.clone(), bench.clone()), DOCUMENT);
        assert_eq!(destination(listing, bench.clone()), EXPLORER);
        assert_eq!(destination(station.clone(), bench.clone()), SIDE);
        let closed = close_buffer("c".into(), bench);
        assert_eq!(destination(station, closed.clone()), closed.focus);
        assert_eq!(destination(file, screen(&["one"])), "pane0");
    }

    /// @tests REQ-SCREEN.resize_moves_one_divider
    #[test]
    fn a_pane_in_a_nested_split_moves_its_own_divider() {
        let pane = |id: &str| Layout::Pane { id: id.into(), buffer: id.into() };
        let nested = Layout::Split {
            axis: Axis::Across,
            parts: vec![
                (10, pane("first")),
                (
                    20,
                    Layout::Split { axis: Axis::Across, parts: vec![(5, pane("middle")), (5, pane("last"))] },
                ),
            ],
        };
        let weights = |layout: &Layout| match layout {
            Layout::Split { parts, .. } => parts.iter().map(|(w, _)| *w).collect::<Vec<_>>(),
            Layout::Pane { .. } => vec![],
        };
        let inner = |layout: &Layout| match layout {
            Layout::Split { parts, .. } => weights(&parts[1].1),
            Layout::Pane { .. } => vec![],
        };
        let moved = resize_at("middle", 2, nested.clone());
        assert_eq!(weights(&moved), vec![10, 20], "the outer divider moved");
        assert_eq!(inner(&moved), vec![7, 3], "the middle pane did not take the weight from the last");
        let moved = resize_at("last", 2, nested);
        assert_eq!(weights(&moved), vec![10, 20], "the outer divider moved");
        assert_eq!(inner(&moved), vec![3, 7]);
    }
}
