# TOOL IMPROVEMENTS

- [ ] T0 – Improving cahing tool caching 
Optimize prompt caching for Amazon Bedrock agents while maintaining the ability to dynamically load specialized tools via embedding-based retrieval.

Phase 1: The Static Core (Always Cached send on every prompt)
To maximize cache hit rates, every request must include core tools  in the API tool payload. They serve as the immutable prefix of the request (the tools can be seen on T2 tasks)

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

The new core tools would be:
(note a tool returns a sucess code, the main infromation, and a json with metdata data)

like: 
            ToolResult {
                success: true,
                content: format!("{} lines", count),
                data: None,
            }


{
  "name": "discover_tools",
  "description": "Find additional tools by capability.",
  "inputSchema": {
    "type": "object",
    "properties": {
      "query": {
        "type": "string",
        "description": "Capabilities or keywords."
      },
      "max_results": {
        "type": "integer",
        "default": 10
      }
    },
    "required": ["query"],
    "additionalProperties": false
  }
}
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
    "name": "list_directory",
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
    "description": "Search project files. Returns matching lines with surrounding context (like grep -C).",
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
        "context": {
          "type": "integer",
          "default": 1,
        }
      },
      "required": ["query"],
      "additionalProperties": false
    }
  },
  // This should trunate lines that are too big say more than 300 carachters
  // backed logic  (The fallback answer to the models must give hints on how to procede)

  give a hint on the data serdes part (look at the implemetnation of execure_read_file). For other command consider this (only makes sense this aditional data if the command stopped early still more things to read etc.). for isntances on a read_file one cool thing to indiacte is the number of lines on the file.

  {
  "total_matches": 1240,
  "hint": "Call find again with offset=20 for more matches (there are 1240 in total). Use read_file on specific lines to see full context."
}

  {
  "matches": [
    { "file": "data.json", "line": 1, "text": "{\"id\":1,\"name\":\"John\",\"data\":\"very long base64 string that exceeds 300 chars so I get truncated... " },
    { "file": "data.json", "line": 2, "text": "  \"short_key\": \"value\"" }
  ],
  "total_matches": 1240,
  "has_more": true,
  "next_offset": 20,
  "any_trucation" : true
  "hint": "lines {x,y,w ...} were truncated as each have more than 300 chars. You can use read_file with line number and offset fields to read that big line in chunks if needed (give the correct command of read_file for the first line in question)"
}

// Also had metadata infomration to a file ready such:
// number lines, nbytes formatted line 10M ,5k etc 
// last modified (3h , 3h2m) (3h 2 min). min (1m) (1min) 
// 30d (days)  (days no need to indicate h or minutes. only for hours is important to also have minutes. >30d (more than 30 days for the rest))
// And string repreesnting the permissions
// like : drwxr-xr-x  
// owner
// groups


  {
    "name": "read_file",
    "description": "Read max_results file lines from offset. offset can be negative -1 represents last line. A read gives also file metadata",
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
// Better this hints that if a line overflows that other function should be called called read_line, that has file offset, to allow granular reading is better than to do this function also work with this complex behaviour

// The backend can respond with some infomraiton if we have hitten the offset like

// For isntances insert as the data serded json object the following:
{
  "metadata": {
    "lines": 1240,
    "bytes": 52143,
    "size": "51 KB",
    "modified": "3h 12m",
    "permissions": "-rw-r--r--",
    "owner": "alvaro",
    "group": "users"
  },
  "has_more": true,
  "next_offset": 300,
}

// And with these i add this fucntion read_line to the available tools for the model

// Other case 
{
  "lines": [
    { "number": 1, "text": "full line of normal length" },
    { "number": 2, "text": "another complete line" },
    { "number": 3, "text": "this line is also short and fully readable" }
  ],
  "offset": 1,
  "max_results": 3,
  "total_lines": 1240,
  "has_more": true,
  "next_offset": 4,
  "hint": "Call read_file again with offset=4 to continue reading."
}



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




- [ ] T3 improve system prompt 
Fix: Add information to the system prompt:

"
Tool rules:
- Tool calls: arguments only, no extra text unless explicitly asked.
- Batch: Call multiple tools in a single response when possible
- file_filter: glob syntax (shell wildcards), e.g., *.py, src/**/*.ts.
"


# CONTEXT MANAGEMENT



- [ ] T5 Dynamic History Pruning 
for all these we will have to differntaite betwwen the main brnach (everythin is not pruned all messages send and can be saws (it is the user view on the chat)). To the model view what is infact pass tot he model that for now will have the cahnges listed here on T5 and on T6. (but potentially can be a lot more complex like, conversation compactation etc etc, so this is the basis of context management) (So we will have to redeifine the arquitecture !, basically what is passed tot he model now is not what the user sees but a transfomration on that, that is also saved.)

The Mechanic: Implement an automated context janitor in your framework loop. If the agent calls read_file on src/main.rs at Iteration 2, and then executes a write_range or str_replace on that same file at Iteration 4, the tool output from Iteration 2 is now stale.

The Implementation: Before sending the message history to the next iteration, have your framework scan the history and remove the old read_range from the history. The model only needs the current state of the code.
(this avoids redudndat file reads that occupy space)

- [ ] T6 Ephemeral Error Squashing (Schema Corrections)
When a model makes a tool parameter error (like Qwen missing the start argument), appending that mistake and the system's error response to the permanent chat history forces the model to re-read its own failure on every subsequent turn.  (what can make it even worse)

The Mechanic: Treat schema parsing failures or missing argument errors as ephemeral.

The Implementation: When the backend detects a tool validation failure, send an immediate correction prompt to the model without committing that failed turn or the error message to the master history array. Once the model returns a valid tool call, append only the successful tool call and its output to the main conversation history. This keeps the long-term context completely clean of syntax blunders.



# Other thigns to consider that are not solved

1 - Automatic Output Overflow Protection
If any tool response exceeds 8,000 characters, the system:

Saves the full output to a unique temporary file inside a well‑known directory (e.g., /tmp/mcp_overflow/).

Discards the raw output from the LLM context to preserve token budget.

Returns a compact, structured response that:

Informs the model of the overflow.

Provides the file path.

Shows a short preview (first 200 chars).

Instructs the model to use read_file (for sequential reading) or find (for targeted searches) to inspect the content efficiently.

Reason:
[This turns a dangerous context‑bomb into an actionable pointer. The model never loses access to the data—it just retrieves it on demand, using tools that are already pagination‑ and truncation‑safe]

{
  "status": "overflow",
  "message": "Tool output exceeded 8,000 character limit and was offloaded.",
  "saved_to": "/tmp/mcp_overflow/out_20260713_142356_a1b2c3.txt",
  "total_chars": 15234,
  "total_lines" : 300,
  "preview": "First 200 characters of the original output... and last 200 chars of the output",
  "hint": "Use read_file with offset=0 and max_results=50 to view this file line‑by‑line, or use find with a query to search inside it."
}

2 - If the agent runs >20 turns, even pruned history grows too large.
Extra mechanism: Implement a token-budget monitor (e.g., 80% of model's context window). When exceeded:

Compactation can also be forced by the user (to reduce current costs) (like with a button press)

Identify the "middle" of the conversation (excluding the latest 3–5 turns, which are most relevant).

Call the LLM to condense that middle block into a 2–3 sentence summary of what was done and decided. (in the future this will probably be a subagent not for now)

Replace the raw middle messages with a single synthetic system or assistant message:
"Earlier summary: User asked to refactor the auth module. We found 3 files, decided to use JWT, and ran tests – all passed. Current state: focusing on the /login handler."

Do not alter the UI history (user view); only mutate the model view.




# Other notes 
Witht he tool caclling now bein well made i dont believe that this is needed
tracelean/core/src/ai/response_parser.rs 
(check if that is the case)

Check it is completly not being ued for now only in agents with some lean things that are still not working or tested can be removed