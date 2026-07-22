//! Embeddings index — local semantic search over project files using fastembed.
//!
//! Build: Chunk source files → embed each chunk → store in-memory.
//! Query: Embed query → cosine similarity → return top-K chunks.
//! Rebuild per session (explicit rebuild via UI button).

use fastembed::{EmbeddingModel, InitOptions, TextEmbedding};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

/// A chunk of code with its embedding vector.
#[derive(Clone)]
struct Chunk {
    /// Relative path to file.
    file: PathBuf,
    /// 0-indexed start line of the chunk in the file.
    start_line: usize,
    /// 0-indexed end line (exclusive).
    end_line: usize,
    /// Raw text content of the chunk.
    text: String,
    /// Embedding vector.
    vector: Vec<f32>,
}

/// Search result from the embeddings index.
#[derive(Debug, Clone)]
pub struct EmbedResult {
    pub file: PathBuf,
    /// Tight 3-line window's range (0-indexed, end exclusive) — the range
    /// `snippet` actually covers, found by re-scoring sub-windows of the
    /// matched chunk against the query (see `sub_windows`).
    pub start_line: usize,
    pub end_line: usize,
    /// The full ~30-line chunk's range the window was drawn from, so the
    /// model can see (or `read_file`) the surrounding context.
    pub chunk_start_line: usize,
    pub chunk_end_line: usize,
    pub snippet: String,
    pub score: f32,
}

/// In-memory embeddings index for a project.
pub struct EmbeddingsIndex {
    chunks: Vec<Chunk>,
    model: TextEmbedding,
}

/// Thread-safe shared index.
pub type SharedIndex = Arc<Mutex<Option<EmbeddingsIndex>>>;

/// Create a new empty shared index.
pub fn new_shared_index() -> SharedIndex {
    Arc::new(Mutex::new(None))
}

/// Build the index for `project_root` on a background thread and store it into
/// `index` when done. Building is local and free (fastembed AllMiniLML6V2,
/// model cached after first download), so it is safe to auto-run on project
/// open without spending any API budget. Best-effort: a build failure just
/// leaves the index empty and `find_semantic` reports it isn't ready yet.
pub fn spawn_build(index: &SharedIndex, project_root: PathBuf) {
    let index = Arc::clone(index);
    std::thread::spawn(move || match EmbeddingsIndex::build(&project_root) {
        Some(built) => {
            if let Ok(mut guard) = index.lock() {
                *guard = Some(built);
            }
        }
        None => {
            eprintln!(
                "[embeddings] index build failed for {} (model load error) — \
                 semantic search will be unavailable this session",
                project_root.display()
            );
        }
    });
}

/// Configuration for index building.
const CHUNK_LINES: usize = 30;
const CHUNK_OVERLAP: usize = 5;
const BATCH_SIZE: usize = 64;
const MAX_FILE_LINES: usize = 10_000;

/// Cosine-similarity floor below which a chunk is considered noise, not a
/// real match. Without this, a query against a small/narrow project pads out
/// to `max_results` with near-zero-relevance chunks just because they exist —
/// "no good matches" was never a possible outcome. 0.1 was picked by eyeballing
/// real query output; a chunk about unrelated code (auth, upload validation)
/// scored 0.017–0.07 against a "royal family" query, so 0.1 comfortably
/// excludes noise without needing per-query tuning.
const MIN_RELEVANCE_SCORE: f32 = 0.1;

/// Width of the sub-window re-embedded within each shortlisted chunk to find
/// the tightest matching snippet (see `sub_windows`, `query`'s second pass).
const SUB_WINDOW_LINES: usize = 3;

/// File extensions to index.
const INDEXABLE_EXTS: &[&str] = &[
    "rs", "py", "js", "ts", "tsx", "jsx", "c", "cpp", "h", "hpp",
    "go", "java", "rb", "lean", "md", "toml", "yaml", "yml", "json",
    "css", "html", "sh", "sql",
];

impl EmbeddingsIndex {
    /// Build index from project root. Blocks until done.
    /// Returns None if model fails to load.
    pub fn build(project_root: &Path) -> Option<Self> {
        // Init model (downloads on first use, cached afterward)
        let mut model = TextEmbedding::try_new(
            InitOptions::new(EmbeddingModel::AllMiniLML6V2)
                .with_show_download_progress(false),
        ).ok()?;

        // Collect file chunks
        let mut texts: Vec<String> = Vec::new();
        let mut chunk_meta: Vec<(PathBuf, usize, usize)> = Vec::new(); // (file, start, end)

        collect_files(project_root, project_root, &mut texts, &mut chunk_meta, 0);

        if texts.is_empty() {
            return Some(Self { chunks: Vec::new(), model });
        }

        // Embed all chunks in batches
        let all_embeddings = model.embed(texts.clone(), Some(BATCH_SIZE)).ok()?;

        // Assemble chunks
        let chunks: Vec<Chunk> = texts.into_iter()
            .zip(chunk_meta.into_iter())
            .zip(all_embeddings.into_iter())
            .map(|((text, (file, start, end)), vector)| {
                Chunk { file, start_line: start, end_line: end, text, vector }
            })
            .collect();

        Some(Self { chunks, model })
    }

    /// Query the index. Returns top-K results sorted by relevance.
    ///
    /// Two passes: first rank whole ~30-line chunks (cheap, one vector per
    /// chunk, already computed at build time) to shortlist `max_results` of
    /// them; then re-embed only the shortlisted chunks' 3-line sliding
    /// windows against the query and keep each chunk's single
    /// best-matching window as the displayed snippet. This is query-time
    /// (not index-build-time) sub-embedding: it re-embeds ~10 windows ×
    /// max_results chunks per query (a few hundred tiny embeddings, fast),
    /// rather than sub-embedding every chunk in the whole project up front.
    pub fn query(&mut self, query: &str, max_results: usize, path_filter: &str, file_filter: &str) -> Vec<EmbedResult> {
        if self.chunks.is_empty() {
            return Vec::new();
        }

        // Embed query
        let query_vecs = match self.model.embed(vec![query.to_string()], None) {
            Ok(v) => v,
            Err(_) => return Vec::new(),
        };
        let query_vec = query_vecs[0].clone();

        // Score all chunks
        let mut scored: Vec<(f32, usize)> = self.chunks.iter().enumerate()
            .filter(|(_, chunk)| {
                // Path filter
                if !path_filter.is_empty() {
                    let chunk_path = chunk.file.to_string_lossy();
                    if !chunk_path.starts_with(path_filter) {
                        return false;
                    }
                }
                // File extension filter
                if !file_filter.is_empty() {
                    if let Some(ext) = file_filter.strip_prefix("*.") {
                        let chunk_ext = chunk.file.extension()
                            .map(|e| e.to_string_lossy().to_string())
                            .unwrap_or_default();
                        if chunk_ext != ext {
                            return false;
                        }
                    }
                }
                true
            })
            .map(|(i, chunk)| (cosine_similarity(&query_vec, &chunk.vector), i))
            .collect();

        // Drop noise below the relevance floor before ranking — otherwise a
        // narrow/small project pads out to max_results with irrelevant chunks.
        scored.retain(|(score, _)| *score >= MIN_RELEVANCE_SCORE);

        // Sort descending by score
        scored.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
        scored.truncate(max_results);

        if scored.is_empty() {
            return Vec::new();
        }

        // Second pass: re-embed each shortlisted chunk's 3-line sliding
        // windows and score them against the (already-computed) query
        // vector, so the snippet shown is the tightest matching group of
        // lines rather than the whole chunk. `content_lines` skips the
        // `// path` prefix line every chunk's text carries (see
        // `collect_files`) so window line numbers line up with the file.
        let mut window_texts: Vec<String> = Vec::new();
        let mut window_meta: Vec<(usize, usize, usize)> = Vec::new(); // (scored_idx, w_start, w_end)

        for (scored_idx, (_, chunk_idx)) in scored.iter().enumerate() {
            let chunk = &self.chunks[*chunk_idx];
            let content_lines: Vec<&str> = chunk.text.lines().skip(1).collect();
            for (w_start, w_end) in sub_windows(content_lines.len()) {
                window_texts.push(content_lines[w_start..w_end].join("\n"));
                window_meta.push((scored_idx, w_start, w_end));
            }
        }

        let window_vecs = if window_texts.is_empty() {
            Vec::new()
        } else {
            self.model.embed(window_texts, Some(BATCH_SIZE)).unwrap_or_default()
        };

        // Best (score, w_start, w_end) per shortlisted chunk.
        let mut best_window: Vec<Option<(f32, usize, usize)>> = vec![None; scored.len()];
        for ((scored_idx, w_start, w_end), vec) in window_meta.into_iter().zip(window_vecs.into_iter()) {
            let s = cosine_similarity(&query_vec, &vec);
            let better = best_window[scored_idx].map(|(best, _, _)| s > best).unwrap_or(true);
            if better {
                best_window[scored_idx] = Some((s, w_start, w_end));
            }
        }

        scored.into_iter().enumerate().map(|(scored_idx, (chunk_score, idx))| {
            let chunk = &self.chunks[idx];
            let content_lines: Vec<&str> = chunk.text.lines().skip(1).collect();
            let (window_score, w_start, w_end) = best_window[scored_idx]
                .unwrap_or((chunk_score, 0, content_lines.len()));
            let snippet = content_lines[w_start..w_end].join("\n");
            EmbedResult {
                file: chunk.file.clone(),
                start_line: chunk.start_line + w_start,
                end_line: chunk.start_line + w_end,
                chunk_start_line: chunk.start_line,
                chunk_end_line: chunk.end_line,
                snippet,
                score: window_score,
            }
        }).collect()
    }

    /// Number of indexed chunks.
    pub fn chunk_count(&self) -> usize {
        self.chunks.len()
    }
}

/// 3-line sliding windows (stride 1) over a chunk of `content_len` lines,
/// as (start, end) pairs (0-indexed, end exclusive, relative to the
/// chunk's own content). A chunk shorter than the window is returned whole
/// as a single window rather than yielding nothing.
fn sub_windows(content_len: usize) -> Vec<(usize, usize)> {
    if content_len == 0 {
        return Vec::new();
    }
    if content_len <= SUB_WINDOW_LINES {
        return vec![(0, content_len)];
    }
    (0..=(content_len - SUB_WINDOW_LINES))
        .map(|s| (s, s + SUB_WINDOW_LINES))
        .collect()
}

/// Cosine similarity between two vectors.
fn cosine_similarity(a: &[f32], b: &[f32]) -> f32 {
    let dot: f32 = a.iter().zip(b.iter()).map(|(x, y)| x * y).sum();
    let norm_a: f32 = a.iter().map(|x| x * x).sum::<f32>().sqrt();
    let norm_b: f32 = b.iter().map(|x| x * x).sum::<f32>().sqrt();
    if norm_a == 0.0 || norm_b == 0.0 { return 0.0; }
    dot / (norm_a * norm_b)
}

/// Recursively collect and chunk project files.
fn collect_files(
    dir: &Path,
    root: &Path,
    texts: &mut Vec<String>,
    meta: &mut Vec<(PathBuf, usize, usize)>,
    depth: usize,
) {
    if depth > 10 { return; }
    let Ok(entries) = std::fs::read_dir(dir) else { return; };

    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().to_string();
        if name.starts_with('.') || name == "node_modules" || name == "target" || name == "dist" || name == "build" {
            continue;
        }
        let path = entry.path();
        if path.is_dir() {
            collect_files(&path, root, texts, meta, depth + 1);
        } else {
            // Check extension
            let ext = path.extension()
                .map(|e| e.to_string_lossy().to_string())
                .unwrap_or_default();
            if !INDEXABLE_EXTS.contains(&ext.as_str()) {
                continue;
            }

            let Ok(content) = std::fs::read_to_string(&path) else { continue; };
            let rel = path.strip_prefix(root).unwrap_or(&path).to_path_buf();
            let lines: Vec<&str> = content.lines().collect();

            if lines.len() > MAX_FILE_LINES { continue; } // skip huge generated files

            // Chunk with overlap
            let mut start = 0;
            while start < lines.len() {
                let end = (start + CHUNK_LINES).min(lines.len());
                let chunk_text = lines[start..end].join("\n");

                // Prefix with file path for context
                let prefixed = format!("// {}\n{}", rel.display(), chunk_text);
                texts.push(prefixed);
                meta.push((rel.clone(), start, end));

                if end >= lines.len() { break; }
                start += CHUNK_LINES - CHUNK_OVERLAP;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sub_windows_slides_by_one_over_long_chunk() {
        // 5 lines -> windows (0,3),(1,4),(2,5): every contiguous 3-line group.
        assert_eq!(sub_windows(5), vec![(0, 3), (1, 4), (2, 5)]);
    }

    #[test]
    fn sub_windows_short_chunk_is_one_window() {
        assert_eq!(sub_windows(2), vec![(0, 2)]);
        assert_eq!(sub_windows(3), vec![(0, 3)]);
        assert_eq!(sub_windows(0), Vec::<(usize, usize)>::new());
    }

    #[test]
    fn cosine_similarity_identical_vectors_is_one() {
        let v = vec![1.0, 2.0, 3.0];
        assert!((cosine_similarity(&v, &v) - 1.0).abs() < 1e-6);
    }

    #[test]
    fn cosine_similarity_orthogonal_is_zero() {
        assert_eq!(cosine_similarity(&[1.0, 0.0], &[0.0, 1.0]), 0.0);
    }
}
