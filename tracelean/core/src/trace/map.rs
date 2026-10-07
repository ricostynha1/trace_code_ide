//! The coverage map: the project laid out by which requirement each part of it
//! serves.
//!
//! The point of this view is the grey. A map that showed only annotated code
//! would flatter the project; showing every file, sized honestly, is what makes
//! "how much of this is traced at all" answerable at a glance.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use super::{Level, TraceIndex};

/// One rectangle: a file, or a symbol within one.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MapEntry {
    pub file: PathBuf,
    /// `None` for the file as a whole.
    pub symbol: Option<String>,
    /// Size of the rectangle.
    pub lines: u32,
    /// Requirements this part serves. Empty means untraced — the grey.
    pub requirements: Vec<String>,
    /// Weakest evidence across those requirements, when there is any.
    pub weakest: Option<Level>,
    pub stale: bool,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CoverageMap {
    pub entries: Vec<MapEntry>,
    pub traced_lines: u32,
    pub total_lines: u32,
}

impl CoverageMap {
    /// Fraction of lines under some annotation. Deliberately line-weighted: a
    /// thousand-line untraced module should not look the same as a one-line
    /// one.
    pub fn traced_fraction(&self) -> f32 {
        if self.total_lines == 0 {
            0.0
        } else {
            self.traced_lines as f32 / self.total_lines as f32
        }
    }
}

/// Build the map from the index plus the files on disk.
///
/// Every source line is attributed to **at most one** anchor. The earlier
/// version pushed one rectangle per *link*, so a function carrying three
/// `@implements` annotations produced three rectangles over the same nine lines
/// and added those nine lines to the covered total three times — a `.min(total)`
/// clamp then hid the overcount in small files and left it in large ones, which
/// made the headline percentage wrong in the flattering direction.
///
/// Attribution goes to the **innermost** anchor covering a line, so an explicit
/// `begin`/`@end` region inside an annotated declaration is credited to the
/// region rather than to the declaration that contains it. That is the same
/// "most specific answer wins" rule the cursor menu uses.
pub fn build(root: &Path, index: &TraceIndex) -> CoverageMap {
    // Links grouped by the anchor they resolved to, so an anchor claimed by
    // several requirements is one rectangle carrying all of them.
    let mut by_file: BTreeMap<PathBuf, BTreeMap<String, Vec<&super::Link>>> = BTreeMap::new();
    for link in &index.links {
        by_file
            .entry(link.anchor.file.clone())
            .or_default()
            .entry(link.anchor.ident())
            .or_default()
            .push(link);
    }

    let mut map = CoverageMap::default();

    for file in &index.scanned_files {
        // Requirement documents describe the project; they are not part of it.
        if file.extension().and_then(|e| e.to_str()) == Some("md") {
            continue;
        }

        let Ok(content) = std::fs::read_to_string(root.join(file)) else { continue };
        let total = content.lines().count() as u32;
        map.total_lines += total;

        let Some(anchors) = by_file.get(file) else {
            map.entries.push(MapEntry {
                file: file.clone(),
                symbol: None,
                lines: total,
                requirements: Vec::new(),
                weakest: None,
                stale: false,
            });
            continue;
        };

        // Outermost first, so an inner anchor overwrites its container and ends
        // up owning the lines it is actually about.
        let mut spans: Vec<(&String, &Vec<&super::Link>)> = anchors.iter().collect();
        spans.sort_by_key(|(_, links)| {
            std::cmp::Reverse(links[0].anchor.end_line.saturating_sub(links[0].anchor.start_line))
        });

        let mut owner: Vec<Option<usize>> = vec![None; total as usize];
        for (slot, (_, links)) in spans.iter().enumerate() {
            let anchor = &links[0].anchor;
            let from = anchor.start_line as usize;
            let to = (anchor.end_line as usize).min(total.saturating_sub(1) as usize);
            for line in owner.iter_mut().take(to + 1).skip(from) {
                *line = Some(slot);
            }
        }

        let mut owned = vec![0u32; spans.len()];
        let mut untraced = 0u32;
        for slot in &owner {
            match slot {
                Some(i) => owned[*i] += 1,
                None => untraced += 1,
            }
        }

        for (slot, (_, links)) in spans.iter().enumerate() {
            if owned[slot] == 0 {
                // Entirely covered by an inner anchor. Reporting a zero-line
                // rectangle would be a tile nobody can click.
                continue;
            }
            let anchor = &links[0].anchor;

            let mut requirements: Vec<String> =
                links.iter().map(|l| l.req_id.clone()).collect();
            requirements.sort();
            requirements.dedup();

            // Several requirements on one anchor are worth their weakest, the
            // same rule the roll-up uses. A rectangle tinted by the best of
            // them would be the average-instead-of-minimum mistake, locally.
            let mut weakest: Option<Level> = None;
            let mut stale = false;
            for link in links.iter() {
                let assurance = index.assurance(&link.req_id, link.clause.as_deref());
                stale |= !assurance.stale.is_empty();
                if assurance.requirement_model.is_some()
                    || assurance.model_impl.is_some()
                    || assurance.model_proof.is_some()
                {
                    let level = assurance.weakest();
                    weakest = Some(match weakest {
                        Some(current) if current <= level => current,
                        _ => level,
                    });
                }
            }

            map.entries.push(MapEntry {
                file: file.clone(),
                symbol: anchor.symbol_path().map(|s| s.to_string()),
                lines: owned[slot],
                requirements,
                weakest,
                stale,
            });
        }

        map.traced_lines += total - untraced;
        if untraced > 0 {
            map.entries.push(MapEntry {
                file: file.clone(),
                symbol: None,
                lines: untraced,
                requirements: Vec::new(),
                weakest: None,
                stale: false,
            });
        }
    }

    map.entries.sort_by(|a, b| {
        b.lines
            .cmp(&a.lines)
            .then_with(|| a.file.cmp(&b.file))
            .then_with(|| a.symbol.cmp(&b.symbol))
    });
    map
}

/// Links anchored in one file, for the editor's gutter chips.
pub fn links_in_file<'a>(index: &'a TraceIndex, file: &Path) -> Vec<&'a super::Link> {
    let mut links: Vec<&super::Link> =
        index.links.iter().filter(|l| l.anchor.file == file).collect();
    links.sort_by_key(|l| l.anchor.start_line);
    links
}
