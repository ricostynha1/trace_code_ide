//! MCP Tool definitions — the set of tools agents can use to interact with the environment.
//! Used by internal agents, MCP host, and agent permission model.

use serde::{Deserialize, Serialize};
use super::provider::{ToolSchema, ToolFunction};

/// A tool callable by an agent.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolDefinition {
    pub name: String,
    pub description: String,
    pub parameters: Vec<ToolParam>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolParam {
    pub name: String,
    pub param_type: ParamType,
    pub description: String,
    pub required: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ParamType {
    String,
    Integer,
    Boolean,
    Array { item_type: Box<ParamType> },
    Object,
}

impl ToolDefinition {
    /// Convert to OpenAI-compatible function calling schema.
    pub fn to_tool_schema(&self) -> ToolSchema {
        let mut properties = serde_json::Map::new();
        let mut required = Vec::new();

        for param in &self.parameters {
            let type_str = match &param.param_type {
                ParamType::String => "string",
                ParamType::Integer => "integer",
                ParamType::Boolean => "boolean",
                ParamType::Array { .. } => "array",
                ParamType::Object => "object",
            };

            let mut prop = serde_json::Map::new();
            prop.insert("type".into(), serde_json::Value::String(type_str.into()));
            prop.insert("description".into(), serde_json::Value::String(param.description.clone()));

            if let ParamType::Array { item_type } = &param.param_type {
                let item_type_str = match item_type.as_ref() {
                    ParamType::String => "string",
                    ParamType::Integer => "integer",
                    ParamType::Boolean => "boolean",
                    _ => "string",
                };
                let mut items = serde_json::Map::new();
                items.insert("type".into(), serde_json::Value::String(item_type_str.into()));
                prop.insert("items".into(), serde_json::Value::Object(items));
            }

            properties.insert(param.name.clone(), serde_json::Value::Object(prop));

            if param.required {
                required.push(serde_json::Value::String(param.name.clone()));
            }
        }

        let parameters = serde_json::json!({
            "type": "object",
            "properties": properties,
            "required": required,
        });

        ToolSchema {
            tool_type: "function".into(),
            function: ToolFunction {
                name: self.name.clone(),
                description: self.description.clone(),
                parameters,
            },
        }
    }
}

/// Convert all builtin tool definitions to OpenAI tool schemas (alphabetically sorted for cache stability).
pub fn builtin_tool_schemas() -> Vec<ToolSchema> {
    let mut schemas: Vec<ToolSchema> = builtin_tool_definitions().iter().map(|t| t.to_tool_schema()).collect();
    schemas.sort_by(|a, b| a.function.name.cmp(&b.function.name));
    schemas
}

/// Result of a tool invocation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolResult {
    pub success: bool,
    pub content: String,
    pub data: Option<serde_json::Value>,
}

/// A tool call request from an agent.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCall {
    pub name: String,
    pub arguments: serde_json::Value,
}

/// UI/tracing metadata attached to a tool call by the agent.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ToolCallMeta {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub goal: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub step: Option<u32>,
}

/// Agent-side tool call: MCP-compliant call + UI metadata envelope.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentToolCall {
    pub name: String,
    pub arguments: serde_json::Value,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ui: Option<ToolCallMeta>,
}

impl AgentToolCall {
    pub fn to_mcp_call(&self) -> ToolCall {
        ToolCall {
            name: self.name.clone(),
            arguments: self.arguments.clone(),
        }
    }

    pub fn reason(&self) -> Option<&str> {
        self.ui.as_ref().and_then(|m| m.reason.as_deref())
    }
}

/// All built-in tools available to agents.
/// NOTE: read_file/write_file removed. Use read_range/write_range instead.
pub fn builtin_tool_definitions() -> Vec<ToolDefinition> {
    vec![
        // --- Reading ---
        ToolDefinition {
            name: "count_lines".into(),
            description: "Return line count of a file. Cheap metadata check — use before read_range to know file size.\nEx: {\"path\":\"src/main.rs\"} → \"42 lines\"".into(),
            parameters: vec![ToolParam {
                name: "path".into(),
                param_type: ParamType::String,
                description: "Relative file path.".into(),
                required: true,
            }],
        },
        ToolDefinition {
            name: "read_range".into(),
            description: "Read lines from a file. Negative idx counts from end (-1=last line). Omit start/end to read whole file (avoid for large files — use count_lines first).\nEx: {\"path\":\"src/lib.rs\",\"start\":0,\"end\":20} — first 20 lines\nEx: {\"path\":\"src/lib.rs\",\"start\":-10} — last 10 lines".into(),
            parameters: vec![
                ToolParam {
                    name: "path".into(),
                    param_type: ParamType::String,
                    description: "Relative file path.".into(),
                    required: true,
                },
                ToolParam {
                    name: "start".into(),
                    param_type: ParamType::Integer,
                    description: "Start line (0-indexed, inclusive). Negative = from end. Default: 0.".into(),
                    required: false,
                },
                ToolParam {
                    name: "end".into(),
                    param_type: ParamType::Integer,
                    description: "End line (exclusive). Negative = from end. Default: EOF.".into(),
                    required: false,
                },
            ],
        },
        // --- Writing ---
        ToolDefinition {
            name: "write_range".into(),
            description: "Replace lines start..end with text. start==end means pure insert (no removal). Negative idx supported (-1=EOF).\nEx: {\"path\":\"f.rs\",\"start\":0,\"end\":0,\"text\":\"// header\\n\"} — prepend\nEx: {\"path\":\"f.rs\",\"start\":-1,\"end\":-1,\"text\":\"// end\\n\"} — append at EOF\nEx: {\"path\":\"f.rs\",\"start\":5,\"end\":8,\"text\":\"new code\\n\"} — replace lines 5-7".into(),
            parameters: vec![
                ToolParam {
                    name: "path".into(),
                    param_type: ParamType::String,
                    description: "Relative file path.".into(),
                    required: true,
                },
                ToolParam {
                    name: "start".into(),
                    param_type: ParamType::Integer,
                    description: "Start line (0-indexed, inclusive). Negative = from end. -1=EOF.".into(),
                    required: true,
                },
                ToolParam {
                    name: "end".into(),
                    param_type: ParamType::Integer,
                    description: "End line (exclusive). Same as start = pure insert (no lines removed). Negative = from end.".into(),
                    required: true,
                },
                ToolParam {
                    name: "text".into(),
                    param_type: ParamType::String,
                    description: "Replacement text (include trailing \\n).".into(),
                    required: true,
                },
            ],
        },
        ToolDefinition {
            name: "str_replace".into(),
            description: "Find & replace exact string in file. old_str must match uniquely (1 occurrence).\nEx: {\"path\":\"f.rs\",\"old_str\":\"fn old(\",\"new_str\":\"fn new(\"}".into(),
            parameters: vec![
                ToolParam {
                    name: "path".into(),
                    param_type: ParamType::String,
                    description: "Relative file path.".into(),
                    required: true,
                },
                ToolParam {
                    name: "old_str".into(),
                    param_type: ParamType::String,
                    description: "Exact string to find (must match once).".into(),
                    required: true,
                },
                ToolParam {
                    name: "new_str".into(),
                    param_type: ParamType::String,
                    description: "Replacement string.".into(),
                    required: true,
                },
            ],
        },
        // --- Filesystem ---
        ToolDefinition {
            name: "list_files".into(),
            description: "List files/dirs under path. Hidden dirs excluded.\nEx: {\"path\":\"src\"} → [\"main.rs\",\"lib.rs\",\"utils/\"]".into(),
            parameters: vec![ToolParam {
                name: "path".into(),
                param_type: ParamType::String,
                description: "Relative dir path (empty=\"\" for project root).".into(),
                required: false,
            }],
        },
        ToolDefinition {
            name: "delete_file".into(),
            description: "Delete file. Reversible via undo.\nEx: {\"path\":\"tmp/scratch.rs\"}".into(),
            parameters: vec![ToolParam {
                name: "path".into(),
                param_type: ParamType::String,
                description: "Relative file path.".into(),
                required: true,
            }],
        },
        // --- Search ---
        ToolDefinition {
            name: "find_grep".into(),
            description: "Regex search across files. Returns file:line:match. Use instead of reading entire files to locate code.\nEx: {\"pattern\":\"fn main\",\"path\":\"src\",\"file_filter\":\"*.rs\",\"max_results\":10}".into(),
            parameters: vec![
                ToolParam {
                    name: "pattern".into(),
                    param_type: ParamType::String,
                    description: "Regex pattern to search.".into(),
                    required: true,
                },
                ToolParam {
                    name: "path".into(),
                    param_type: ParamType::String,
                    description: "File or dir to search (default: project root).".into(),
                    required: false,
                },
                ToolParam {
                    name: "recursive".into(),
                    param_type: ParamType::Boolean,
                    description: "Recurse into subdirs (default: true).".into(),
                    required: false,
                },
                ToolParam {
                    name: "max_results".into(),
                    param_type: ParamType::Integer,
                    description: "Cap results (default: 20).".into(),
                    required: false,
                },
                ToolParam {
                    name: "context_lines".into(),
                    param_type: ParamType::Integer,
                    description: "Lines before/after each match (default: 0).".into(),
                    required: false,
                },
                ToolParam {
                    name: "file_filter".into(),
                    param_type: ParamType::String,
                    description: "Glob filter e.g. \"*.rs\", \"*.py\" (default: all files).".into(),
                    required: false,
                },
            ],
        },
        ToolDefinition {
            name: "find_embed".into(),
            description: "Semantic search across codebase using embeddings. Use when grep won't work (e.g. \"find auth handler\" when fn is named validate_credentials).\nEx: {\"query\":\"error handling for uploads\",\"max_results\":5}".into(),
            parameters: vec![
                ToolParam {
                    name: "query".into(),
                    param_type: ParamType::String,
                    description: "Natural language query.".into(),
                    required: true,
                },
                ToolParam {
                    name: "path".into(),
                    param_type: ParamType::String,
                    description: "Scope search to dir (default: project root).".into(),
                    required: false,
                },
                ToolParam {
                    name: "max_results".into(),
                    param_type: ParamType::Integer,
                    description: "Max results (default: 5).".into(),
                    required: false,
                },
                ToolParam {
                    name: "file_filter".into(),
                    param_type: ParamType::String,
                    description: "Glob filter e.g. \"*.rs\" (default: all).".into(),
                    required: false,
                },
            ],
        },
        // --- Traceability ---
        ToolDefinition {
            name: "query_trace_graph".into(),
            description: "Query traceability graph by requirement ID. Returns linked specs, code, tests.\nEx: {\"req_id\":\"REQ-01\"}".into(),
            parameters: vec![ToolParam {
                name: "req_id".into(),
                param_type: ParamType::String,
                description: "Requirement ID (e.g. REQ-01).".into(),
                required: true,
            }],
        },
        ToolDefinition {
            name: "query_code_element".into(),
            description: "Query trace graph for a code element → linked reqs/specs.\nEx: {\"file\":\"src/auth.rs\",\"name\":\"login\"}".into(),
            parameters: vec![
                ToolParam {
                    name: "file".into(),
                    param_type: ParamType::String,
                    description: "File containing element.".into(),
                    required: true,
                },
                ToolParam {
                    name: "name".into(),
                    param_type: ParamType::String,
                    description: "Symbol name.".into(),
                    required: true,
                },
            ],
        },
        ToolDefinition {
            name: "list_requirements".into(),
            description: "List all project requirements (ID, title, status).\nEx: {} (no args)".into(),
            parameters: vec![],
        },
        ToolDefinition {
            name: "get_symbols".into(),
            description: "Get parsed symbols (fns, structs, classes) from file.\nEx: {\"path\":\"src/auth.rs\"} → \"login fn L5-L20, Credentials struct L1-L4\"".into(),
            parameters: vec![ToolParam {
                name: "path".into(),
                param_type: ParamType::String,
                description: "Relative file path.".into(),
                required: true,
            }],
        },
        // --- Web (bugs.md Feature 6) ---
        ToolDefinition {
            name: "web_search".into(),
            description: "Search the web, or fetch a URL. Pass a URL to fetch: the page is saved as text to a temp file inside the project (.tracelean/web/) and you then read/grep it with read_range/find_grep. Pass search terms to get a list of result titles + URLs.\nEx: {\"query\":\"rust tokio watch channel\"} — search\nEx: {\"query\":\"https://docs.rs/tokio\"} — fetch page to file".into(),
            parameters: vec![
                ToolParam {
                    name: "query".into(),
                    param_type: ParamType::String,
                    description: "Search terms, or a full http(s):// URL to fetch.".into(),
                    required: true,
                },
                ToolParam {
                    name: "max_results".into(),
                    param_type: ParamType::Integer,
                    description: "Max search results (default: 5, search mode only).".into(),
                    required: false,
                },
            ],
        },
        // --- Shell ---
        ToolDefinition {
            name: "run_shell".into(),
            description: "Execute shell cmd in project dir. Returns stdout/stderr.\nEx: {\"command\":\"cargo test\",\"timeout_secs\":60}".into(),
            parameters: vec![
                ToolParam {
                    name: "command".into(),
                    param_type: ParamType::String,
                    description: "Shell command.".into(),
                    required: true,
                },
                ToolParam {
                    name: "timeout_secs".into(),
                    param_type: ParamType::Integer,
                    description: "Timeout in seconds (default: 30).".into(),
                    required: false,
                },
            ],
        },
    ]
}

/// Format tool definitions into a system prompt fragment.
/// Includes behavioral directives: parallel calls, tool priority, compactness.
pub fn tools_as_system_prompt(tools: &[ToolDefinition]) -> String {
    let mut out = String::from("# Tools\n\n");

    // Behavioral directives (T0, T1, T2, T3, T10)
    out.push_str("\
## Rules

PARALLEL: When multiple independent tool calls are needed, emit ALL in ONE response. \
Independent = result of A not needed as input to B.
GOOD: write_range(file1,0,0,\"// hi\\n\") + write_range(file2,0,0,\"// hi\\n\") in one response.
BAD: write_range(file1,...), wait, write_range(file2,...).

PRIORITY: str_replace > write_range(start==end) for inserts > write_range for replacements. \
Never rewrite a whole file when a smaller edit suffices.

SKIP-READ: Don't read a file if the edit is deterministic (prepend, append, replace exact string). \
Use str_replace or write_range directly.

COMPACT: Minimize output tokens. No filler (\"I'll help you\", \"Let me\", \"Now let's\"). \
State only what you did or will do.

JSON: All tool arguments must be valid JSON. Double-check braces and quotes.

");

    // Tool list
    out.push_str("## Available Tools\n\n");
    for tool in tools {
        out.push_str(&format!("### {}\n{}\n", tool.name, tool.description));
        if !tool.parameters.is_empty() {
            out.push_str("Params: ");
            let params: Vec<String> = tool.parameters.iter().map(|p| {
                let req = if p.required { "" } else { "?" };
                format!("{}{}:{:?}", p.name, req, p.param_type)
            }).collect();
            out.push_str(&params.join(", "));
            out.push('\n');
        }
        out.push('\n');
    }

    out.push_str("\
To call a tool, respond with a JSON block:
```tool_call
{\"name\": \"insert_lines\", \"arguments\": {\"path\": \"src/main.rs\", \"line\": 0, \"text\": \"// comment\\n\"}}
```

Multiple calls in one response (parallel):
```tool_call
{\"name\": \"insert_lines\", \"arguments\": {\"path\": \"src/a.rs\", \"line\": 0, \"text\": \"// hi\\n\"}}
```
```tool_call
{\"name\": \"insert_lines\", \"arguments\": {\"path\": \"src/b.rs\", \"line\": 0, \"text\": \"// hi\\n\"}}
```

When done, provide final answer without tool_call blocks.
");
    out
}

/// System prompt additions for tool-calling compactness (Req 3).
/// These rules reduce wasted output tokens and redundant tool calls.
pub fn tool_calling_rules() -> &'static str {
    r#"Tool rules:
- Tool calls: arguments only, no extra text unless explicitly asked.
- Batch: call multiple tools in a single response when possible.
- file_filter: glob syntax (shell wildcards), e.g. *.py, src/**/*.ts.
- Prefer `find` over `run_shell` for file search tasks.
- Do not repeat file contents already visible in context. Reference by path and line range.
- When a tool returns has_more=true, decide whether more data is needed before calling again.
- Minimize redundant reads: if a file region is already in context and unmodified, do not re-read it."#
}
