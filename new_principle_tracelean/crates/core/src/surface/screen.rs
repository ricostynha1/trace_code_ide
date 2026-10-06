//! What is opened, and what is shown.
//!
//! Implements `REQ-SCREEN`. `view` describes a buffer and `produce` says where
//! each one comes from; an editor shows several at once, and this is the value
//! that says which. Until it existed `shown` answered with one buffer, so a
//! window could draw one thing — not because anybody chose that, but because
//! nothing else was expressible.
//!
//! A pane carries its own identity. `split_keeps_the_buffer` puts one buffer in
//! two panes, and a focus naming the buffer could not say which half it meant.
//!
//! Weights are integers and shares are taken by running sums, so the parts of a
//! split add up to exactly the region. Fractions would give this, the Lean model
//! and the TypeScript frontend three answers at the last digit, and a
//! differential suite reporting rounding is one everybody learns to ignore.
//!
//! The encoding here is the one `TraceLean.Screen` writes by hand, because the
//! two are compared case for case.

use serde::{Deserialize, Serialize};

use crate::surface::produce::{menu_buffer, MenuEntry};
use crate::surface::view::{Buffer, BufferKind, Role};

/// Which way a split divides its region.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Axis {
    /// Parts side by side; the region's width is shared.
    Across,
    /// Parts stacked; the region's height is shared.
    Down,
}

/// Where to move the focus.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Direction {
    Left,
    Right,
    Up,
    Down,
}

/// A region of the screen, in characters.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Rect {
    pub left: u64,
    pub top: u64,
    pub width: u64,
    pub height: u64,
}

/// Which buffers are on screen, and where.
///
/// @implements REQ-SCREEN.screen_is_a_value
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum Layout {
    /// One buffer, filling its region.
    Pane { id: String, buffer: String },
    /// A region divided between parts, each weighted.
    Split { axis: Axis, parts: Vec<(u64, Layout)> },
}

/// Everything a session holds.
///
/// `next_pane` is a counter rather than a clock or a random number, so that
/// minting an identity stays a function of the value (`ARCH-DETERMINISM`).
///
/// @implements REQ-SCREEN.screen_is_a_value
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Screen {
    pub opened: Vec<Buffer>,
    pub layout: Layout,
    pub focus: String,
    pub next_pane: u64,
}

// ---------------------------------------------------------------- reading

/// Every pane a layout holds, as an identity and the buffer it shows, left to
/// right and top to bottom.
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

/// The identities of a layout's panes.
pub fn pane_ids(layout: &Layout) -> Vec<String> {
    panes(layout).into_iter().map(|(id, _)| id).collect()
}

/// The buffers a layout places.
pub fn shown_buffers(layout: &Layout) -> Vec<String> {
    panes(layout).into_iter().map(|(_, buffer)| buffer).collect()
}

fn has_duplicate(names: &[String]) -> bool {
    names.iter().enumerate().any(|(at, name)| names[at + 1..].contains(name))
}

/// No two panes share an identity.
///
/// @implements REQ-SCREEN.panes_are_distinct
pub fn distinct_panes(layout: &Layout) -> bool {
    !has_duplicate(&pane_ids(layout))
}

/// The screen holds together: every buffer placed is opened, the focused pane is
/// one the layout places, and no two panes share an identity.
///
/// One function because the three are one question — whether a frontend can
/// draw this value — and answering them separately would let a caller check two
/// and ship the third.
///
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

// ---------------------------------------------------------------- placing

fn running_cuts(extent: u64, total: u64, weights: &[u64]) -> Vec<u64> {
    let mut out = Vec::with_capacity(weights.len());
    let mut so_far = 0u64;
    for weight in weights {
        let upto = so_far + weight;
        // Integer division at each boundary, and a part takes the difference
        // between its boundary and the one before it. The last boundary is
        // `extent * total / total`, so the parts tile the region exactly.
        let cut = if total == 0 { 0 } else { extent * upto / total };
        let previous = if total == 0 { 0 } else { extent * so_far / total };
        out.push(cut.saturating_sub(previous));
        so_far = upto;
    }
    out
}

/// How much of `extent` each weight gets.
///
/// Weights that are all zero share the region evenly rather than taking none of
/// it: a layout whose parts all weigh nothing is still a layout, and answering
/// with nothing would leave a frontend with a region and no pane to draw in it.
///
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

/// What to divide a region by.
///
/// The parts' weights, with a part that places nothing zeroed. A split may hold
/// a part that draws nothing — a split of no parts, or one whose own parts are
/// all like that — and giving it a share leaves a hole: it takes extent and
/// produces no rectangle, and the region is no longer covered.
///
/// When every part that *does* place weighs nothing, the region is divided
/// evenly between them, which is `shares`' own rule applied to the mask rather
/// than to the weights, so a part placing nothing is still given nothing.
///
/// Found by the law rather than by review: `place` agreed with its Lean twin on
/// every generated case, because both left the same hole.
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

/// Where each pane goes, given the region the layout fills.
///
/// A layout that places any pane at all tiles its region: the shares of a split
/// add up to the region's extent along its axis, each part is placed where the
/// one before it ended, and a part that would place nothing is given nothing. A
/// layout with no panes places nothing, and there is no tiling to speak of.
///
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

// ------------------------------------------------- changing what is shown

fn is_opened(screen: &Screen, id: &str) -> bool {
    screen.opened.iter().any(|buffer| buffer.id == id)
}

/// Put `buffer` in the pane named `pane`, leaving every other pane alone.
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

/// Show an opened buffer in the focused pane.
///
/// A buffer that is not opened is not shown: the answer is the screen
/// unchanged, because showing something the session does not hold would put a
/// pane in front of a buffer nothing produced.
///
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

/// Open a buffer and show it.
///
/// A buffer already opened is *not* opened again and the one already held is
/// kept, so what it has accumulated survives being left and returned to. That is
/// the whole difference between a buffer and a panel rebuilt every time it
/// becomes visible.
///
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

// ------------------------------------------------------------ the workbench

/// The pane that holds what you choose from: listings.
pub const EXPLORER: &str = "explorer";
/// The pane that holds what you read and edit: files and reviews.
pub const DOCUMENT: &str = "document";
/// The pane that holds what you consult beside it: menus and records.
pub const SIDE: &str = "side";

/// Add a buffer to the opened set unless one of its identity is already held.
fn hold(buffer: Buffer, mut opened: Vec<Buffer>) -> Vec<Buffer> {
    if !opened.iter().any(|held| held.id == buffer.id) {
        opened.push(buffer);
    }
    opened
}

/// The screen a session opens on: three panes side by side — the explorer, the
/// document and the side panel — weighted one, two, one — the proportions of the first TraceLean's
/// sidebars beside its editor — with the focus on the explorer.
///
/// Named panes rather than minted ones, so that where a buffer belongs can be
/// said of a pane that a person may since have resized, split beside or
/// closed. Minted identities are `pane` and a number, so they never collide
/// with these.
///
/// A buffer offered twice is held once, as `open_buffer` would hold it.
///
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

/// The pane a buffer of this kind belongs in.
/// Records read like a file — an opened requirement, a judge's prompt, lists
/// of places with their lines — named by how their titles start.
pub const DOCUMENT_RECORDS: [&str; 7] =
    ["requirement ", "judge ", "context ", "definitions of ", "uses of ", "search ", "keys"];

pub fn home_of(kind: &BufferKind) -> &'static str {
    match kind {
        BufferKind::Directory { .. } => EXPLORER,
        BufferKind::File { .. } | BufferKind::Review { .. } => DOCUMENT,
        // A record read like a file is shown where files are.
        BufferKind::Record { title } if DOCUMENT_RECORDS.iter().any(|lead| title.starts_with(lead)) => DOCUMENT,
        BufferKind::Menu { .. } | BufferKind::Record { .. } => SIDE,
    }
}

/// Where a buffer of this kind is shown: its home pane while the layout places
/// one, and the focused pane once it does not.
///
/// The fallback is what keeps a person's own arrangement theirs. Close the
/// side panel and a station opens where you are rather than bringing the panel
/// back; the screen is never rearranged on a buffer's behalf.
///
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

// ------------------------------------------- splitting, closing, resizing

/// Replace the *first* pane named `pane` with a split of two, both showing what
/// it showed, the new half taking the identity `fresh`.
///
/// The first and not every one. Two panes may carry the same identity — nothing
/// in the type forbids it, which is why `panes_are_distinct` is a clause and not
/// an invariant — and splitting both would add two panes for one request and
/// mint one identity for both new halves. `None` when nothing matched is what
/// stops the traversal at the first hit.
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

/// Divide the focused pane, leaving its buffer in both halves.
///
/// Neither half is empty, so a split never produces a region with nothing to
/// draw in it. The new pane takes its identity from the counter the screen
/// carries, and the focus stays where it was — splitting is not a way of moving.
///
/// A focus on no pane splits nothing and mints no identity: a counter advanced
/// by a request that did nothing would leave a gap in the names for no reason.
///
/// @implements REQ-SCREEN.split_keeps_the_buffer
/// @implements REQ-SCREEN.panes_are_distinct
/// @drt REQ-SCREEN.split_keeps_the_buffer
pub fn split_focus(axis: Axis, screen: Screen) -> Screen {
    // The counter alone is not enough to mint a name nobody has. `next_pane` is
    // a field of a value anyone can build, so a screen with `pane0` placed and
    // `next_pane: 0` is coherent by every stated invariant and one split away
    // from two panes called `pane0` — which `panes_are_distinct` forbids, and
    // which makes a pane unnameable, the thing pane identities exist for.
    //
    // Found by an independent review of the clause. The generator could not
    // have found it: it draws pane ids from an alphabet that never spells
    // `paneN`, so the colliding case was unreachable by construction.
    let taken = pane_ids(&screen.layout);
    let free = free_from(&taken, screen.next_pane, taken.len() + 1);
    match split_at(&screen.focus, &format!("pane{free}"), axis, screen.layout.clone()) {
        None => screen,
        Some(layout) => Screen { layout, next_pane: free + 1, ..screen },
    }
}

/// The first `paneN` from `start` upward that nothing already placed is called.
///
/// `fuel` is one more than the number of panes placed, which is enough: among
/// that many consecutive candidates at most `fuel - 1` can be taken.
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

/// Drop every pane showing `buffer`, collapsing a split left with one part.
///
/// Answers `None` when nothing is left, which is how a caller learns that
/// removing would have emptied the region.
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

/// Close a buffer: drop it from the opened set, and give the region of every
/// pane showing it to the rest of the layout.
///
/// Two states are refused rather than entered. Closing the last opened buffer
/// answers unchanged, because a session with nothing opened has no layout to
/// draw. Closing one that fills every pane keeps the opened set's next buffer on
/// screen, because a region with no pane in it is not something a frontend can
/// render.
///
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

/// Move `amount` of weight from the part at `index + 1` to the part at `index`,
/// or the other way when it is negative, keeping both at one or more.
fn shift_at(index: usize, amount: i64, mut parts: Vec<(u64, Layout)>) -> Vec<(u64, Layout)> {
    let (Some(here), Some(next)) =
        (parts.get(index).map(|part| part.0), parts.get(index + 1).map(|part| part.0))
    else {
        return parts;
    };
    // What can actually move: the taker cannot fall below one, so the amount is
    // cut to what each side can spare. A cut rather than a refusal, because a
    // drag that went too far should stop at the floor and not undo itself.
    let room = if amount >= 0 { next.saturating_sub(1) } else { here.saturating_sub(1) } as i64;
    let moved = if amount >= 0 { amount.min(room) } else { amount.max(-room) };
    parts[index].0 = (here as i64 + moved).max(0) as u64;
    parts[index + 1].0 = (next as i64 - moved).max(0) as u64;
    parts
}

fn index_holding(pane: &str, parts: &[(u64, Layout)]) -> Option<usize> {
    parts.iter().position(|(_, inner)| holds_pane(pane, inner))
}

/// Resize the divider beside the pane named `pane`.
///
/// The divider *after* the part holding it, unless that part is last, in which
/// case the divider before it — so the last pane of a split can still be
/// resized.
fn resize_at(pane: &str, amount: i64, layout: Layout) -> Layout {
    match layout {
        Layout::Pane { id, buffer } => Layout::Pane { id, buffer },
        Layout::Split { axis, parts } => match index_holding(pane, &parts) {
            None => Layout::Split { axis, parts },
            Some(index) => {
                if index + 1 < parts.len() {
                    Layout::Split { axis, parts: shift_at(index, amount, parts) }
                } else if index == 0 {
                    // One part, or the pane is in the only part there is: nothing
                    // to move weight against, so descend and let an inner split
                    // answer.
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

/// Grow or shrink the focused pane against its neighbour.
///
/// @implements REQ-SCREEN.resize_moves_one_divider
/// @implements REQ-SCREEN.resize_has_a_floor
/// @drt REQ-SCREEN.resize_moves_one_divider
/// @drt REQ-SCREEN.resize_has_a_floor
pub fn resize_focus(amount: i64, screen: Screen) -> Screen {
    let layout = resize_at(&screen.focus, amount, screen.layout);
    Screen { layout, ..screen }
}

// --------------------------------------------------- moving the focus

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

/// Of two candidates, the one a move should land on: nearest in the direction
/// travelled, and when they are equally near, the earlier of the two in reading
/// order.
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

/// The pane a move in a direction lands on.
///
/// The nearest pane wholly beyond the focused one in that direction and
/// overlapping it across the direction of travel. When there is none — at the
/// edge of the screen, or with nothing alongside — the focus stays where it is,
/// because a move that wrapped round would take a person somewhere they did not
/// point at.
///
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
    // Folded from the right, exactly as the model's `pick` recurses: on a tie of
    // both distance and position the earlier candidate wins, and the direction
    // of the fold is what decides that.
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

// ------------------------------------------------------------ one way in

/// A change to the arrangement, named rather than performed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum Arrangement {
    /// Divide the focused pane.
    Split { axis: Axis },
    /// Close the buffer the focused pane shows.
    Close,
    /// Move the focus in a direction, which is what a key does.
    Focus { dir: Direction },
    /// Focus a named pane, which is what a pointer does: clicking in a pane, or
    /// grabbing the divider on its edge. A pane the layout does not place is not
    /// focused, because a focus off the layout is one no frontend can draw.
    FocusPane { pane: String },
    /// Grow the focused pane by this much, or shrink it when negative.
    Resize { amount: i64 },
    /// Show an already-opened buffer in the focused pane.
    ShowBuffer { buffer: String },
}

/// The buffer the focused pane shows, if the focus is on a pane at all.
fn focused_buffer(screen: &Screen) -> Option<String> {
    panes(&screen.layout)
        .into_iter()
        .find(|(pane, _)| *pane == screen.focus)
        .map(|(_, buffer)| buffer)
}

/// Make a change to the arrangement.
///
/// Every change to the layout comes through here. A terminal resizes with a key
/// and a window resizes by dragging a divider; the easy thing is for each
/// frontend to work the new weights out itself, and then there are two answers
/// to what a resize does and only one is ever tested. A drag becomes an amount
/// before it arrives here, not after.
///
/// The region is an argument because moving the focus is a question about
/// geometry, and geometry is not in the screen: the same layout in a narrow
/// terminal and a wide window has the panes in different places.
///
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

// ------------------------------------------------- the strip and stations

/// The opened set, as a buffer.
///
/// A rendering of `opened` rather than a list kept beside it: a strip maintained
/// separately is a second answer to what the session holds, which is the failure
/// `REQ-VIEW` prevents one level up. The row carries `screen.show` and nothing
/// more — which buffer it means is the row under the cursor, the way
/// `REQ-ACT.focus_is_carried` already resolves a target.
///
/// @implements REQ-SCREEN.strip_is_the_opened_set
/// @drt REQ-SCREEN.strip_is_the_opened_set
/// The folders a path sits in, outermost first.
fn folders_of(path: &str) -> Vec<&str> {
    let mut parts: Vec<&str> = path.split('/').collect();
    parts.pop();
    parts
}

/// The fewest innermost folders of `path` that no other path's folders end
/// with: `tui/src` beside `desktop/src`.
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

/// A tab's title among the others opened: a file whose name another opened
/// file shares says the folders that tell it apart, so two `main.rs` read
/// `main.rs · tui/src` and `main.rs · desktop/src`.
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

pub fn strip(screen: Screen) -> Buffer {
    strip_with_unsaved(screen, &[])
}

/// The strip, with `●` after each file that has changes not yet saved.
///
/// `strip` is this with nothing unsaved, so the differential test of `strip`
/// runs this code. The rows are the same either way — a row still means the
/// same buffer; only a title grows a mark after it.
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
                action: Some("screen.show".to_string()),
            }
        })
        .collect();
    let mut rows = menu_buffer("opened".to_string(), entries);
    // The row of the buffer the focused pane shows is a heading: the active tab.
    let focused = focused_buffer(&screen);
    for (span, buffer) in rows.spans.iter_mut().zip(&screen.opened) {
        if Some(&buffer.id) == focused.as_ref() {
            span.role = Role::Heading;
        }
    }
    rows
}

/// The last part of a path: the name a tab shows.
fn last_segment(path: &str) -> String {
    path.rsplit('/').next().unwrap_or(path).to_string()
}

/// What a buffer is called on its tab: a file by its name, a listing as the
/// files, a change by what it changes, anything else by its title.
///
/// The identity stays the buffer's id; this is only what a person reads. Two
/// files of one name in different directories share a title, and the row's
/// number still tells them apart.
pub fn title_of(buffer: &Buffer) -> String {
    match &buffer.kind {
        BufferKind::File { path } => last_segment(path),
        BufferKind::Directory { .. } => "files".to_string(),
        BufferKind::Review { target } => format!("review {}", last_segment(target)),
        BufferKind::Menu { title } | BufferKind::Record { title } => title.clone(),
    }
}

/// The six stations, in the order they are always in.
///
/// Each row carries its own action rather than a shared one taking the row as a
/// target, so that a station is reachable from a bare keyboard as well as from
/// a pointer (see `surface::act`).
pub fn station_entries() -> Vec<MenuEntry> {
    [
        ("project", "Open a project"),
        ("trace", "What this file claims, and what else claims it"),
        ("sandbox", "Watch a sandboxed agent"),
        ("requirements", "Requirements and clauses"),
        ("design", "The refinement graph"),
        ("history", "The undo tree"),
    ]
    .into_iter()
    .map(|(key, description)| MenuEntry {
        key: key.to_string(),
        description: description.to_string(),
        action: Some(format!("screen.station.{key}")),
    })
    .collect()
}

/// The stations, as a buffer.
///
/// A function of the screen that reads none of it, which is the clause: a
/// station list derived from the current project is unreachable exactly when no
/// project is open, and that is when the station that opens one is most needed.
///
/// @implements REQ-SCREEN.stations_are_constant
/// @drt REQ-SCREEN.stations_are_constant
pub fn stations(screen: Screen) -> Buffer {
    // Taken and then ignored, deliberately. `stations_are_constant` is the claim
    // that the answer does not depend on this argument, and a function that did
    // not take it could not be compared against one that might.
    let _ = screen;
    menu_buffer("stations".to_string(), station_entries())
}

/// What a station stands for: the kind of buffer pressing it produces.
///
/// One function rather than a case in each frontend, for the reason every
/// producer is in the core: a window that knew `requirements` meant the
/// requirement index and a terminal that did not would be two editors.
///
/// A name that is not a station's answers `None`. That is not a gap — it is how
/// the dispatcher refuses an action nobody declared, and it is what makes
/// `every_station_produces` falsifiable rather than true by construction.
///
/// @implements REQ-SCREEN.station_produces_a_buffer
/// @drt REQ-SCREEN.station_produces_a_buffer
pub fn station_kind(station: String) -> Option<BufferKind> {
    match station.as_str() {
        "project" => Some(BufferKind::Directory { path: ".".to_string() }),
        "requirements" => Some(BufferKind::Menu { title: "requirements".to_string() }),
        "design" => Some(BufferKind::Menu { title: "design".to_string() }),
        "sandbox" => Some(BufferKind::Record { title: "sandbox".to_string() }),
        "history" => Some(BufferKind::Record { title: "history".to_string() }),
        "trace" => Some(BufferKind::Record { title: "trace".to_string() }),
        _ => None,
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
        // The property the running-sum allocation exists for: whatever the
        // weights, the parts add up to the region and nothing is lost to
        // rounding.
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

    /// Splitting mints a name nobody already has, whatever the counter says.
    ///
    /// `next_pane` is a field of a value anyone can build. This screen is
    /// coherent by every stated invariant and its counter has fallen behind
    /// what is placed — and the old mint answered with two panes called
    /// `pane0`, which `panes_are_distinct` forbids and which leaves a pane
    /// nothing can name.
    ///
    /// The test above could never have caught it: its counter is ahead, which
    /// is the one state where the collision cannot happen. Nor could the
    /// generator, which draws pane ids from an alphabet that never spells
    /// `paneN`.
    ///
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
        // One pane left, and it is a pane rather than a split of one.
        assert!(matches!(after.layout, Layout::Pane { .. }));
        assert!(coherent(after));
    }

    #[test]
    fn resizing_cannot_take_a_pane_below_one() {
        let split = split_focus(Axis::Across, screen(&["one"]));
        let shrunk = resize_focus(-50, split);
        let Layout::Split { parts, .. } = &shrunk.layout else { panic!("a split") };
        assert_eq!(parts[0].0, 1);
        // The weight the floor refused to give up stayed where it was: a
        // resize moves weight, it does not destroy it.
        assert_eq!(parts[0].0 + parts[1].0, 2);
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
        assert!(bar.spans.iter().all(|span| span.actions == vec!["screen.show".to_string()]));
    }

    /// Two opened files with one name say which folder each is in.
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
        // One buffer offered for two places is held once.
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
        // With the side panel closed, a station opens where the focus is.
        let closed = close_buffer("c".into(), bench);
        assert_eq!(destination(station, closed.clone()), closed.focus);
        // And a screen that never had the places keeps its focus.
        assert_eq!(destination(file, screen(&["one"])), "pane0");
    }
}
