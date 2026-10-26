---
id: REQ-NAV
title: Getting around the workspace
refines: [REQ-ACT, ARCH-CORE-SHELL]
status: draft
decomposition: complete
clauses:
  declaration_line: The system shall take a line to declare the word after its declaring word — `fn`, `struct`, `def`, `theorem`, `function` and the like — when only modifiers come before that word, and shall take no line to declare a path such as `Kind::File`.
  definitions_found: When going to a name's definition, the system shall offer every line of the workspace declaring it, by path then line, and shall also look for a dotted name by its last part.
  uses_found: When finding a name's uses, the system shall list every line holding it as a whole word, and when searching, every line holding the text in any case, by path then line, at most the number asked.
  quick_open: When quick open is given letters, the system shall list the files whose paths hold the letters in order, best first — letters in the file's own name before letters in its folders, letters together before letters apart, then the shorter name, then the shorter path — and given no letters, every file, shortest first.
  find_by_glob: When finding, the system shall take `*` as any run of characters and `?` as any one, within a line and with the letters a to z in either case, and a text with neither as itself, and shall never give an empty match.
  find_lists_lines: When listing matches, the system shall give every line of the buffer that holds one, by line number, with the line itself.
  name_is_asked_for: When an action needs a name nobody pointed at, the system shall ask for it on the status line, take the keys typed as the answer, perform the action on Enter and nothing on Escape.
  session_recalled: When a project is opened, the system shall bring back the tabs, the open folders and the file in front it had when last closed, leaving out what no longer exists.
---

# Getting around the workspace

The editor starts no language server for this: "go to definition" mostly wants
the line that says `fn name` or `theorem name`, found by reading the files the
editor already holds, and quick open reaches a known file faster than the tree.
Both are pure functions of the workspace (`surface/definition.rs`,
`surface/quick.rs`); recalling the session is a shell that reads and writes
`.tracelean/editor.json`.

`Space f f` asks what to find and goes to the next match, `Space f /` lists the
lines holding one, and `*` and `?` let a fragment be enough (`REQ-*.draft`). A
key sequence that needs a name asks for it on the status line, as Emacs's
minibuffer does, and the context menu asks in a box for the same actions.
