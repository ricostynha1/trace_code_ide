//! The tutorial is one text with sections that stay folded until asked for.
//!
//! A line `+++ Title` starts a folded section and a line `+++` ends it. Folded, the
//! section is one row, `▸ Title`; unfolded it is `▾ Title` and its lines. The row is
//! what carries the action that unfolds it, so the keyboard and the pointer reach it alike.
//! Sections nest: a fold inside a fold is a row only while the one around it is open, and
//! its row is indented two characters for each fold around it.

pub const FOLD: &str = "+++";
pub const SHUT: char = '▸';
pub const OPEN: char = '▾';

/// The action a fold's row carries.
pub const MORE: &str = "help.more";

/// The title a fold's row names, with its mark taken off; the row as it is when it has none.
pub fn title_of(row: &str) -> &str {
    row.strip_prefix(SHUT).or_else(|| row.strip_prefix(OPEN)).map_or(row, str::trim_start)
}

/// The tutorial as shown with the sections named in `open` unfolded: its text, and for each
/// fold the characters its row covers.
///
/// @implements REQ-GUIDE.sections_unfold
pub fn unfold(text: &str, open: &[String]) -> (String, Vec<(usize, usize)>) {
    let mut shown = String::new();
    let mut rows = Vec::new();
    let mut at = 0usize;
    // For each fold around the line: whether it is open.
    let mut around: Vec<bool> = Vec::new();
    let push = |line: &str, shown: &mut String, at: &mut usize| {
        shown.push_str(line);
        shown.push('\n');
        *at += line.chars().count() + 1;
    };
    for line in text.lines() {
        match line.strip_prefix(FOLD).map(str::trim) {
            Some("") => {
                around.pop();
            }
            Some(title) => {
                let unfolded = open.iter().any(|name| name == title);
                if around.iter().all(|held| *held) {
                    let indent = "  ".repeat(around.len());
                    let row = format!("{} {title}", if unfolded { OPEN } else { SHUT });
                    let start = at + indent.chars().count();
                    rows.push((start, start + row.chars().count()));
                    push(&format!("{indent}{row}"), &mut shown, &mut at);
                }
                around.push(unfolded);
            }
            None if !around.iter().all(|held| *held) => {}
            None => push(line, &mut shown, &mut at),
        }
    }
    (shown, rows)
}

/// Every fold the text declares, in order.
pub fn folds(text: &str) -> Vec<String> {
    text.lines()
        .filter_map(|line| line.strip_prefix(FOLD).map(str::trim))
        .filter(|title| !title.is_empty())
        .map(str::to_string)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    const TEXT: &str = "top\n+++ One\ninside one\n+++\nbetween\n+++ Two\ninside two\n+++\nend\n";

    /// @tests REQ-GUIDE.sections_unfold
    #[test]
    fn a_folded_section_is_one_row_and_an_unfolded_one_shows_its_lines() {
        let (shut, rows) = unfold(TEXT, &[]);
        assert_eq!(shut, "top\n▸ One\nbetween\n▸ Two\nend\n");
        let named: Vec<String> = rows.iter().map(|(a, b)| shut.chars().skip(*a).take(b - a).collect()).collect();
        assert_eq!(named, ["▸ One", "▸ Two"]);
        assert_eq!(title_of(&named[1]), "Two");

        let (open, rows) = unfold(TEXT, &["Two".into()]);
        assert_eq!(open, "top\n▸ One\nbetween\n▾ Two\ninside two\nend\n");
        let named: Vec<String> = rows.iter().map(|(a, b)| open.chars().skip(*a).take(b - a).collect()).collect();
        assert_eq!(named, ["▸ One", "▾ Two"]);
        assert_eq!(title_of(&named[1]), "Two");
        assert_eq!(folds(TEXT), ["One", "Two"]);
    }

    /// @tests REQ-GUIDE.sections_unfold
    #[test]
    fn a_fold_inside_a_fold_is_a_row_only_while_the_one_around_it_is_open() {
        let text = "top\n+++ Outer\nin outer\n+++ Inner\nin inner\n+++\nafter inner\n+++\nend\n";
        let (shut, rows) = unfold(text, &[]);
        assert_eq!(shut, "top\n▸ Outer\nend\n");
        assert_eq!(rows.len(), 1);
        let (outer, _) = unfold(text, &["Outer".into()]);
        assert_eq!(outer, "top\n▾ Outer\nin outer\n  ▸ Inner\nafter inner\nend\n");
        let (both, rows) = unfold(text, &["Outer".into(), "Inner".into()]);
        assert_eq!(both, "top\n▾ Outer\nin outer\n  ▾ Inner\nin inner\nafter inner\nend\n");
        let named: Vec<String> = rows.iter().map(|(a, b)| both.chars().skip(*a).take(b - a).collect()).collect();
        assert_eq!(named, ["▾ Outer", "▾ Inner"]);
        let (inner_only, _) = unfold(text, &["Inner".into()]);
        assert_eq!(inner_only, "top\n▸ Outer\nend\n", "an open fold inside a shut one showed");
    }

    /// @tests REQ-GUIDE.sections_unfold
    #[test]
    fn the_shipped_tutorial_folds_and_every_fold_closes() {
        let text = include_str!("../../../../assets/tutorial.md");
        let names = folds(text);
        assert!(names.len() >= 6, "only {} folds", names.len());
        let (shut, _) = unfold(text, &[]);
        let sections = shut.lines().filter(|line| line.starts_with(SHUT)).count();
        assert!(sections >= 9, "the major sections are not each a row when everything is folded: {shut}");
        assert!(shut.lines().count() < 60, "the tutorial is not folded at the start: {} lines", shut.lines().count());
        let ends = text.lines().filter(|line| line.trim() == FOLD).count();
        assert_eq!(ends, names.len(), "a fold is left open");
        let (shut, _) = unfold(text, &[]);
        let (open, rows) = unfold(text, &names);
        assert_eq!(rows.len(), names.len());
        assert!(open.lines().count() > shut.lines().count() + 40);
        assert!(!open.contains(FOLD) && !shut.contains(FOLD));
    }
}
