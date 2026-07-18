# Myth: Text-as-Universal-Interface Design

> Everything is text. Text has grammar. Grammar nodes bind to style AND behavior.

## 1. Core Concept

A single architecture where any UI surface (code editor, file tree, menus, keybindings) is:
1. A **text buffer**
2. Parsed by a **Tree-sitter grammar**
3. Annotated via **Tree-sitter queries** that capture named nodes
4. Those captures map to **style** (colors, bold, etc.) AND **functions** (actions the user can invoke)

This means the same machinery renders syntax-highlighted code, drives modal keybindings, builds file tree UI, and powers menus.

## 2. The Three Inputs

| Input | Role | Example |
|-------|------|---------|
| Grammar | Defines text structure | `tree_sitter_json()`, custom `color_markup`, `keyboard` grammar |
| Query scheme | Maps AST nodes to capture names | `((state_name) @state_name)` |
| Binding map (JSON) | Associates captures to style props or function pointers | `{"@state_name": {"color": "#00ff00"}}` |

## 3. Style Binding (Rendering)

Captures map to visual properties:

```json
{
  "@json_string": { "color": "#0000ff" },
  "@key_sequence": { "color": "#ff0000", "underline": "1", "bold": "1" }
}
```

The frontend receives a flat array of style runs — no AST traversal needed at render time.

### Style Run Structure

```c
typedef struct artiststyle {
  UINT32_t rel_start;  // offset from previous run (cache-friendly, cheap inserts)
  UINT32_t length;
  STR_t *jsonKey;      // key into style_map
} artiststyle_t;
```

**Design choice**: `rel_start` means inserting a new run only shifts later elements, no recompute of absolute positions.

**Known limitation**: STR_t as text backing is expensive for large edits. Needs rope/gap buffer eventually.

### Dynamic Styles (Color Markup Example)

Grammar can encode style values inline in the text itself:

```
This is normal <color=#0000ff>this is blue</color>
```

Query captures both the hex value and the content span; the builder resolves `"color": "@color.value"` by looking up the sibling capture. Same machinery, different grammar.

## 4. Function Binding (Behavior)

**Key insight**: captures don't just map to style — they can map to **callable functions**.

```json
{
  "@file_line": { "functions": ["erase_file", "new_file", "rename_file"] },
  "@transitionFunction": { "action": "callBoundFunction" }
}
```

This means:
- User selects/hovers an AST node
- System looks up bound functions for that capture
- Those functions become available actions (context menu, keybind, etc.)

## 5. Keyboard as Grammar-Driven State Machine

Keybindings defined as text with a custom grammar:

```
Main:
  (C-SPC):Options
  (up):goUpLine
  (ESC):Exit

Options:
  (ESC):Main
  (c):copy
  (v):paste
  (C):Copy        // uppercase = submenu (state)
  (c):copy        // lowercase = function call
```

**Rules**:
- Uppercase target = transition to another state (menu)
- Lowercase target = call a function
- `(NM)` = no-match fallback
- `(ESC)` = always returns to parent/Main

The modal editing model (like vim) emerges from states. Tree-sitter grammar for this keymap enables syntax highlighting of the keymap itself, validation, and runtime interpretation.

## 6. UI Surfaces as Text

### File Tree
```
src/
  main.rs
  auth.rs
```
Grammar: `file_line \n file_line`
Binds: `@file_line -> {erase_file, new_file, rename_file, open_file}`

### Top Menu Bar
Same pattern — text parsed by grammar, nodes bind to actions.

### Implication
The entire application is composed of text buffers. The "GUI" is just styled text with function bindings on nodes. Rendering beautifully is a separate concern (the frontend interprets style runs).

## 7. Tree-Sitter Navigation as First-Class Movement

Because everything is an AST:
- `goParent` / `goChild` / `goLeftSibling` / `goRightSibling` — structural nav
- `goUpLine` / `goDownLine` / `goLeftChar` / `goRightChar` — spatial nav
- Selection = selecting an AST node (semantic selection)
- Click = maps to most internal named node at cursor position

## 8. Discoverability: Which-Key Pattern

The state-machine keymap naturally supports a **which-key** style popup (as in Emacs' which-key, general.el, etc.):

- User enters a state (e.g., presses `C-SPC` → enters `Options`)
- After a short delay (or immediately), display all available bindings for the current state
- Since the keymap is grammar-parsed text, generating this popup is trivial: query all children of the current state node, render as styled text

This solves the "everything-is-text is opaque" problem. The state machine already contains the full map — just surface it. No separate help system needed; the keymap IS the help.

Emacs proves this works at scale: which-key handles hundreds of bindings across deeply nested prefix maps. The grammar-based approach here is even cleaner since the keymap is already structured data (AST), not a runtime-assembled alist.

**Implementation note**: The which-key popup itself is just another text buffer with a grammar (list of `(key):label` lines), rendered with the same style machinery. Turtles all the way down.

## 9. Open Questions / TODOs

1. **Text buffer backing**: STR_t is insufficient for large files. Rope or gap buffer needed.
2. **Complex queries**: Only simple captures work now. Nested queries (class > method) need support for scoped bindings.
3. **Dynamic style resolution**: Frontend currently treats JSON values as literals. Need a pass that resolves references like `"@color.value"`.
4. **Function dispatch**: How does the runtime resolve function names to actual code? Plugin registry? Static table?
5. **Conflict resolution**: When a node matches multiple queries, which binding wins?
6. **Performance**: Incremental re-parse on edit is Tree-sitter's strength, but incremental style-run update (`local_update_style`) needs careful implementation.

## 10. Summary

| Traditional approach | Myth approach |
|---------------------|---------------|
| Code → AST → separate renderer | Code → AST → style runs (same pipe) |
| Keybindings → hardcoded map | Keybindings → grammar-parsed text → state machine |
| File tree → custom widget | File tree → text + lgrammar + function bindings |
| Menus → framework widgets | Menus → text + grammar + function bindings |

**One architecture. Text in, grammar applied, style + behavior out.**


## Also on a note simply highlights text differnt color like keyboard is also supported 

Note even for things not so much formatted as full on programming alnguages like HTML or console color codes this solution can also work like this:

2. Simple HTML-like color grammar
We define text of the form:
This is a phrase <color=#0000ff>this must be blue</color>
Tree-sitter grammar:
module.exports = grammar({
  name: 'color_markup',
  extras: $ => [],
  rules: {
    source_file: $ => repeat($._node),
    _node: $ => choice($.element, $.text),
    element: $ => seq(
      field('open_tag', $.open_tag),
      repeat($._node),
      field('close_tag', $.close_tag)
    ),
    open_tag: $ => seq('<', 'color', optional(/\s*/), '=', optional(/\s*),
                       field('color_value', $.hex_color), optional(/\s*), '>'),
    close_tag: $ => seq('</', 'color', '>'),
    hex_color: $ => /#[0-9A-Fa-f]{6}/,
    text: $ => /[^<]+/
  }
});
Query capturing color and content:
(element
  open_tag: (open_tag (hex_color) @color.value)
  close_tag: (close_tag)
  (text) @color.text
) @color.element


Captures:

@color.value -> the hex color
@color.text -> the text inside the tag
@color.element -> the full element (optional, for reference)



Mapping to JSON:
{
    "@color.text": { "color": "@color.value" }
}

This approach allows assigning styles to text spans directly based on the color tag.
The machinery to parse queries and build style runs is the same as for structured grammars - only the grammar and query differ.

(TODO: Currently, the frontend is using json as giving the values pure without need to look them up further, will need to change that in the builder creation)

# VERY IMPORTANT ABOUT FUNCITON CALLING
My idea was only that the functions will be associated with grammar nodes. 

only the function names,

Thise names then would be implemented in a given rust localization (possible the a satndard one, like under a dynamic_functions) localization. 

And the functions would potetnially receive only the node as arguemnt (content of it.) And the app state on other variable and will have to perform the action (simmilar in fact to what I had in impelemnted in C).

If you manage to think on better ideas please tell.

Also there is no need for they keyboard itself to a have a proper grammar no that i think of it, a json suffices well (that was more for a prove of concpet that inspired my in general).

# TASK TO PERFORM
YOUR TASK IS TO (1) give feedback on the idea, do you see value (that is on top).

Then formulate better the idea (2)

Then give an overview on how would you implement in tracelean. (3)

You will create a new file docs/myth_fable.md with all this  (1), (2),(3) points