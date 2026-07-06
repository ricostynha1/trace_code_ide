//! Tool Selection Policy — determines which editing mode to use.
//!
//! Priority order:
//! 1. AST Edit (structural changes to declarations/functions/expressions)
//! 2. Git Patch (multi-region or complex edits)
//! 3. Search & Replace (simple text substitutions)
//! 4. Whole File Write (last resort, new/generated files)

use serde::{Deserialize, Serialize};

/// Which editing mode the policy recommends.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum EditMode {
    AstEdit,
    GitPatch,
    SearchReplace,
    WholeFileWrite,
}

/// Intent classification for an edit request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EditIntent {
    /// Modify a declaration (function, struct, enum, impl, trait, etc.)
    ModifyDeclaration,
    /// Insert a new declaration
    InsertDeclaration,
    /// Delete a declaration
    DeleteDeclaration,
    /// Rename a symbol
    RenameSymbol,
    /// Modify an expression or statement
    ModifyExpression,
    /// Add/remove/modify imports
    ModifyImport,
    /// Edit class/struct/enum members (fields, variants, methods)
    ModifyMembers,
    /// Refactoring (extract, inline, move)
    Refactor,
    /// Replace a literal/constant value
    ReplaceLiteral,
    /// Update a comment
    UpdateComment,
    /// Change a configuration value
    ChangeConfig,
    /// Edit spans multiple unrelated regions
    MultiRegion,
    /// Formatting-only change
    FormatChange,
    /// Existing diff available
    ApplyDiff,
    /// Create entirely new file
    CreateNewFile,
    /// Replace machine-generated output
    ReplaceGenerated,
}

/// Policy engine that recommends editing modes.
pub struct EditPolicy;

impl EditPolicy {
    /// Given an edit intent, return the recommended editing mode.
    pub fn recommend(intent: &EditIntent) -> EditMode {
        match intent {
            // AST Edit — structural, syntax-aware
            EditIntent::ModifyDeclaration
            | EditIntent::InsertDeclaration
            | EditIntent::DeleteDeclaration
            | EditIntent::RenameSymbol
            | EditIntent::ModifyExpression
            | EditIntent::ModifyImport
            | EditIntent::ModifyMembers
            | EditIntent::Refactor => EditMode::AstEdit,

            // Git Patch — multi-region or complex
            EditIntent::MultiRegion
            | EditIntent::FormatChange
            | EditIntent::ApplyDiff => EditMode::GitPatch,

            // Search & Replace — simple text
            EditIntent::ReplaceLiteral
            | EditIntent::UpdateComment
            | EditIntent::ChangeConfig => EditMode::SearchReplace,

            // Whole File Write — last resort
            EditIntent::CreateNewFile
            | EditIntent::ReplaceGenerated => EditMode::WholeFileWrite,
        }
    }

    /// Given multiple intents, return the highest-priority mode that covers all.
    pub fn recommend_for_multiple(intents: &[EditIntent]) -> EditMode {
        if intents.is_empty() {
            return EditMode::WholeFileWrite;
        }

        // If any intent requires whole-file write, use that
        if intents.iter().any(|i| Self::recommend(i) == EditMode::WholeFileWrite) {
            return EditMode::WholeFileWrite;
        }

        // If mixed modes or multi-region, use git patch
        let modes: Vec<EditMode> = intents.iter().map(|i| Self::recommend(i)).collect();
        if modes.len() > 1 {
            let first = modes[0];
            if modes.iter().any(|m| *m != first) {
                return EditMode::GitPatch;
            }
        }

        modes[0]
    }

    /// Determine if an edit is expressible as AST edit given file has parseable language.
    pub fn can_use_ast(has_language_support: bool, intent: &EditIntent) -> bool {
        has_language_support && Self::recommend(intent) == EditMode::AstEdit
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ast_edit_recommended_for_declarations() {
        assert_eq!(EditPolicy::recommend(&EditIntent::ModifyDeclaration), EditMode::AstEdit);
        assert_eq!(EditPolicy::recommend(&EditIntent::InsertDeclaration), EditMode::AstEdit);
        assert_eq!(EditPolicy::recommend(&EditIntent::DeleteDeclaration), EditMode::AstEdit);
        assert_eq!(EditPolicy::recommend(&EditIntent::RenameSymbol), EditMode::AstEdit);
    }

    #[test]
    fn test_patch_recommended_for_multi_region() {
        assert_eq!(EditPolicy::recommend(&EditIntent::MultiRegion), EditMode::GitPatch);
        assert_eq!(EditPolicy::recommend(&EditIntent::ApplyDiff), EditMode::GitPatch);
    }

    #[test]
    fn test_search_replace_for_literals() {
        assert_eq!(EditPolicy::recommend(&EditIntent::ReplaceLiteral), EditMode::SearchReplace);
        assert_eq!(EditPolicy::recommend(&EditIntent::UpdateComment), EditMode::SearchReplace);
        assert_eq!(EditPolicy::recommend(&EditIntent::ChangeConfig), EditMode::SearchReplace);
    }

    #[test]
    fn test_whole_file_for_new_files() {
        assert_eq!(EditPolicy::recommend(&EditIntent::CreateNewFile), EditMode::WholeFileWrite);
        assert_eq!(EditPolicy::recommend(&EditIntent::ReplaceGenerated), EditMode::WholeFileWrite);
    }

    #[test]
    fn test_mixed_intents_use_patch() {
        let intents = vec![EditIntent::ModifyDeclaration, EditIntent::ReplaceLiteral];
        assert_eq!(EditPolicy::recommend_for_multiple(&intents), EditMode::GitPatch);
    }

    #[test]
    fn test_homogeneous_intents_use_single_mode() {
        let intents = vec![EditIntent::ModifyDeclaration, EditIntent::InsertDeclaration];
        assert_eq!(EditPolicy::recommend_for_multiple(&intents), EditMode::AstEdit);
    }

    #[test]
    fn test_can_use_ast_with_language() {
        assert!(EditPolicy::can_use_ast(true, &EditIntent::ModifyDeclaration));
        assert!(!EditPolicy::can_use_ast(false, &EditIntent::ModifyDeclaration));
        assert!(!EditPolicy::can_use_ast(true, &EditIntent::ReplaceLiteral));
    }
}
