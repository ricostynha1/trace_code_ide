---
describes: [REQ-SHOW, REQ-ACT, REQ-LOCK, REQ-ROLLUP]
described_hash:
  REQ-SHOW: 34c6ba52a2c5208c
  REQ-ACT: 9046b0e9fd5b66b7
  REQ-LOCK: 0a0865a009898e29
  REQ-ROLLUP: a93cb0f6b8f92bae
---

# The editor, end to end

```
key ─keymap─► action ─dispatch─► intent ─produce─► buffer ─draw─► screen
             REQ-MYTH   REQ-ACT           REQ-SHOW        REQ-VIEW
```

Four links, each a function from a value to a value, each modelled and
differentially tested. A frontend owns the last arrow only.

## Producers (`REQ-SHOW`)

One function per kind, in `surface::produce`:

| Kind | From |
|---|---|
| `file` | path, text, and the marks a parser found |
| `directory` | the entries under a path (`view::directory_buffer`) |
| `review` | two texts, diffed by common prefix and suffix |
| `menu` | a keymap mode's rows |
| `record` | observed events |
| `menu:requirements` | the requirement set, each row its marks, how many clauses hold each, and its lines run (`tracelean-view` only) |
| `menu:design` | the same set, drawn indented along `refines`, folded to its roots except where unfolded |
| `record:sandbox` | what an agent changed, said, and is estimated to have spent |

The last two are stations' content, and all three are producers for the same
reason the rest are: a window that knew what a requirement index looks like and
a terminal that did not would be two editors. Both of the first two carry the
evidence marks in spans, one a letter (`Role::Mark { mark, held }`), rather than
leaving them in the text, so a frontend colours a gap apart from a proof without
parsing what it was given.
The graph walk is bounded — `refines` is data, a cycle in it is representable,
and a walk without a bound would not be a function.

A row's `+children 88%` is lines run over it and everything refining it, each
child's figure added whole, so a requirement two paths reach counts twice: it
says how much under a row was run, not an exact count, and appears only when
something under the row was left unrun. Both stations take `open_only`: the
index keeps what has a clause short of a mark it can hold or a line never run,
the graph also what is above such a requirement.

None reads a disk; the state arrives as an argument. Every one ends with
`tidy`, which clamps, drops backwards and overlapping spans and sorts — so a
produced buffer has no faults *whatever* it was produced from, including marks a
parser got wrong. That is the property generated states check and hand-written
tests miss.

A span dropped is not a span moved. Moving one hides which producer was wrong.

## Dispatch (`REQ-ACT`)

`dispatch(action, focus, workspace, waiting) -> Intent`, where the focus is the
buffer, the offset, and the text of the span under the cursor, and `waiting` is
an agent's changes not yet taken in (accepting one file finds its commands).

```
Display kind   produce that buffer and show it
Edit command   change the workspace, through the one path with an inverse
Travel move    back, forward, or to another branch
Observe watch  start, accept, reject
Persist        write the state where it belongs
Refuse why     unknown action, or a target that was not supplied
```

Total: every name the keymap dispatches has an answer and every name it does not
has a refusal, because a key that appears to do nothing is the failure nobody
reports. For the same reason the pointer's menu offers nothing the dispatcher
would refuse as unknown, and a role's actions come from one table
(`actions_for`), so a path offers the same things wherever it is drawn.

The workspace is an argument because deleting carries the content it removed —
the witness is part of the command, which is what gives it an inverse.

## Frontends

A frontend renders and dispatches; it produces nothing. That is checked by
reading: a frontend source constructing a `Buffer` or a `Span` fails
`no_frontend_builds_a_buffer_of_its_own`.

`crates/tui`'s `draw` module is the single place it decides what the screen says
— the interactive loop, the `--protocol` answer and the `--paint` screen all go
through it, which is what stops it reporting one thing and painting another.
Roles become colours there and nothing else.

`crates/desktop` and `web/` are in [07-frontends](07-frontends.md). All three
drive `crates/editor`, so none of them is a second answer to what the editor
does.

## Modes

A mode with a parent was reached from a menu, and a key nothing binds there goes
back to the root — so a mistyped leader sequence never strands anyone. A mode
without a parent passes the key through to the editor instead.

There are two parentless modes, and the difference is what a passed-through key
means: in `Normal` it is refused out loud, in `Insert` it is text. `Insert`
therefore binds Escape itself, because a parentless mode that did not would have
no way out.

A mode's menu is offered *beside* the buffer, in `editor.menu`, not in place of
it. Replacing the screen with the menu moves the cursor onto a menu row, and
`Space f o` then opens the row rather than the file — which is exactly what it
did until a test walked that path.

## Reports

A report is a `record` buffer, so it is produced like every other buffer. What
fills it is a read of the tree:

| Report | Reads |
|---|---|
| `check`, `findings` | the index, through `checker::check` |
| `evidence` | the roles claiming each clause, and the marks it holds |
| `lock` | renders the index and writes `.tracelean/trace.lock` |
| `rollup` | `rollup::tree` per root, covered where a clause is tested |
| `stale` | every doclink's state |
| `drt bindings` | the annotations, with `.tracelean/drt.json` |
| `history`, `observed` | the history tree, and what watching saw |

A clause's marks past `A` come from the lock and never from the tree: an
annotation is a claim, and holds nothing more until a backend earns it. A
project with no lock reads `A---` at most and says so.

`drt run`, `drt shrink` and `drt coverage` name the command instead of answering
— producing them means starting a process, and this editor starts none
(`ARCH-NO-DRIVING`). `drt judge` says the judgement is a person's.

## Watching

`observe.start` reads the working tree and derives, through `mirror::mutations`,
how it differs from what the editor holds. Nothing is started: somebody else ran
something, and this looks at what it left.

`observe.accept` pushes those commands through the history tree, so an agent's
work is undoable for the same reason typing is. `observe.reject` drops them and
leaves the tree alone — deleting another process's work on a keystroke is not a
decision this editor makes for anybody.

The `evidence` report writes each clause's marks in place
(`REQ-EVID.marks_written`): a clause with a proved model and an unchecked
implementation reads `A--P`, which is a different situation from `A---` and
calls for different work.

## Asking for a name, and finding

An action that needs a name nobody pointed at (`Space f r` renames the file
under the cursor, `Space f n` makes one) asks for it on the status line, where
the keys typed are the answer: Enter performs it, Escape drops it, Backspace
edits it. The context menu asks in a box for the same actions and sends the same
answer to `Editor::choose`, so a key and a pointer make the one change
(`REQ-NAV.name_is_asked_for`).

`Space f f` asks what to look for and goes to the next match; `Space f /` asks
and lists the lines of the buffer that hold one — the way to search a file tree,
the requirements or the findings, where the lines are the information. The text
may hold `*` (any run of characters) and `?` (any one), within a line and in any
case (`REQ-NAV.find_by_glob`); `F3` goes on to the next match of the last text.

The stations on the left are Project, Trace, Sandbox, Design, History,
Findings, Definitions and Proof.

**Definitions** lists every colour of the theme with its value now, read from the
JSON that sets it, and says where the keys and the languages live. `Space f e`
(or a click) on a row asks for the new value on the status line and writes it to
`.tracelean/theme.json` at once; the window repaints without a restart, and a
value that is not a colour is refused (`REQ-LOOK.setting_edited_in_place`).

**Proof** shows the goals Lean reports at the cursor of a Lean file, beside the
file, following the cursor and the edits, as the infoview does in VS Code. It
asks Lean's own server (`lake env lean --server`, in the nearest folder with a
`lean-toolchain`) and never waits for it: the answer is picked up by the next
poll, so a slow elaboration costs a stale panel and not a frozen key. Opening a Lean
file opens the goals pane beside it, closing either closes the other, and there
is no other way to open it (`REQ-LSP.goals_open_with_the_file`,
`goals_close_with_the_file`). When the server cannot be started the pane says
why. This is the one process the editor starts apart from the
differential runners, and the architecture test allows exactly that module
(`crates/core/src/lean_server.rs`): it is Lean's toolchain, never an agent
(`REQ-LSP.proof_state_shown`).

## Still open

- Nobody has opened the desktop window (see [05-view](05-view.md)).
