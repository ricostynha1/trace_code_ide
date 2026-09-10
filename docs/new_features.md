# New Features Plan

This document outlines the plan and considerations for implementing the requested features.

---

## 1. Embeddings Auto-Build on Project Selection

### Problem
The `find` tool with semantic mode fails with "embeddings index not initialized" because:
1. The embeddings index is never built automatically when a project is selected
2. In `runtime.rs:902`, `embed_index` is always passed as `&None`
3. There's no automatic build when `open_project()` is called

### Root Cause Analysis
- `SharedIndex` type exists in `core/src/ai/embeddings.rs` (line 43)
- `new_shared_index()` function exists but is never called anywhere in gui_backend
- `execute_tool_reviewed` receives `&None` for embed_index (runtime.rs:902)

### Solution
1. Add `embed_index` to the global state in gui_backend
2. Build embeddings asynchronously when project is opened
3. Pass the actual index to tool executor

### Implementation Steps
```rust
// In gui_backend/src/lib.rs, add state wrapper:
// pub struct EmbeddingsIndexWrapper(pub ai::SharedIndex);

// In open_project(), after project opens:
// - Create shared index: let embed_index = ai::new_shared_index()
// - Start background thread to build it
// - Store in state for tool executor access
```

### Resilience Improvements
- Add timeout for embeddings build (don't block forever)
- Show progress indicator in UI  
- Handle partial failures gracefully
- Cache embeddings to disk for faster reload on same project

---

## 2. Improve Regex Error Message (Glob vs Regex)

### Problem
Users get confusing error when using glob patterns like `*.py`:
```
Invalid regex '*.py': regex parse error:
    *.py
    ^
error: repetition operator missing expression
```

### Solution
Detect common glob patterns and provide clearer guidance:

- In `execute_find()` auto mode, detect if pattern looks like glob
- Use glob matching
- Provide clearer messaging about mode differences

---

## 3. OpenRouter Testing with Free Models

### Current State
- `openrouter.rs` exists (489 lines) - full implementation
- Frontend has settings for openrouter_api_key
- Need to verify integration works end-to-end

### Free Models Available on OpenRouter
- `anthropic/claude-3-haiku` - free tier
- `google/gemini-pro-1.5` - free tier  
- `mistralai/mistral-7b-instruct` - free tier
- And many more via OpenRouter's free tier

### Testing Plan
1. Add integration test that uses OpenRouter with free model
2. Use environment variable for API key (CI-friendly)
3. Verify tool definitions work correctly
4. Test fallback behavior when OpenRouter fails

### Implementation
```rust
// In core/src/ai/openrouter.rs tests:
#[tokio::test]
async fn test_openrouter_free_model() {
    let api_key = std::env::var("OPENROUTER_API_KEY").unwrap();
    let provider = OpenRouterProvider::new(api_key);
    // Use a free model
    let request = AiRequest {
        model: "anthropic/claude-3-haiku".to_string(),
        // ...
    };
    let response = provider.complete(&request).await;
    assert!(response.is_ok());
}
```

---

## 4. OpenCode Integration via ACP Protocol

### Background
OpenCode is an agentic coding tool. The user wants to integrate it using ACP (Agent Communication Protocol) to allow delegation when the built-in agent isn't sufficient.

### What is ACP?
ACP (Agent Communication Protocol) is a protocol for agent-to-agent communication. It allows:
- One agent to delegate tasks to another
- Structured request/response format
- Capability discovery

### Integration Points
1. **As MCP-like client**: Similar to existing MCP client in `mcp_client.rs`
2. **Tool integration**: Allow agent to call OpenCode as a tool
3. **Result handling**: Parse and incorporate OpenCode results

### Implementation Approach
1. **Study ACP protocol** - understand message format, capabilities
2. **Create ACP client** similar to MCP client:
   ```rust
   // tracelean/core/src/acp_client.rs
   pub struct AcpClient {
       endpoint: String,
       client: reqwest::Client,
   }
   
   impl AcpClient {
       pub async fn connect(&mut self, endpoint: &str) -> Result<()>;
       pub async fn send_request(&self, request: AcpRequest) -> Result<AcpResponse>;
   }
   ```
3. **Add to tool registry** - allow agent to call OpenCode as a tool
4. **Handle async communication** - ACP is async

### Benefits
- Allows delegation to OpenCode for specialized tasks
- Provides fallback if built-in agent isn't sufficient
- Enables multi-agent workflows

---

## Summary of Changes Required

| Feature | Files to Modify | Complexity |
|---------|-----------------|------------|
| Embeddings auto-build | `gui_backend/src/lib.rs`, `gui_backend/src/ipc/editor.rs`, `core/src/agent/runtime.rs` | Medium |
| Regex error improvement | `core/src/ai/tool_executor.rs` | Low |
| OpenRouter tests | `core/src/ai/openrouter.rs` | Medium |
| OpenCode/ACP integration | New: `core/src/acp_client.rs` | High |

---

## Notes

- All features should maintain backward compatibility
- Consider adding configuration options for each feature
- Add appropriate logging for debugging
- Ensure proper error handling throughout
- The embeddings feature is the highest priority as it breaks existing functionality