//! Requirements, identified by an immutable frontmatter `id` — never by filename
//! or directory. Any `.md` anywhere in the project carrying an `id:` is a
//! requirement; markdown without one is ordinary markdown and is ignored.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// Whether a requirement's children are claimed to exhaust it.
///
/// This is the denominator of every percentage the UI shows: with `Open` the
/// UI must render "≥ x%", never "x%", and the requirement can never display as
/// finished. Defaulting to `Open` means an author has to *claim* completeness
/// before a number is presented as exact.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum Decomposition {
    Complete,
    #[default]
    Open,
}

impl Decomposition {
    pub fn parse(s: &str) -> Self {
        match s.trim().to_ascii_lowercase().as_str() {
            "complete" => Decomposition::Complete,
            _ => Decomposition::Open,
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            Decomposition::Complete => "complete",
            Decomposition::Open => "open",
        }
    }
}

/// Workflow state. Orthogonal to evidence level: a requirement can be
/// `Approved` with no evidence at all, or `Draft` and fully conformant.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum ReqStatus {
    #[default]
    Draft,
    Approved,
    Linked,
}

impl ReqStatus {
    pub fn parse(s: &str) -> Self {
        match s.trim().to_ascii_lowercase().as_str() {
            "approved" => ReqStatus::Approved,
            "linked" => ReqStatus::Linked,
            _ => ReqStatus::Draft,
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            ReqStatus::Draft => "draft",
            ReqStatus::Approved => "approved",
            ReqStatus::Linked => "linked",
        }
    }
}

/// One requirement document.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Requirement {
    pub id: String,
    pub title: String,
    /// Path relative to the project root. Carries no meaning — identity is `id`.
    pub file: PathBuf,
    /// Parents in the refinement DAG (a requirement may refine several).
    pub refines: Vec<String>,
    pub decomposition: Decomposition,
    /// Clause key → clause text. Ordered so hashes and output are stable.
    pub clauses: BTreeMap<String, String>,
    pub status: ReqStatus,
    /// Markdown body after the frontmatter.
    pub body: String,
    /// Hash of the semantic content (clauses + body), used to detect that a
    /// requirement changed and its formalization link must be re-judged.
    pub content_hash: String,
}

impl Requirement {
    /// Clause keys, or a single synthetic `""` clause when the document
    /// declares none — so callers can treat "requirement-level" links
    /// uniformly with clause-level ones.
    pub fn clause_keys(&self) -> Vec<Option<String>> {
        if self.clauses.is_empty() {
            vec![None]
        } else {
            self.clauses.keys().map(|k| Some(k.clone())).collect()
        }
    }

    pub fn has_clause(&self, clause: &str) -> bool {
        self.clauses.contains_key(clause)
    }
}

/// A problem found while parsing a requirement document. Malformed frontmatter
/// is reported, never a panic and never a silent skip — a requirement that
/// fails to parse is far more dangerous than one that is missing.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FrontmatterProblem {
    pub file: PathBuf,
    pub line: u32,
    pub message: String,
}

/// Result of parsing one markdown file.
pub enum ParseOutcome {
    /// Has frontmatter with an `id`.
    Requirement(Box<Requirement>, Vec<FrontmatterProblem>),
    /// Ordinary markdown — no frontmatter, or frontmatter without `id`.
    NotARequirement,
}

/// Parse a markdown file into a requirement, if it declares an `id`.
///
/// The supported frontmatter subset is deliberately tiny — `key: value`,
/// `key: [a, b]`, and a one-level nested map under `clauses:` — so that no YAML
/// dependency is needed and the failure modes stay predictable.
pub fn parse_markdown(path: &Path, root: &Path, content: &str) -> ParseOutcome {
    let rel = path.strip_prefix(root).unwrap_or(path).to_path_buf();
    let mut problems = Vec::new();

    let Some((fm, body)) = split_frontmatter(content) else {
        // Documents predating frontmatter used a `# REQ-01: Title` heading and
        // a `Status:` line. That is a *document* format, not a filename
        // convention, so it costs nothing to keep reading it — and it keeps
        // existing projects working while they migrate.
        return parse_legacy(&rel, content);
    };

    let fields = parse_frontmatter(fm, &rel, &mut problems);

    let Some(id) = fields.scalars.get("id").cloned() else {
        return ParseOutcome::NotARequirement;
    };
    if id.is_empty() {
        problems.push(FrontmatterProblem {
            file: rel.clone(),
            line: 1,
            message: "`id:` is present but empty".into(),
        });
        return ParseOutcome::NotARequirement;
    }

    // Title: explicit field, else the first markdown heading, else the id.
    let title = fields
        .scalars
        .get("title")
        .cloned()
        .or_else(|| first_heading(body))
        .unwrap_or_else(|| id.clone());

    let refines = fields.lists.get("refines").cloned().unwrap_or_default();

    let decomposition = fields
        .scalars
        .get("decomposition")
        .map(|s| Decomposition::parse(s))
        .unwrap_or_default();

    let status = fields
        .scalars
        .get("status")
        .map(|s| ReqStatus::parse(s))
        .unwrap_or_default();

    let clauses = fields.maps.get("clauses").cloned().unwrap_or_default();

    let content_hash = super::hash::hash_requirement(&clauses, body);

    ParseOutcome::Requirement(
        Box::new(Requirement {
            id,
            title,
            file: rel,
            refines,
            decomposition,
            clauses,
            status,
            body: body.to_string(),
            content_hash,
        }),
        problems,
    )
}

/// Split `---\n…\n---\n` frontmatter from the body. Returns `None` when the
/// file does not open with a fence, which is the common case for ordinary
/// markdown and must stay cheap.
fn split_frontmatter(content: &str) -> Option<(&str, &str)> {
    let rest = content.strip_prefix("---\n").or_else(|| content.strip_prefix("---\r\n"))?;
    // Find the closing fence at the start of a line.
    let mut offset = 0usize;
    for line in rest.split_inclusive('\n') {
        let trimmed = line.trim_end_matches(['\r', '\n']);
        if trimmed.trim_end() == "---" {
            let fm = &rest[..offset];
            let body = &rest[offset + line.len()..];
            return Some((fm, body));
        }
        offset += line.len();
    }
    None
}

#[derive(Default)]
struct Fields {
    scalars: BTreeMap<String, String>,
    lists: BTreeMap<String, Vec<String>>,
    maps: BTreeMap<String, BTreeMap<String, String>>,
}

fn parse_frontmatter(fm: &str, file: &Path, problems: &mut Vec<FrontmatterProblem>) -> Fields {
    let mut fields = Fields::default();
    // Key of the nested map currently being filled, if any.
    let mut current_map: Option<String> = None;

    for (idx, raw) in fm.lines().enumerate() {
        let line_no = idx as u32 + 2; // +1 for the opening fence, +1 for 1-indexing
        let line = raw.trim_end();
        if line.trim().is_empty() || line.trim_start().starts_with('#') {
            continue;
        }

        let indented = line.starts_with(' ') || line.starts_with('\t');

        if indented {
            let Some(map_key) = current_map.clone() else {
                problems.push(FrontmatterProblem {
                    file: file.to_path_buf(),
                    line: line_no,
                    message: format!("indented line does not belong to any key: {:?}", line.trim()),
                });
                continue;
            };
            match split_key_value(line.trim()) {
                Some((k, v)) => {
                    fields
                        .maps
                        .entry(map_key)
                        .or_default()
                        .insert(k, unquote(v).to_string());
                }
                None => {
                    // A `- item` list entry under a key: treat as a list.
                    if let Some(item) = line.trim().strip_prefix("- ") {
                        fields
                            .lists
                            .entry(map_key)
                            .or_default()
                            .push(unquote(item.trim()).to_string());
                    } else {
                        problems.push(FrontmatterProblem {
                            file: file.to_path_buf(),
                            line: line_no,
                            message: format!("cannot parse nested entry: {:?}", line.trim()),
                        });
                    }
                }
            }
            continue;
        }

        current_map = None;
        let Some((key, value)) = split_key_value(line) else {
            problems.push(FrontmatterProblem {
                file: file.to_path_buf(),
                line: line_no,
                message: format!("expected `key: value`, got {:?}", line),
            });
            continue;
        };

        let value = value.trim();
        if value.is_empty() {
            // Opens a nested block (`clauses:` / `refines:` on their own line).
            current_map = Some(key);
            continue;
        }

        if let Some(inner) = value.strip_prefix('[').and_then(|v| v.strip_suffix(']')) {
            let items: Vec<String> = inner
                .split(',')
                .map(|s| unquote(s.trim()).to_string())
                .filter(|s| !s.is_empty())
                .collect();
            fields.lists.insert(key, items);
        } else {
            fields.scalars.insert(key, unquote(value).to_string());
        }
    }

    fields
}

/// Split on the first `:` that is not inside quotes.
fn split_key_value(line: &str) -> Option<(String, &str)> {
    let mut in_quote: Option<char> = None;
    for (i, ch) in line.char_indices() {
        match (in_quote, ch) {
            (Some(q), c) if c == q => in_quote = None,
            (None, '"') | (None, '\'') => in_quote = Some(ch),
            (None, ':') => {
                let key = line[..i].trim();
                if key.is_empty() || key.contains(char::is_whitespace) {
                    return None;
                }
                return Some((key.to_string(), &line[i + 1..]));
            }
            _ => {}
        }
    }
    None
}

fn unquote(s: &str) -> &str {
    let s = s.trim();
    for q in ['"', '\''] {
        if s.len() >= 2 && s.starts_with(q) && s.ends_with(q) {
            return &s[1..s.len() - 1];
        }
    }
    s
}

fn first_heading(body: &str) -> Option<String> {
    body.lines()
        .find_map(|l| l.trim().strip_prefix("# ").map(|h| h.trim().to_string()))
        .filter(|h| !h.is_empty())
}

/// The pre-frontmatter document format: `# REQ-01: Title` plus an optional
/// `Status:` line. Produces a requirement with no clauses and an open
/// decomposition, since neither can be expressed in that format.
fn parse_legacy(rel: &Path, content: &str) -> ParseOutcome {
    let heading = content
        .lines()
        .find_map(|l| l.trim().strip_prefix("# ").map(|h| h.to_string()));
    let Some(heading) = heading else { return ParseOutcome::NotARequirement };
    let Some((id, title)) = heading.split_once(':') else { return ParseOutcome::NotARequirement };

    let id = id.trim().to_string();
    // Only ids that look like requirement ids, so an ordinary `# Note: this`
    // heading does not become a requirement.
    if id.is_empty() || !id.starts_with(|c: char| c.is_ascii_uppercase()) || !id.contains('-') {
        return ParseOutcome::NotARequirement;
    }

    let status = content
        .lines()
        .find_map(|l| l.trim().strip_prefix("Status:"))
        .map(ReqStatus::parse)
        .unwrap_or_default();

    let body = content
        .lines()
        .skip_while(|l| !l.trim().starts_with("Status:"))
        .skip(1)
        .collect::<Vec<_>>()
        .join("\n")
        .trim()
        .to_string();
    let body = if body.is_empty() { content.to_string() } else { body };

    let clauses = BTreeMap::new();
    let content_hash = super::hash::hash_requirement(&clauses, &body);

    ParseOutcome::Requirement(
        Box::new(Requirement {
            id,
            title: title.trim().to_string(),
            file: rel.to_path_buf(),
            refines: Vec::new(),
            decomposition: Decomposition::Open,
            clauses,
            status,
            body,
            content_hash,
        }),
        Vec::new(),
    )
}
