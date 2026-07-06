//! Search & Replace — exact text substitution with uniqueness validation.
//!
//! Produces shadow buffer, then diffs → Insert/Delete only.

use crate::surgical_edit::error::SurgicalEditError;
use crate::surgical_edit::shadow_diff::diff_to_commands;
use crate::surgical_edit::EditResult;
use std::path::PathBuf;

/// A search-and-replace edit.
#[derive(Debug, Clone)]
pub struct SearchReplace {
    pub search: String,
    pub replacement: String,
    pub allow_multiple: bool,
}

impl SearchReplace {
    pub fn new(search: impl Into<String>, replacement: impl Into<String>) -> Self {
        Self {
            search: search.into(),
            replacement: replacement.into(),
            allow_multiple: false,
        }
    }

    pub fn with_allow_multiple(mut self, allow: bool) -> Self {
        self.allow_multiple = allow;
        self
    }

    /// Apply search&replace → shadow buffer → diff → Insert/Delete commands.
    pub fn apply(
        &self,
        file: &PathBuf,
        source: &str,
    ) -> Result<EditResult, SurgicalEditError> {
        let count = source.matches(&self.search).count();

        if count == 0 {
            return Err(SurgicalEditError::SearchTextNotFound);
        }
        if count > 1 && !self.allow_multiple {
            return Err(SurgicalEditError::MultipleMatches(count));
        }

        // Build shadow buffer
        let shadow = if self.allow_multiple {
            source.replace(&self.search, &self.replacement)
        } else {
            source.replacen(&self.search, &self.replacement, 1)
        };

        // Diff → Insert/Delete commands
        let commands = diff_to_commands(file, source, &shadow);

        Ok(EditResult {
            file: file.clone(),
            commands,
            new_content: shadow,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::Command;

    fn assert_only_insert_delete(commands: &[Command]) {
        for cmd in commands {
            match cmd {
                Command::Insert { .. } | Command::Delete { .. } => {}
                other => panic!("Expected only Insert/Delete, got: {:?}", other),
            }
        }
    }

    #[test]
    fn test_simple_replace() {
        let source = "const MAX_SIZE: u64 = 100 * 1024 * 1024;\nfn main() {}\n";
        let edit = SearchReplace::new(
            "const MAX_SIZE: u64 = 100 * 1024 * 1024;",
            "const MAX_SIZE: u64 = 200 * 1024 * 1024;",
        );
        let result = edit.apply(&PathBuf::from("test.rs"), source).unwrap();
        assert_only_insert_delete(&result.commands);
        assert!(result.new_content.contains("200 * 1024 * 1024"));
    }

    #[test]
    fn test_not_found() {
        let source = "fn main() {}";
        let edit = SearchReplace::new("nonexistent text", "replacement");
        let result = edit.apply(&PathBuf::from("test.rs"), source);
        assert!(matches!(result, Err(SurgicalEditError::SearchTextNotFound)));
    }

    #[test]
    fn test_multiple_matches_fails() {
        let source = "let x = 1;\nlet x = 1;\n";
        let edit = SearchReplace::new("let x = 1;", "let x = 2;");
        let result = edit.apply(&PathBuf::from("test.rs"), source);
        assert!(matches!(result, Err(SurgicalEditError::MultipleMatches(2))));
    }

    #[test]
    fn test_multiple_matches_allowed() {
        let source = "let x = 1;\nlet x = 1;\n";
        let edit = SearchReplace::new("let x = 1;", "let x = 2;").with_allow_multiple(true);
        let result = edit.apply(&PathBuf::from("test.rs"), source).unwrap();
        assert_only_insert_delete(&result.commands);
        assert_eq!(result.new_content.matches("let x = 2;").count(), 2);
    }

    #[test]
    fn test_preserves_surrounding_content() {
        let source = "// header\nconst VALUE: i32 = 5;\n// footer\n";
        let edit = SearchReplace::new("const VALUE: i32 = 5;", "const VALUE: i32 = 10;");
        let result = edit.apply(&PathBuf::from("test.rs"), source).unwrap();
        assert_only_insert_delete(&result.commands);
        assert!(result.new_content.contains("// header"));
        assert!(result.new_content.contains("// footer"));
        assert!(result.new_content.contains("const VALUE: i32 = 10;"));
    }
}
