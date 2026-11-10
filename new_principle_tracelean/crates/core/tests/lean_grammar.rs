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
                && !line.strip_prefix("instance ").is_some_and(|rest| rest.starts_with([':', '(', '[', '{']));
            if declares && !scanned.declarations.iter().any(|d| d.start_line as usize <= n && n <= d.end_line as usize) {
                wrong.push(format!("{file}:{}: no declaration found for `{line}` (starts at {starts:?})", n + 1));
            }
        }
    }
    assert!(wrong.is_empty(), "{wrong:#?}");
}

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
    ("a named match subject", "def f (xs : List Nat) : Nat :=\n  match h : xs with\n  | [] => 0\n  | _ :: _ => 1\n"),
    ("bind", "def f (x : Option Nat) : Option Nat := x >>= fun n => some n\n"),
    ("a tactic declared and extended", "syntax \"step\" : tactic\nmacro_rules | `(tactic| step) => `(tactic| first | rfl | trivial)\nmacro \"steps\" : tactic => `(tactic| repeat step)\n"),
    ("braces in a string", "def s (x : Nat) : String := s!\"a {x} and {\"{\"}\"\n"),
    ("a nested block comment", "/-- shows /- this -/ inside -/\ndef f := 1\n"),
    ("commands quoted inside a body", "def q := `(def $x : Nat := 1\nnamespace $y)\n"),
    ("class inductive", "class inductive Ne (a : Sort u) : Prop where\n  | intro (v : a) : Ne a\n"),
    ("a constructor's doc comment at column 0", "inductive K\n/-- the a -/\n| a\n/-- the b -/\n| b\n"),
    ("an indented section", "section\n  variable (n : Nat)\n  def f := n\nend\n"),
    ("open … in inside a proof", "theorem t : True := by\n  open Classical in\n  trivial\n"),
    ("a width after #", "theorem t : x <<< 0#w\u{2082} = x := by simp\n"),
    ("a fun with typed binders", "def f := fun a b : Nat => a + b\n"),
    ("lets on lines of their own in a statement", "theorem t :\n    let a := 1\n    a = 1 := rfl\n"),
];

/// @tests REQ-ANNOT.totality
#[test]
fn each_construct_the_grammar_was_extended_for_parses() {
    for (name, text) in CONSTRUCTS {
        let found = unparsed(text);
        assert!(found.is_empty(), "{name} does not parse: {found:#?}");
    }
}

/// @tests REQ-CHECK.structural_rejects
#[test]
fn a_malformed_declaration_is_still_reported() {
    for text in ["def := 1\n", "/- never closed\ndef f := 1\n", "def q := `(def x\n"] {
        assert!(!unparsed(text).is_empty(), "{text:?} should not parse cleanly");
    }
}

fn lean_sources() -> std::path::PathBuf {
    let formal = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../formal");
    let toolchain = std::fs::read_to_string(formal.join("lean-toolchain")).unwrap();
    let elan = std::env::var_os("ELAN_HOME")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| Path::new(&std::env::var_os("HOME").unwrap()).join(".elan"));
    let dir = elan.join("toolchains").join(toolchain.trim().replace('/', "--").replace(':', "---")).join("src/lean");
    assert!(dir.is_dir(), "the toolchain's own sources are not at {}", dir.display());
    dir
}

fn files_under(dir: &Path, out: &mut Vec<std::path::PathBuf>) {
    for entry in std::fs::read_dir(dir).unwrap().flatten() {
        let path = entry.path();
        if path.is_dir() {
            files_under(&path, out);
        } else if path.extension().is_some_and(|e| e == "lean") {
            out.push(path);
        }
    }
}

/// For each byte, whether it is inside a syntax quotation `` `(…) ``, counting
/// parentheses outside comments, string literals and `'('` / `')'`.
fn quotations(text: &str, comments: &[(usize, usize)], literals: &[(usize, usize)]) -> Vec<bool> {
    let bytes = text.as_bytes();
    let skipped: Vec<(usize, usize)> = comments.iter().chain(literals).copied().collect();
    let mut inside = vec![false; bytes.len() + 1];
    let (mut depth, mut opened): (usize, Vec<usize>) = (0, Vec::new());
    let mut i = 0;
    while i < bytes.len() {
        if let Some((_, end)) = skipped.iter().find(|(s, e)| *s <= i && i < *e) {
            for b in inside.iter_mut().take(*end).skip(i) {
                *b = !opened.is_empty();
            }
            i = *end;
            continue;
        }
        inside[i] = !opened.is_empty();
        if bytes[i..].starts_with(b"'('") || bytes[i..].starts_with(b"')'") {
            i += 3;
            continue;
        }
        match bytes[i] {
            b'`' if bytes.get(i + 1) == Some(&b'(') => {
                opened.push(depth);
                depth += 1;
                i += 1;
            }
            b'(' => depth += 1,
            b')' => {
                depth = depth.saturating_sub(1);
                if opened.last() == Some(&depth) {
                    opened.pop();
                }
            }
            _ => {}
        }
        i += 1;
    }
    inside
}

/// Lines that start a named declaration, read from the text alone: after
/// attributes and modifiers, outside comments and string literals, and not
/// inside a syntax quotation (there they are data, not declarations).
fn declaring_lines(text: &str, comments: &[(usize, usize)], literals: &[(usize, usize)]) -> Vec<usize> {
    let modifiers = ["private", "protected", "noncomputable", "partial", "unsafe", "nonrec", "scoped", "local", "public", "meta"];
    let keywords = ["def", "theorem", "lemma", "abbrev", "instance", "structure", "class", "inductive"];
    let quoted = quotations(text, comments, literals);
    let mut out = Vec::new();
    let mut at = 0usize;
    for (n, line) in text.split_inclusive('\n').enumerate() {
        let start = at;
        at += line.len();
        let mut rest = line.trim_start();
        let mut offset = start + (line.len() - rest.len());
        if rest.starts_with("@[") {
            let Some(close) = rest.find(']') else { continue };
            let trimmed = rest[close + 1..].trim_start();
            offset += rest.len() - trimmed.len();
            rest = trimmed;
        }
        loop {
            let word = rest.split_whitespace().next().unwrap_or_default();
            if !modifiers.contains(&word) {
                break;
            }
            let trimmed = rest[word.len()..].trim_start();
            offset += rest.len() - trimmed.len();
            rest = trimmed;
        }
        let mut words = rest.split_whitespace();
        let (Some(keyword), Some(name)) = (words.next(), words.next()) else { continue };
        if !keywords.contains(&keyword) || (keyword == "instance" && [":", "where"].contains(&name)) || name.starts_with(['(', '[', '{', ':']) {
            continue;
        }
        let inside = |ranges: &[(usize, usize)]| ranges.iter().any(|(s, e)| *s <= offset && offset < *e);
        if !inside(comments) && !inside(literals) && !quoted[offset] {
            out.push(n);
        }
    }
    out
}

/// @tests REQ-ANNOT.totality
/// @tests REQ-ANCHOR.symbol_not_line
#[test]
#[ignore = "parses the Lean toolchain's own sources; run with --ignored"]
fn lean_s_own_sources_parse_and_every_declaration_is_found() {
    let mut files = Vec::new();
    files_under(&lean_sources(), &mut files);
    assert!(files.len() > 1000, "found only {} files", files.len());
    let mut wrong = Vec::new();
    let mut found = 0usize;
    for path in &files {
        let text = std::fs::read_to_string(path).unwrap();
        let scanned = scan(&text, Some(Lang::Lean4));
        if let Some(problem) = unparsed(&text).first() {
            wrong.push(format!("{}:{problem}", path.display()));
        }
        let starts: Vec<usize> = scanned.declarations.iter().map(|d| d.start_line as usize).collect();
        let expected = declaring_lines(&text, &scanned.comment_ranges, &scanned.literal_ranges);
        for line in &expected {
            if !starts.contains(line) {
                wrong.push(format!("{}:{}: no declaration found", path.display(), line + 1));
            }
        }
        for start in &starts {
            if !expected.contains(start) {
                wrong.push(format!("{}:{}: a declaration the text does not start", path.display(), start + 1));
            }
        }
        found += starts.len();
    }
    assert!(wrong.is_empty(), "{} problems in {} files:\n{:#?}", wrong.len(), files.len(), &wrong[..wrong.len().min(40)]);
    println!("{} files, {found} declarations, every one found", files.len());
}
