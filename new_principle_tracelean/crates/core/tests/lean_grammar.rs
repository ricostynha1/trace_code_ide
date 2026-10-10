//! The vendored Lean grammar reads the Lean this project writes
//! (`vendor/tree-sitter-lean4`, action plan §8).
//!
//! A region the grammar cannot parse anchors to the whole file and caps every
//! annotation after it at L1 (ADR-0008), so a grammar change that loses a
//! construct costs evidence silently. Two checks guard it: every file of
//! `formal/` parses cleanly and loses no declaration, and each construct the
//! extension added parses on its own — and a malformed one does not.

use std::path::Path;

use tracelean_core::trace::anchor::{scan, Lang};

fn lean_files() -> Vec<(String, String)> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap().parent().unwrap().join("formal");
    let mut out = Vec::new();
    let mut stack = vec![root.clone()];
    while let Some(dir) = stack.pop() {
        for entry in std::fs::read_dir(&dir).unwrap().flatten() {
            let path = entry.path();
            if path.is_dir() {
                if !path.file_name().is_some_and(|n| n == ".lake" || n == "build") {
                    stack.push(path);
                }
            } else if path.extension().is_some_and(|e| e == "lean")
                // Lake's own DSL, not the Lean the models are written in.
                && !path.file_name().is_some_and(|n| n == "lakefile.lean")
            {
                let rel = path.strip_prefix(&root).unwrap().display().to_string();
                out.push((rel, std::fs::read_to_string(&path).unwrap()));
            }
        }
    }
    assert!(out.len() > 20, "found only {} Lean files", out.len());
    out
}

/// The stored parser is the one `grammar.js` generates: the build uses a stale
/// one with only a warning, so that an editor's background check never starts
/// a generation, and this is where a stale one fails.
#[test]
fn the_stored_parser_is_generated_from_the_grammar() {
    let vendor = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../vendor/tree-sitter-lean4");
    let grammar = std::fs::read(vendor.join("grammar.js")).unwrap();
    let mut hash: u64 = 0xcbf29ce484222325;
    for byte in grammar {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x00000100000001B3);
    }
    let stored = std::fs::read_to_string(vendor.join("src/grammar.hash")).unwrap();
    assert_eq!(
        stored.trim(),
        format!("{hash:016x}"),
        "src/parser.c.gz is older than grammar.js: build with TRACELEAN_GENERATE_LEAN_PARSER=1 and store it (vendor/tree-sitter-lean4/README.md)"
    );
}

/// Where a file does not parse, as `line: text`.
fn unparsed(text: &str) -> Vec<String> {
    let scanned = scan(text, Some(Lang::Lean4));
    let mut found: Vec<String> = scanned
        .error_ranges
        .iter()
        .map(|(start, end)| format!("{}: {}", text[..*start].lines().count(), text[*start..*end].lines().next().unwrap_or_default()))
        .collect();
    found.extend(
        scanned.declarations.iter().filter(|d| !d.precise).map(|d| format!("{}: declaration `{}`", d.start_line + 1, d.symbol_path)),
    );
    found
}

/// Every file of this project's own Lean parses, and every declaration that
/// starts a line is found.
///
/// @tests REQ-ANNOT.totality
#[test]
fn every_file_of_formal_parses_and_loses_no_declaration() {
    let keywords = ["def ", "theorem ", "structure ", "inductive ", "abbrev ", "instance ", "lemma "];
    let mut wrong = Vec::new();
    for (file, text) in lean_files() {
        let found = unparsed(&text);
        if !found.is_empty() {
            wrong.push(format!("{file} has regions the grammar cannot parse: {found:?}"));
        }
        let scanned = scan(&text, Some(Lang::Lean4));
        let starts: Vec<u32> = scanned.declarations.iter().map(|d| d.start_line).collect();
        for (n, line) in text.lines().enumerate() {
            let declares = keywords.iter().any(|k| line.starts_with(k) || line.starts_with(&format!("private {k}")))
                // An instance with no name has nothing to be addressed by.
                && !line.strip_prefix("instance ").is_some_and(|rest| rest.starts_with([':', '(', '[', '{']));
            // A declaration's start may be its doc comment above the line.
            if declares && !scanned.declarations.iter().any(|d| d.start_line as usize <= n && n <= d.end_line as usize) {
                wrong.push(format!("{file}:{}: no declaration found for `{line}` (starts at {starts:?})", n + 1));
            }
        }
    }
    assert!(wrong.is_empty(), "{wrong:#?}");
}

/// Constructs the extension reads, each alone.
const CONSTRUCTS: &[(&str, &str)] = &[
    ("by_cases", "theorem t (p : Prop) : p ∨ ¬p := by\n  by_cases h : p\n  · exact Or.inl h\n  · exact Or.inr h\n"),
    ("obtain with a pattern", "theorem t (h : ∃ x : Nat, x = 1) : True := by\n  obtain ⟨x, hx⟩ := h\n  trivial\n"),
    ("match in a tactic", "theorem t (n : Nat) : n = n := by\n  match n with\n  | 0 => rfl\n  | k + 1 => rfl\n"),
    ("cases with alternatives", "theorem t (xs : List Nat) : xs = xs := by\n  cases xs with\n  | nil => rfl\n  | cons a r => rfl\n"),
    ("induction with alternatives", "theorem t (xs : List Nat) : xs = xs := by\n  induction xs with\n  | nil => rfl\n  | cons a r ih => rfl\n"),
    ("rcases with alternatives", "theorem t (a : Nat) : a = a := by\n  rcases a with _ | a\n  · rfl\n  · rfl\n"),
    ("first with alternatives", "theorem t : True := by\n  first | trivial | rfl\n"),
    ("the sum type", "def d (x : Nat) : Nat ⊕ Nat := .inl x\n"),
    ("bitwise and", "def c (x : Nat) : Nat := x &&& 3\n"),
    ("an escaped field name", "structure S where\n  «end» : Nat\n"),
    ("mutual definitions", "mutual\ndef a : Nat → Nat\n  | 0 => 0\n  | n + 1 => b n\ndef b : Nat → Nat\n  | 0 => 0\n  | n + 1 => a n\nend\n"),
    ("a where clause on a definition", "def f (n : Nat) : Nat := go n\nwhere\n  go (m : Nat) : Nat := m\n"),
    ("termination_by", "def f (n : Nat) : Nat := n\ntermination_by n\n"),
    ("decreasing_by", "def f (n : Nat) : Nat := n\ndecreasing_by simp_wf; omega\n"),
    ("lets on lines of their own after else", "def f (c : Bool) : Nat :=\n  if c then 0\n  else\n    let a := 1\n    let (b, d) := (2, 3)\n    a + b + d\n"),
    ("else at the indent of the block", "def f (c : Bool) : Nat :=\n  let g := fun (x : Nat) =>\n    if c then x\n    else x + 1\n  g 0\n"),
    ("a location after simp", "theorem t (h : 0 = 0) : True := by\n  simp only [Nat.add_zero] at h ⊢\n  cases h\n  case refl => trivial\n"),
    ("let rec", "def f (n : Nat) : Nat :=\n  let rec go (m : Nat) : Nat := m\n  go n\n"),
];

/// @tests REQ-ANNOT.totality
#[test]
fn each_construct_the_grammar_was_extended_for_parses() {
    for (name, text) in CONSTRUCTS {
        let found = unparsed(text);
        assert!(found.is_empty(), "{name} does not parse: {found:#?}");
    }
}

/// An extension that accepts everything would pass the test above, so what is
/// broken stays broken: an unfinished declaration is reported.
///
/// @tests REQ-CHECK.structural_rejects
#[test]
fn a_malformed_declaration_is_still_reported() {
    for text in ["def f (x : Nat : Nat := \n", "theorem t : True := by\n  first |\n  )))\n"] {
        assert!(!unparsed(text).is_empty(), "{text:?} should not parse cleanly");
    }
}
