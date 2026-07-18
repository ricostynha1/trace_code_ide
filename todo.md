# Context

Bedrock not supporting well caching with minimax, made me call tools on the system prompt! (it is what it is really). So we have to pass tool definition on system prompt (before that really, on bedrock.rs but it was the only most reliable way)
[ But it was the only way as mantle was not caching tool calls other way and that was burning all our money ]

(Now our solution generateat some compicaiton)


# P1) Now that bedrock is tooling on system call, the tools calls are not apeearing well on the frontend ! 
(we should fix it) (like on the  AI chat message frontend). As possibly we are not passing them as the format that the AI chat message is expecting. They should appear like read_fileX3 to read 3 files, and simply a blank space is apppearing on the  AI chat


# P2) Clearly we have a parsing problem with the tools using the  bedrock and passing that on the system prompt: 

This comes from a real prompt

```real_test_examplex
Now I understand the situation. The system prompt is in `tracelean/core/src/ai/mod.rs` at lines 56-77. The `discover_tools` tool exists but isn't mentioned in the system prompt. Let me update it to be shorter and include the discover_tools mention:

[TOOL_CALL_ERROR]
- Failed call: `{"name":"replace_str","arguments":{"path":"tracelean/core/src/ai/mod.rs","old_str":"/// Canonical system prompt for all ...` — invalid JSON: EOF while parsing an object at line 1 column 2279
```

Here the probem is that the returned json where the old str is is not sanitized .... obviously though. Unclear how too fix this though

As we are infact using Minimax directly it is better to use exactly the system prmpttheir model was fine tuned see: https://github.com/MiniMax-AI/MiniMax-M2.5/blob/main/docs/tool_calling_guide.md
(and use a equivalent parser of the output they recommned )
"We strongly recommend using vLLM or SGLang for parsing tool calls. If you cannot use the built-in parser of inference engines (e.g., vLLM and SGLang) that support MiniMax-M2.5, or need to use other inference frameworks (such as transformers, TGI, etc.), you can manually parse the model's raw output using the following method. This approach requires you to parse the XML tag format of the model output yourself."

(see that document with code examples)

The model is trained on these types of formats:
(So simply pass on the system prmpt introduce tools exactly as model expects and was trained !!! (better really))

```prompt
]~!b[]~b]system
You are a helpful assistant.

# Tools
You may call one or more tools to assist with the user query.
Here are the tools available in JSONSchema format:

<tools>
<tool>{"name": "search_web", "description": "Search function.", "parameters": {"type": "object", "properties": {"query_list": {"type": "array", "items": {"type": "string"}, "description": "Keywords for search, list should contain 1 element."}, "query_tag": {"type": "array", "items": {"type": "string"}, "description": "Category of query"}}, "required": ["query_list", "query_tag"]}}</tool>
</tools>

When making tool calls, use XML format to invoke tools and pass parameters:

<minimax:tool_call>
<invoke name="tool-name-1">
<parameter name="param-key-1">param-value-1</parameter>
<parameter name="param-key-2">param-value-2</parameter>
...
</invoke>
[e~[
]~b]user
When were the latest announcements from OpenAI and Gemini?[e~[
]~b]ai
<think>
Format Description:

]~!b[]~b]system: System message start marker
[e~[: Message end marker
]~b]user: User message start marker
]~b]ai: Assistant message start marker
]~b]tool: Tool result message start marker
<tools>...</tools>: Tool definition area, each tool is wrapped with <tool> tag, content is JSON Schema
<minimax:tool_call>...</minimax:tool_call>: Tool call area
<think>...</think>: Thinking process marker during generation
```

# P3) The error message is weak: it is this one
Expected format:
<tool_call>
{"name": "<tool_name>", "arguments": {<params>}}
</tool_call

We should indicate the tool call that failed in question, like there is enough inforation at that pint to now what tool try to be called ! (so we could say instead )
{"name":"replace_str" etc etc}   
To be even easiser we could on the json have  a string on the tools.json with one example like so, and when models failed we could just send exactly that message


# P34) critical edition bug i really i am not able to edit the file manually with insert well at all (something is clashing with the undo tree it seems)

It seems that de deele does not work very well and reliable (maybe the problem is that the modes of position on the file are not being logged..... Soemthing is clearly broken) (It seems that a back delete anywhere on the file try to delete the first carachter of the document rather quicky)

Basically undo tree or somehting is stopping to have a good pleasnat editing expereince. Potentially it is mising other basic command change position (that kinda is trigger by a click that changes the cursor to a certain carahter etc, and only after that insert and deletes can be reliable undone.)

A thing that can simplify even further is that instead of having htese two commands Insert and delete we can only have replace, that replaces a group of carachter by another after the cursos. This is generat thatn encodes a insert and a delete. 

Potentially if this is the fundametal building block the bugs can dimiuish from the apllicaiton


My solution is this, simplify even further (I only need a editing primitive for the undo tree):

The editing model can be simplified to a single primitive:

```
replace(start, old, new)
```

The undo operates using **symbol indexes**, while the underlying file is stored as a normal UTF-8 `String`.

Rust already provides the necessary abstraction because `String` is UTF-8 encoded, while `.chars()` iterates over Unicode scalar values. The conversion layer only needs to translate the model's symbol-based positions into the byte offsets required by Rust string operations.

Example:

```rust
fn byte_index_at_symbol(s: &str, symbol_index: usize) -> usize {
    s.char_indices()
        .nth(symbol_index)
        .map(|(idx, _)| idx)
        .unwrap_or(s.len())
}
```

The `replace` primitive can then be implemented by locating the first occurrence of `old` after the given symbol position, converting the symbol offset into a byte offset, and applying Rust's native string replacement:

```rust
fn replace(
    text: &mut String,
    start: usize,
    old: &str,
    new: &str,
) {
    let start_byte = byte_index_at_symbol(text, start);

    if let Some(relative_idx) = text[start_byte..].find(old) {
        let old_start = start_byte + relative_idx;
        let old_end = old_start + old.len();

        text.replace_range(old_start..old_end, new);
    }
}
```

This elimiates the insert and delte comment as a insert is equal to this 

insert(pos, "","char inserted")
and
delete(pos, "char deleted","") 

and pos basically defines cursor movement so the command replace is full reversable.

In that idea we can also only have one file primitive 

replace_file("old_file_path", "new_file_path")  
this copys the old_file to the new_file 

replace_file(NONE, "new_file_path") 
this creates a new file on new file path

replace_file("old_file_path", NONE) 
This erases the file on old_file_path 

To move a file do a copy and then do an erase. 

This also works with direcoties exactly on the same way

replace_file("old_dir_path", "new_dir_path")  

replace_file(NONE, "new_dir_path") 

replace_file("old_dir_path", NONE) 
(this ilimiantes the amount of primitives we have for files on the undo tree)

Also when editing the backspace is equivalent to 
if pos > 0 
 replace(pos -1 , "char_in_front" , "" )
if pos == 0 ( no op)



(After this primitives solve me the editing neghtmars that I am having for now  we will have ways to compact the basic editions)

like
replace(0, "","A")
replace(1,"","B")
should give "AB" and could be compacted to single command
replace(1,"", "AB") -> This compactations really could be made general like, calling a diff tool, and converting the diff to replace commands.

# P5 The diff view on the editor is not working
When i open tree undo , and hover in a node i am not seing the diff view on the editor for the file !! more than that as another feature it would be cool to view also the diff view on the file tree !! 

# P6 The connection between requiremnst -> formal spec -> code and tests 
is not being well enforced on the ui, we should have convention like //* Req abc   on a formal spec to connect to the Req etc etc, 

And things cehcking if all code is connected to any req, formal spec and vice versa if not that is only disconnected.

# P7 The utlization model context is not appearing on the chat ui 
(% of utilization in memory), but it should, also there is not the button to reset the context. 
