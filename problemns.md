## Current implementation priorities

- [ ] T1: Hover diff view on undo tree — compare current file state vs hovered node state


**Goal:** When hovering a node in the undo tree, show a real unified diff between the current editor state and the state at that node.

**Approach (clone + replay):**
1. Clone current `AppState` (it's already `Clone`)
2. Use `undo_tree.jump_to(target)` on the clone to get commands
3. Execute those commands on the clone via `execute_raw`
4. Extract the file content from the clone's buffer
5. Diff current content vs clone content using simple line-based diff
6. Return unified diff string

~15 lines in `state.rs`. Uses existing infrastructure. Add caching later if hover feels slow.

**Backend:** New method `pub fn node_content_diff(&self, node_id: NodeId) -> String` in state.rs.

**Frontend:** Already calls `get_undo_node_diff` and displays result. No changes needed.


- [ ] T2: File deletes and creates are not being reversed on the undo tree (or at lesat if they are that reversal is not showing on the frontend tree structure 


- [ ] T3: Agent things 
- T3.1 The session cost on the botton of the chat should appear imeediatly (in themometn any message is received exchange it is not it was only whn user got back control).

- T3. 2 When the agent is reponing and perfom lots of tool callings on the web chat of the agent it appears that a giant message is being generated, instead of a lot of small ones (but when i close the tab and go back the message is indees sepearted in lots of tool calls and agent messages)

- T3.3 Logs The log shouls show on top the total cost of the logs on top (it is not it is only showing cost per log)

- T3.4 The model context used shold be displayes potentially after session cost (%context used)1M  (if context 1M or like 0.2M if context is 200 k etc)

- T3.5 I still did a small crash and the conversation completly stopped there: Do me a description of this code repository what do i have what is the structure with files what do they do
call list_files
rsp list_files
call list_files
rsp list_files
call list_files
rsp list_files
call list_files
rsp list_files
call list_files
rsp list_files
tool_call_failed: model produced invalid JSON for get_symbols({"path": "src/auth.rs"): EOF while parsing an object at line 1 column 22

integration/
unit/
[tool_call_id: call_43133b2af8ae4d1a9d65021a]
ASSISTANT:
→ get_symbols({"path": "src/auth.rs")
TOOL:
Error: invalid JSON arguments — EOF while parsing an object at line 1 column 22
[tool_call_id: call_135ab74824fb4aa68735cb9e]
Response
HTTP 400 Bad Request: {"error":{"code":"validation_error","message":"ErrorEvent { error: APIError { type: \"BadRequestError\", code: Some(400), message: \"Expecting ',' delimiter: line 1 column 23 (char 22)\", param: None 

- T3.6 The tool selector seems to be odd
The tool selector only consider i believe the current message what is odd for inaces if i have a conversation like this:

Can you list the files? 

And then i answer : Can you repeat? 
On the repsat the tools are not being selected and passed to the model in my inpection of the logs and tool usage.

Potentially the fix would be if the tool does dot find tools that match query in one message, 

compact with the user queries of the last say 5 queries and provide those list to the model (note list capped also at the maximum number of tools we have, so if merged passed we would trimmed)

- T3.7 Show more cost information on log
In each log I would like to appear the cost taken as well 

Instead of this 
Token Usage
Input	3198
Output	48
Thinking	0
Cached	1344
Cache savings	$0.000968

Show 
Token Usage
Non Input	3198  (cost)
Cached inputs ccc  (cost)
Cache savings (savings)

Output	48 (cost)
Thinking	0 (cost)

- T3.8 Create hard money expand cap of configurable in settings (for now 1 dolar default, can be increased).
































# Agent Architecture Decision

## Decision: Implement Zed ACP as our agent interface
Gathered all context on the internet to implement Zed ACP protocol)

**Why:**
1. Emerging standard for agent-to-editor communication (JSON-RPC 2.0, "LSP for agents")
2. Ships a default agent while remaining open to any ACP-compatible agent
3. Immediate compatibility with Claude Code, Codex CLI, Gemini CLI, etc.
4. Separates "IDE features" from "agent intelligence" — the right boundary
5. Other people can extend our IDE with their own agents without touching our code
6. Backed by Zed + JetBrains, 60+ agents in registry, active development

## Protocol Stack (settled as of mid-2026)

```
├─────────────────────────────────────────┤
│  Zed ACP      (agent ↔ editor/IDE)      │  ← To implement
├─────────────────────────────────────────┤
│  MCP          (agent ↔ tools/data)      │  ← We already have this
└─────────────────────────────────────────┘
```

## Architecture: Message-Passing Only

Agent communicates with IDE ONLY via message passing:
```
Agent → IDE: tool calls (read_file, write_file, etc.) via MCP
IDE → Agent: tool results via MCP
Agent → UI: events (message_start, message_update, tool_execution_start, etc.)
UI → Agent: prompts, steering messages, abort signals
```

Benefits:
- Agent is testable in isolation
- Agent is replaceable (swap for any ACP agent)
- Other people can connect their own agents
- Can run agent in separate process/container for isolation
