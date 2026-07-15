# Tool Definition: Single Source of Truth

## Problem

Tool definitions exist in two places:
1. `data/tools.json` — schema sent to model (param names, types, required fields)
2. `core/src/ai/tool_executor.rs` — parameter extraction hardcoded with `get_str_arg(call, "name")`

If someone renames a param in JSON (e.g. `pattern` → `query`), the executor silently breaks.

## Proposed Solution: `build.rs` code generation

### Approach

`build.rs` reads `data/tools.json` at **compile time** and generates:

```rust
// AUTO-GENERATED from data/tools.json — do not edit
pub mod tool_params {
    pub mod find {
        pub const QUERY: &str = "query";
        pub const MODE: &str = "mode";
        pub const CASE_SENSITIVE: &str = "case_sensitive";
        pub const MAX_RESULTS: &str = "max_results";
        pub const PATH: &str = "path";
        pub const FILE_FILTER: &str = "file_filter";
        // Required params
        pub const REQUIRED: &[&str] = &["query"];
    }
    pub mod edit_file {
        pub const PATH: &str = "path";
        pub const TEXT: &str = "text";
        pub const START: &str = "start";
        pub const END: &str = "end";
        pub const REQUIRED: &[&str] = &["path", "text"];
    }
    // ... etc
}
```

Then executor uses:
```rust
use crate::generated::tool_params::find;
let pattern = get_str_arg(call, find::QUERY); // compile-time binding to schema
```

### Benefits
- If schema changes, constants update → any hardcoded wrong name = compile error
- REQUIRED array usable for auto-validation + error message generation
- Zero runtime cost (all const)

### Alternative: Runtime validation middleware

Instead of code generation, add a validation step in `execute_tool_with_index`:

```rust
fn validate_args(call: &ToolCall, registry: &ToolRegistry) -> Result<(), ToolResult> {
    let schema = registry.schema_for(&call.name)?;
    for required in schema.required_params() {
        if call.arguments.get(required).is_none() {
            return Err(ToolResult {
                success: false,
                content: format!(
                    "Missing '{}' argument. Expected: {}({})",
                    required, call.name, schema.signature_string()
                ),
                data: None,
            });
        }
    }
    Ok(())
}
```

Pros: no build step complexity. Cons: errors only at runtime, still possible to use wrong param name in executor code.

### Recommendation

Use `build.rs` approach. It's ~50 lines of build script, gives compile-time safety, and is the standard Rust pattern for this (similar to how protobuf/flatbuffers work).

## Current Workarounds (applied)

- `find` executor accepts both `"query"` and `"pattern"` via `get_str_arg(call, "pattern").or_else(|| get_str_arg(call, "query"))`
- `edit_file` handles both-omitted case for start/end
- Error messages include expected signature from each tool handler
