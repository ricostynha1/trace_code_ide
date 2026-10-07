//! `WorkspaceEdit` → invertible `Command`s.
//!
//! This is the load-bearing half of the LSP integration, and the reason it is
//! not optional: TraceLean's claim is that the undo tree is *one* history of
//! everything. An LSP rename that wrote to disk behind the command stream would
//! be the one edit you cannot undo — and, worse, the one edit that silently
//! breaks traceability, because T1's anchors follow a rename only if the rename
//! is a command the trace layer can see.
//!
//! Two things this module refuses to do quietly:
//!
//! - **Edit a document it cannot read.** Lowering needs the buffer to compute
//!   the `old` witness every `Replace` carries. If the buffer is unavailable the
//!   whole lowering fails rather than dropping that file's edits, because a
//!   partially-applied rename is worse than a refused one.
//! - **Guess a position encoding.** LSP positions are UTF-16 code units unless
//!   the server negotiated otherwise, and a wrong encoding corrupts exactly the
//!   files with non-ASCII content — the failure nobody notices in testing.

use std::path::{Path, PathBuf};

use serde_json::Value;

use crate::commands::Command;

/// LSP position encodings. UTF-16 is the protocol default and what servers use
/// when nothing was negotiated.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PositionEncoding {
    Utf16,
    Utf8,
    Utf32,
}

impl Default for PositionEncoding {
    fn default() -> Self {
        PositionEncoding::Utf16
    }
}

impl PositionEncoding {
    pub fn parse(s: &str) -> PositionEncoding {
        match s {
            "utf-8" => PositionEncoding::Utf8,
            "utf-32" => PositionEncoding::Utf32,
            _ => PositionEncoding::Utf16,
        }
    }

    fn units(self, c: char) -> usize {
        match self {
            PositionEncoding::Utf16 => c.len_utf16(),
            PositionEncoding::Utf8 => c.len_utf8(),
            PositionEncoding::Utf32 => 1,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum LowerError {
    /// The buffer for a document in the edit is not loaded.
    UnknownDocument(PathBuf),
    /// A position points past the end of the document.
    PositionOutOfRange { file: PathBuf, line: u32, character: u32 },
    /// A `documentChanges` entry this client does not implement.
    Unsupported(String),
    Malformed(String),
}

impl std::fmt::Display for LowerError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LowerError::UnknownDocument(p) => write!(
                f,
                "the edit touches `{}`, which is not open — open it and retry",
                p.display()
            ),
            LowerError::PositionOutOfRange { file, line, character } => write!(
                f,
                "{}:{}:{} is past the end of the buffer (the server's view is stale)",
                file.display(),
                line,
                character
            ),
            LowerError::Unsupported(what) => write!(f, "unsupported workspace edit: {}", what),
            LowerError::Malformed(what) => write!(f, "malformed workspace edit: {}", what),
        }
    }
}

impl std::error::Error for LowerError {}

/// (line, character) in `encoding` → char offset into `content`.
///
/// A `character` past the end of its line clamps to the line end — that is the
/// spec's own rule, not a guess ("if the character value is greater than the
/// line length it defaults back to the line length"). A `line` past the end of
/// the document returns `None`, because that means the server's view of the
/// file is stale and applying the edit anywhere would be wrong.
pub fn char_offset(
    content: &str,
    line: u32,
    character: u32,
    encoding: PositionEncoding,
) -> Option<usize> {
    let mut chars = 0usize;
    let mut current_line = 0u32;
    let mut iter = content.chars().peekable();

    while current_line < line {
        let c = iter.next()?;
        chars += 1;
        if c == '\n' {
            current_line += 1;
        }
    }

    let mut units = 0u32;
    while units < character {
        match iter.peek() {
            Some('\n') | None => break, // clamp to the line end
            Some(&c) => {
                units += encoding.units(c) as u32;
                chars += 1;
                iter.next();
            }
        }
    }
    Some(chars)
}

/// What the lowering needs from the editor: the current text of a document.
pub type DocumentReader<'a> = dyn Fn(&Path) -> Option<String> + 'a;

/// Lower a `WorkspaceEdit` into a single atomic `Command::Batch`.
///
/// One batch, not a sequence: a rename that half-applied is not a state the
/// undo tree should be able to represent.
pub fn lower_workspace_edit(
    edit: &Value,
    root: &Path,
    encoding: PositionEncoding,
    read: &DocumentReader<'_>,
) -> Result<Command, LowerError> {
    let mut commands: Vec<Command> = Vec::new();

    if let Some(changes) = edit.get("documentChanges").and_then(|v| v.as_array()) {
        for change in changes {
            lower_document_change(change, root, encoding, read, &mut commands)?;
        }
    } else if let Some(changes) = edit.get("changes").and_then(|v| v.as_object()) {
        // `changes` is a map, so its iteration order is not the server's
        // order. Sort by URI to make the resulting command list deterministic —
        // the edits are independent by spec, so any fixed order is correct and
        // a reproducible one is testable.
        let mut uris: Vec<&String> = changes.keys().collect();
        uris.sort();
        for uri in uris {
            let file = uri_to_relative(uri, root)?;
            let edits = changes[uri]
                .as_array()
                .ok_or_else(|| LowerError::Malformed(format!("changes[{}] is not an array", uri)))?;
            lower_text_edits(&file, edits, encoding, read, &mut commands)?;
        }
    } else {
        return Err(LowerError::Malformed(
            "neither `changes` nor `documentChanges` present".to_string(),
        ));
    }

    Ok(Command::Batch { commands })
}

fn lower_document_change(
    change: &Value,
    root: &Path,
    encoding: PositionEncoding,
    read: &DocumentReader<'_>,
    out: &mut Vec<Command>,
) -> Result<(), LowerError> {
    match change.get("kind").and_then(|v| v.as_str()) {
        Some("rename") => {
            let old = change
                .get("oldUri")
                .and_then(|v| v.as_str())
                .ok_or_else(|| LowerError::Malformed("rename without oldUri".into()))?;
            let new = change
                .get("newUri")
                .and_then(|v| v.as_str())
                .ok_or_else(|| LowerError::Malformed("rename without newUri".into()))?;
            out.push(Command::RenameFile {
                from: uri_to_relative(old, root)?,
                to: uri_to_relative(new, root)?,
            });
            Ok(())
        }
        Some("create") => {
            let uri = change
                .get("uri")
                .and_then(|v| v.as_str())
                .ok_or_else(|| LowerError::Malformed("create without uri".into()))?;
            out.push(Command::CreateFile { path: uri_to_relative(uri, root)? });
            Ok(())
        }
        Some("delete") => {
            let uri = change
                .get("uri")
                .and_then(|v| v.as_str())
                .ok_or_else(|| LowerError::Malformed("delete without uri".into()))?;
            let path = uri_to_relative(uri, root)?;
            // `DeleteFile` carries the content as its inverse witness, so the
            // document has to be readable even though the edit only removes it.
            let content = read(&path).ok_or_else(|| LowerError::UnknownDocument(path.clone()))?;
            out.push(Command::DeleteFile { path, content });
            Ok(())
        }
        Some(other) => Err(LowerError::Unsupported(other.to_string())),
        None => {
            // No `kind` means a TextDocumentEdit.
            let uri = change
                .get("textDocument")
                .and_then(|v| v.get("uri"))
                .and_then(|v| v.as_str())
                .ok_or_else(|| LowerError::Malformed("text document edit without uri".into()))?;
            let file = uri_to_relative(uri, root)?;
            let edits = change
                .get("edits")
                .and_then(|v| v.as_array())
                .ok_or_else(|| LowerError::Malformed("text document edit without edits".into()))?;
            lower_text_edits(&file, edits, encoding, read, out)
        }
    }
}

fn lower_text_edits(
    file: &Path,
    edits: &[Value],
    encoding: PositionEncoding,
    read: &DocumentReader<'_>,
    out: &mut Vec<Command>,
) -> Result<(), LowerError> {
    if edits.is_empty() {
        return Ok(());
    }
    let content = read(file).ok_or_else(|| LowerError::UnknownDocument(file.to_path_buf()))?;
    let chars: Vec<char> = content.chars().collect();

    // Resolve every edit to char offsets first.
    struct Resolved {
        start: usize,
        end: usize,
        new_text: String,
    }
    let mut resolved: Vec<Resolved> = Vec::new();
    for e in edits {
        // `AnnotatedTextEdit` is a `TextEdit` plus an id; the extra field is
        // metadata for a confirmation UI we do not have, so it lowers the same.
        let range = e
            .get("range")
            .ok_or_else(|| LowerError::Malformed("text edit without range".into()))?;
        let (sl, sc) = position_of(range, "start")?;
        let (el, ec) = position_of(range, "end")?;
        let start = char_offset(&content, sl, sc, encoding).ok_or(LowerError::PositionOutOfRange {
            file: file.to_path_buf(),
            line: sl,
            character: sc,
        })?;
        let end = char_offset(&content, el, ec, encoding).ok_or(LowerError::PositionOutOfRange {
            file: file.to_path_buf(),
            line: el,
            character: ec,
        })?;
        if end < start {
            return Err(LowerError::Malformed("text edit range ends before it starts".into()));
        }
        let new_text = e
            .get("newText")
            .and_then(|v| v.as_str())
            .ok_or_else(|| LowerError::Malformed("text edit without newText".into()))?
            .to_string();
        resolved.push(Resolved { start, end, new_text });
    }

    // LSP says the edits in one array apply as if simultaneously. `Replace`
    // commands apply in sequence against absolute offsets, so emitting them
    // last-to-first keeps every remaining offset valid without rewriting any.
    resolved.sort_by(|a, b| b.start.cmp(&a.start).then(b.end.cmp(&a.end)));

    for r in resolved {
        let old: String = chars[r.start..r.end.min(chars.len())].iter().collect();
        out.push(Command::Replace {
            file: file.to_path_buf(),
            at: r.start,
            old,
            new: r.new_text,
        });
    }
    Ok(())
}

fn position_of(range: &Value, which: &str) -> Result<(u32, u32), LowerError> {
    let p = range
        .get(which)
        .ok_or_else(|| LowerError::Malformed(format!("range without `{}`", which)))?;
    let line = p
        .get("line")
        .and_then(|v| v.as_u64())
        .ok_or_else(|| LowerError::Malformed(format!("{} without line", which)))? as u32;
    let character = p
        .get("character")
        .and_then(|v| v.as_u64())
        .ok_or_else(|| LowerError::Malformed(format!("{} without character", which)))?
        as u32;
    Ok((line, character))
}

/// A `file:` URI → a path relative to the project root, which is the form every
/// `Command` uses. A path outside the root stays absolute rather than growing
/// `../..` segments that no buffer key would match.
pub fn uri_to_relative(uri: &str, root: &Path) -> Result<PathBuf, LowerError> {
    let abs = super::transport::uri_to_path(uri)
        .ok_or_else(|| LowerError::Malformed(format!("not a file URI: {}", uri)))?;
    Ok(abs.strip_prefix(root).map(|p| p.to_path_buf()).unwrap_or(abs))
}
