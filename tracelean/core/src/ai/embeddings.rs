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
    pub start_line: usize,
    pub end_line: usize,
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

/// Configuration for index building.
const CHUNK_LINES: usize = 30;
const CHUNK_OVERLAP: usize = 5;
const BATCH_SIZE: usize = 64;
const MAX_FILE_LINES: usize = 10_000;

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
    pub fn query(&mut self, query: &str, max_results: usize, path_filter: &str, file_filter: &str) -> Vec<EmbedResult> {
        if self.chunks.is_empty() {
            return Vec::new();
        }

        // Embed query
        let query_vecs = match self.model.embed(vec![query.to_string()], None) {
            Ok(v) => v,
            Err(_) => return Vec::new(),
        };
        let query_vec = &query_vecs[0];

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
            .map(|(i, chunk)| (cosine_similarity(query_vec, &chunk.vector), i))
            .collect();

        // Sort descending by score
        scored.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
        scored.truncate(max_results);

        scored.into_iter().map(|(score, idx)| {
            let chunk = &self.chunks[idx];
            EmbedResult {
                file: chunk.file.clone(),
                start_line: chunk.start_line,
                end_line: chunk.end_line,
                snippet: chunk.text.clone(),
                score,
            }
        }).collect()
    }

    /// Number of indexed chunks.
    pub fn chunk_count(&self) -> usize {
        self.chunks.len()
    }
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
