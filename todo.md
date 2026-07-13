# TOOL IMPROVEMENTS

- [ ] T0 – Improving cahing tool caching 
Optimize prompt caching for Amazon Bedrock agents while maintaining the ability to dynamically load specialized tools via embedding-based retrieval.

Phase 1: The Static Core (Always Cached send on every prompt)
To maximize cache hit rates, every request must include the following four static tools in the API payload. They serve as the immutable prefix of the request (the tools can be seen on T2 tasks)

Phase 2: Dynamic Expansion (On-Demand Loading)
If the model determines that the static core is insufficient to fulfill the user's intent, it calls the discover_tools with a descriptive query (e.g., "find a tool to analyze CSV data").

Behind the scenes:

The application receives the query and runs it through the existing embedding-based retrieval logic.

The system identifies the top-K most relevant specialized tools from the registry. (using the method that now is choosing every tool)

The crucial step (Bedrock compliance):

The application appends these retrieved tool definitions (full schemas) to the tools array in the next API request payload.

Simultaneously, it returns a toolResult message to the model, listing the same tools so the model knows they are available.

Temporary payload state (cache miss):

text
write_range    (static)
read_range     (static)
list_file      (static)
search_tool    (static)
new_cool_tool1 (dynamic - appended)
new_cool_tool2 (dynamic - appended)
⚠️ Note: This modification changes the request prefix, resulting in a cache miss for this specific turn. This is intentional and acceptable, as this expansion only occurs when absolutely necessary.

To maximize the return on investment (ROI) of the initial cache miss, the system must not immediately revert to the static core after a single toolUse exchange. Retaining the dynamically appended tools for several subsequent turns allows the model to call them repeatedly (e.g., multiple read_range or write_range operations) while maintaining a cache-hit state for those follow-up requests.

However, indefinite accumulation of dynamic tools will bloat the request prefix, increase latency, incur higher base token costs, and ultimately distort the cache key. Therefore, reversion is governed by a size-based threshold.

Rule: Dynamic tools remain appended to the static core as long as their total combined character length (the sum of their full JSON schemas, names, and descriptions) remains below a configurable limit—defaulting to 2,048 characters.

When the threshold is exceeded: The system automatically purges all appended dynamic tools and reverts the tools array to the original static core for the next prompt.

When a new session begins: The system reverts immediately (this requirement is already handled by your application's session management).

- [ ] T1 make write_range (which will be renamed in point T2 to edit_file) more forgiving 
Problem (Qwen): The model omitted start, causing an error and an extra iteration (~1,400 tokens, +$0.005).

Fix: Modify the tool implementation so that if start and end are not supplied, it replaces the entire file content.

This eliminates the most common parameter mistake without sacrificing fine‑grained editing when needed.

- [ ] T2 Improving tools 
Do these improvemnts note these are the tools that will belong to the core
Pass these tools to the core (note write_range is renamed to edit_file, read_range to read_file)

The new core tools are these:
[
  {
    "name": "discover_tools",
    "description": "Find additional tools by capability.",
    "inputSchema": {
      "type": "object",
      "properties": {
        "query": {
          "type": "string",
          "description": "Capabilities or keywords phrases."
        },
        "max_results": {
          "type": "integer",
          "default": 10
        }
      },
      "required": ["query"],
      "additionalProperties": false
    }
  },
  {
    "name": "list_files",
    "description": "List files and directories.",
    "inputSchema": {
      "type": "object",
      "properties": {
        "path": {
          "type": "string",
          "default": ""
        },
        "recursive": {
          "type": "boolean",
          "default": false
        },
        "max_results": {
          "type": "integer",
          "default": 50
        },
        "offset": {
          "type": "integer",
          "default": 0
        },
        "file_filter": {
          "type": "string"
        },
        "max_depth": {
          "type": "integer"
        }
      },
      "required": [],
      "additionalProperties": false
    }
  },
  {
    "name": "find",
    "description": "Search project files.",
    "inputSchema": {
      "type": "object",
      "properties": {
        "query": {
          "type": "string"
        },
        "recursive": {
          "type": "boolean",
          "default": false
        },
        "mode": {
          "type": "string",
          "enum": ["auto", "regex", "semantic"],
          "default": "auto"
        },
        "case_sensitive": {
          "type": "boolean",
          "default": false
        },
        "max_results": {
          "type": "integer",
          "default": 20
        },
        "offset": {
          "type": "integer",
          "default": 0
        },
        "path": {
          "type": "string"
        },
        "file_filter": {
          "type": "string"
        },
        "max_depth": {
          "type": "integer"
        }
      },
      "required": ["query"],
      "additionalProperties": false
    }
  },
  {
    "name": "read_file",
    "description": "Read max_results file lines from offset.",
    "inputSchema": {
      "type": "object",
      "properties": {
        "path": {
          "type": "string"
        },
        "offset": {
          "type": "integer",
          "default": 0
        },
        "max_results": {
          "type": "integer",
          "default": 300
        }
      },
      "required": ["path"],
      "additionalProperties": false
    }
  },
  {
    "name": "edit_file",
    "description": "Replace content between lines [start,end[ with text. If start equals end, it appends text after start. -1 represents last line (Python like indexing for negatives)",
    "inputSchema": {
      "type": "object",
      "properties": {
        "path": {
          "type": "string"
        },
        "text": {
          "type": "string"
        },
        "start": {
          "type": "integer"
        },
        "end": {
          "type": "integer"
        }
      },
      "required": ["path", "text"],
      "additionalProperties": false
    }
  },
  {
    "name": "run_shell",
    "description": "Run a shell command.",
    "inputSchema": {
      "type": "object",
      "properties": {
        "command": {
          "type": "string"
        },
        "cwd": {
          "type": "string"
        },
        "timeout_secs": {
          "type": "integer",
          "default": 30
        },
        "head_lines": {
          "type": "integer",
          "default": 5
        },
        "tail_lines": {
          "type": "integer",
          "default": 5
        }

      },
      "required": ["command"],
      "additionalProperties": false
    }
  }
]

One more optimization that I think is worth adopting

Since you control both the tools and the system prompt, you can make the tool responses themselves teach the model how to continue, reducing the need for prompt text (for the cases with pagination, find, list_files, read answer with something like)

For example:

{
  "matches": [...],
  "has_more": true,
  "next_offset": 20,
  "hint": "Call find again with offset=20 to continue."
}


- [ ] T3 improve system prompt 
Fix: Add a single, generic line to the system prompt:

"
Tool rules:
- Tool calls: arguments only, no extra text unless explicitly asked.
- Batch: Call multiple tools in a single response when possible
- file_filter: glob syntax (shell wildcards), e.g., *.py, src/**/*.ts.
"




# CONTEXT MANAGEMENT


- [ ] T5 Dynamic History Pruning 
for all these we will have to differntaite betwwen the main brnach (everythin is not pruned all messages send and can be saws (it is the user view on the chat)). To the model view what is infact pass tot he model that for now will have the cahnges listed here on T5 and on T6. (but potentially can be a lot more complex like, conversation compactation etc etc, so this is the basis of context management)

The Mechanic: Implement an automated context janitor in your framework loop. If the agent calls read_file on src/main.rs at Iteration 2, and then executes a write_range or str_replace on that same file at Iteration 4, the tool output from Iteration 2 is now stale.

The Implementation: Before sending the message history to the next iteration, have your framework scan the history and remove the old read_range from the history. The model only needs the current state of the code.
(this avoids redudndat file reads that occupy space)

- [ ] T6 Ephemeral Error Squashing (Schema Corrections)
When a model makes a tool parameter error (like Qwen missing the start argument), appending that mistake and the system's error response to the permanent chat history forces the model to re-read its own failure on every subsequent turn.  (what can make it even worse)

The Mechanic: Treat schema parsing failures or missing argument errors as ephemeral.

The Implementation: When the backend detects a tool validation failure, send an immediate correction prompt to the model without committing that failed turn or the error message to the master history array. Once the model returns a valid tool call, append only the successful tool call and its output to the main conversation history. This keeps the long-term context completely clean of syntax blunders.

