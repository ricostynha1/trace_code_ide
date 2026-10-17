---
id: REQ-NAV
title: Getting around the workspace
refines: [REQ-ACT, ARCH-CORE-SHELL]
status: draft
decomposition: complete
clauses:
  declaration_line: A line shall declare the word after its declaring word — `fn`, `struct`, `def`, `theorem`, `function` and the like — when only modifiers come before that word, and a path such as `Kind::File` shall be declared by no line.
  definitions_found: Going to a name's definition shall offer every line of the workspace declaring it, by path then line, and a dotted name shall also be looked for by its last part.
  uses_found: Finding a name's uses shall list every line holding it as a whole word, and a search every line holding the text in any case, by path then line, at most the number asked.
  quick_open: Quick open shall list the files whose paths hold the typed letters in order, best first — letters in the file's own name before letters in its folders, letters together before letters apart, then the shorter name, then the shorter path — and every file, shortest first, for no letters.
  find_by_glob: Finding shall take `*` as any run of characters and `?` as any one, within a line and in any case, and a text with neither as itself; a match shall never be empty.
  find_lists_lines: Listing matches shall give every line of the buffer that holds one, by line number, and the line itself.
  name_is_asked_for: An action that needs a name nobody pointed at shall ask for it on the status line, take the keys typed as the answer, perform the action on Enter and nothing on Escape.
  session_recalled: Opening a project shall bring back the tabs, the open folders and the file in front it had when last closed, leaving out what no longer exists.
---

# Getting around the workspace

The editor starts no language server. What a person mostly wants from "go to
definition" is the line that says `fn name` or `theorem name`, and that is
found by reading the files the editor already holds. Quick open reaches a file
whose name is known faster than the tree does. Both are pure functions of the
workspace (`surface/definition.rs`, `surface/quick.rs`); recalling the session
is a shell that reads and writes `.tracelean/editor.json`.

Finding is asked for the way a name is: `Space f f` asks what to look for on the
status line and goes to the next match, `Space f /` asks and lists the lines that
hold one — which is the way to search a file tree, a requirement list or the
findings, where the lines *are* the information. Both take `*` and `?`, so a
fragment of a name or a state is enough (`REQ-*.draft`). `F3` goes on to the
next match of the last text. A search of every file is Ctrl+Shift+F.

A key sequence that needs a name (`Space f r` renames, `Space f n` makes a file)
asks for it where the status line is, as Emacs's minibuffer does, so the hands
never leave the keys. The context menu asks in a box for the same actions and
sends the same answer.
