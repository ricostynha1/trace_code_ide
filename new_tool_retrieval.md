# Specification: Local Semantic MCP Tool Retrieval System

## 1. Overview

Implement a local retrieval layer for MCP (Model Context Protocol) tool selection.

The current system uses **TF-IDF keyword retrieval** to select relevant tools from the available MCP tool list. This feature replaces or augments TF-IDF with **semantic embedding-based retrieval** to improve tool discovery when user intent does not exactly match tool names or descriptions.

The goal is:

* Run fully locally.
* Have minimal latency.
* Avoid sending the entire MCP tool list to the LLM.
* Retrieve only the most relevant tools before LLM tool selection.
* Maintain compatibility with existing MCP tool definitions.

---

# 2. Goals

## Functional goals

The system must:

1. Index all available MCP tools.
2. Generate semantic embeddings for each tool.
3. Store embeddings locally.
4. Retrieve the top-K most relevant tools for a user query.
5. Return candidate tools that can be passed to the LLM.
6. Support rebuilding the index when tools change.
7. Support replacing the current TF-IDF retrieval implementation.

---

# 3. Retrieval Architecture

The retrieval pipeline should be:

```
                 MCP Tools
                    |
                    v
          Tool description builder
                    |
                    v
          Local embedding model
                    |
                    v
             Vector index
                    |
                    |
User Query ---------+
                    |
                    v
          Query embedding generation
                    |
                    v
              Similarity search
                    |
                    v
          Top-K candidate tools
                    |
                    v
              LLM tool selection
```

---

# 4. Embedding Model

## Default model

Use:

```
BAAI/bge-small-en-v1.5
```

Requirements:

* Must run locally.
* Prefer ONNX inference.
* No external API calls.
* CPU inference must be supported.

Recommended implementation:

```
FastEmbed
```

Reason:

* Lightweight.
* Optimized ONNX runtime.
* Low startup overhead.
* Good retrieval quality for short technical descriptions.

---

# 5. Tool Representation

Do not embed only the raw MCP tool name.

Instead, generate an enriched textual representation.

Example:

```
Tool:
read_file

Description:
Reads the contents of a file from the filesystem.

Category:
filesystem

Aliases:
open file
load file
display file
show source code
cat file

Examples:
- Read Cargo.toml
- Open src/main.rs
- Display configuration file

Parameters:
path: filesystem path
```

The embedding should capture:

* tool name
* description
* parameters
* examples
* aliases
* category

---

# 6. Vector Storage

Use:

```
HNSWlib
```

Requirements:

* Local persistent index.
* Fast approximate nearest neighbor search.
* No external database dependency.

Store:

```
tool_id
embedding_vector
metadata
```

Example:

```json
{
  "tool_id": "read_file",
  "description": "Reads a file",
  "embedding": [...]
}
```

---

# 7. Retrieval Algorithm

Given a user query:

Example:

```
"open the configuration file"
```

Perform:

1. Generate query embedding.
2. Search vector index.
3. Return top-K tools.

Default:

```
K = 5
```

Configurable:

```
retrieval.top_k
```

Example output:

```json
[
  {
    "tool": "read_file",
    "score": 0.91
  },
  {
    "tool": "list_directory",
    "score": 0.62
  }
]
```

---

# 8. Similarity Metric

Use:

```
cosine similarity
```

between:

```
query_embedding
```

and

```
tool_embedding
```

# 10. Index Lifecycle

## Startup 
If database exist, dont recompute, ellse recomputed. 

If folder where mcp information is stored is modified regaring last observation recompute database (only on startup though no inlive apllication remakes)


# 12. Configuration

Example:

```yaml
retrieval:
  enabled: true

  provider:
    type: embeddings

    model:
      name: BAAI/bge-small-en-v1.5
      backend: fastembed

  vector_store:
    type: hnswlib
    path: .mcp/tool_index

  top_k: 5

```

---

# 13. Failure Handling

If embedding model fails:
(thorw a error for the user "developer to debug")


## Do Retrieval tests

Create benchmark queries:

Example:

Tool:

```
git_diff
```

Queries:

```
show changes
what changed in git
compare current branch
```

Expected:

```
git_diff in top-K results
```

---