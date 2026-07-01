//! Prompt template system — context assembly and response parsing.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// A prompt template with named placeholders.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PromptTemplate {
    pub name: String,
    pub system_prompt: String,
    pub user_template: String,
    /// Expected response format hint (for parsing)
    pub response_format: ResponseFormat,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ResponseFormat {
    /// Free text
    Text,
    /// Expect markdown with code blocks
    Markdown,
    /// Expect JSON matching a schema
    Json { schema_hint: String },
    /// Expect Lean 4 code
    Lean4,
    /// Expect code in specified language
    Code { language: String },
}

impl PromptTemplate {
    /// Render the user template with variable substitution.
    pub fn render(&self, vars: &HashMap<String, String>) -> String {
        let mut result = self.user_template.clone();
        for (key, value) in vars {
            result = result.replace(&format!("{{{{{}}}}}", key), value);
        }
        result
    }

    /// Render system prompt with variable substitution.
    pub fn render_system(&self, vars: &HashMap<String, String>) -> String {
        let mut result = self.system_prompt.clone();
        for (key, value) in vars {
            result = result.replace(&format!("{{{{{}}}}}", key), value);
        }
        result
    }
}

/// Context assembled for an AI request.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AssembledContext {
    /// Files included in context with their content
    pub files: Vec<ContextFile>,
    /// Requirement text (if relevant)
    pub requirement: Option<String>,
    /// Lean spec content (if relevant)
    pub spec: Option<String>,
    /// Additional context snippets
    pub extra: Vec<String>,
    /// Total estimated token count of assembled context
    pub estimated_tokens: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContextFile {
    pub path: String,
    pub content: String,
    pub language: String,
}

impl AssembledContext {
    pub fn new() -> Self {
        Self {
            files: Vec::new(),
            requirement: None,
            spec: None,
            extra: Vec::new(),
            estimated_tokens: 0,
        }
    }

    pub fn add_file(&mut self, path: String, content: String, language: String) {
        let tokens = content.len() as u32 / 4; // rough estimate
        self.estimated_tokens += tokens;
        self.files.push(ContextFile { path, content, language });
    }

    /// Format context into a string block for inclusion in prompt.
    pub fn format_for_prompt(&self) -> String {
        let mut parts = Vec::new();

        if let Some(ref req) = self.requirement {
            parts.push(format!("## Requirement\n{}", req));
        }
        if let Some(ref spec) = self.spec {
            parts.push(format!("## Lean Specification\n```lean4\n{}\n```", spec));
        }
        for file in &self.files {
            parts.push(format!("## File: {}\n```{}\n{}\n```", file.path, file.language, file.content));
        }
        for extra in &self.extra {
            parts.push(extra.clone());
        }

        parts.join("\n\n")
    }
}

/// Parse AI response to extract code blocks.
pub fn extract_code_blocks(response: &str) -> Vec<CodeBlock> {
    let mut blocks = Vec::new();
    let mut in_block = false;
    let mut current_lang = String::new();
    let mut current_content = String::new();

    for line in response.lines() {
        if line.starts_with("```") && !in_block {
            in_block = true;
            current_lang = line.trim_start_matches('`').trim().to_string();
            current_content.clear();
        } else if line == "```" && in_block {
            in_block = false;
            blocks.push(CodeBlock {
                language: current_lang.clone(),
                content: current_content.clone(),
            });
        } else if in_block {
            if !current_content.is_empty() {
                current_content.push('\n');
            }
            current_content.push_str(line);
        }
    }

    blocks
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CodeBlock {
    pub language: String,
    pub content: String,
}

// --- Built-in Templates ---

pub fn elicitation_template() -> PromptTemplate {
    PromptTemplate {
        name: "elicitation".into(),
        system_prompt: "You are a requirements engineer working inside the TraceLean IDE. Given a user's description of what they want, \
            produce structured requirements in markdown format. Each requirement should have:\n\
            - A unique ID (REQ-XX)\n\
            - A title\n\
            - A description\n\
            - Acceptance criteria\n\
            Be precise and testable. Ask clarifying questions if the description is ambiguous.\n\n\
            You have access to the project environment via tools. Use them to read existing files and understand context before generating requirements.".into(),
        user_template: "The user wants: {{goal}}\n\n\
            Project context:\n{{context}}\n\n\
            {{tools_prompt}}\n\n\
            Generate structured requirements.".into(),
        response_format: ResponseFormat::Markdown,
    }
}

pub fn formalisation_template() -> PromptTemplate {
    PromptTemplate {
        name: "formalisation".into(),
        system_prompt: "You are a formal methods expert working inside the TraceLean IDE. Given a natural language requirement, \
            produce a Lean 4 specification that:\n\
            1. Defines the formal data types needed for implementation\n\
            2. States correctness theorems as propositions\n\
            The types should be directly usable in implementation. \
            The theorems should be translatable to integration tests.\n\n\
            You have access to the project environment via tools. Use read_file to inspect existing specs and code for context.".into(),
        user_template: "Requirement:\n{{requirement}}\n\n\
            Existing types (if any):\n{{existing_types}}\n\n\
            {{tools_prompt}}\n\n\
            Produce a Lean 4 specification.".into(),
        response_format: ResponseFormat::Lean4,
    }
}

pub fn implementation_template() -> PromptTemplate {
    PromptTemplate {
        name: "implementation".into(),
        system_prompt: "You are a programmer working inside the TraceLean IDE. Given a Lean 4 specification and a target language, \
            produce an implementation that satisfies the spec's types and theorems. \
            Follow the data types exactly. Ensure the code would pass tests derived from the theorems.\n\n\
            You have access to the project environment via tools. Use read_file to see existing code and conventions. \
            Use write_file to create implementation files. Use list_files and search_files to explore the project.".into(),
        user_template: "Lean spec:\n```lean4\n{{spec}}\n```\n\n\
            Target language: {{language}}\n\n\
            Existing code context:\n{{context}}\n\n\
            {{tools_prompt}}\n\n\
            Produce the implementation.".into(),
        response_format: ResponseFormat::Code { language: "{{language}}".into() },
    }
}

pub fn repair_template() -> PromptTemplate {
    PromptTemplate {
        name: "repair".into(),
        system_prompt: "You are a debugging expert working inside the TraceLean IDE. Given a violation (test failure or spec mismatch), \
            suggest either a code fix or a spec update. Explain which is more appropriate and why.\n\n\
            You have access to the project environment via tools. Use read_file to inspect related files. \
            Use query_trace_graph and query_code_element to understand traceability relationships. \
            Use search_files to find related code patterns.".into(),
        user_template: "Violation:\n{{violation}}\n\n\
            Current code:\n```{{language}}\n{{code}}\n```\n\n\
            Spec:\n```lean4\n{{spec}}\n```\n\n\
            {{tools_prompt}}\n\n\
            Suggest a fix.".into(),
        response_format: ResponseFormat::Markdown,
    }
}
