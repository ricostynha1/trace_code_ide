---
describes: [REQ-SHOW, REQ-ACT, REQ-LOCK, REQ-ROLLUP]
described_hash:
  REQ-SHOW: ef7db576d47e74de
  REQ-ACT: 79e056ad0341b89d
  REQ-LOCK: bac341f63b8d1e71
  REQ-ROLLUP: 44424fd9e2e5f00a
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
| `menu:requirements` | the requirement set, each row carrying the level it reached (`tracelean-view` only) |
| `menu:design` | the same set, drawn indented along `refines`, folded to its roots except where unfolded |
| `record:sandbox` | what an agent changed, said, and is estimated to have spent |

The last two are stations' content, and all three are producers for the same
reason the rest are: a window that knew what a requirement index looks like and
a terminal that did not would be two editors. Both of the first two carry the
evidence level in a span (`Role::Level(grade)`) rather than leaving it in the
text, so a frontend colours L1 apart from L4 without parsing what it was given.
The graph walk is bounded — `refines` is data, a cycle in it is representable,
and a walk without a bound would not be a function.

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
| `evidence` | the roles claiming each clause, and the level the lock records |
| `lock` | renders the index and writes `.tracelean/trace.lock` |
| `rollup` | `rollup::tree` per root, coverage at floor L3 |
| `stale` | every doclink's state |
| `drt bindings` | `.tracelean/drt.json` |
| `history`, `observed` | the history tree, and what watching saw |

A clause's level comes from the lock and never from the tree: an annotation is a
claim, and a claim is L1 until a backend earns more. A project with no lock reads
L1 everywhere and says so.

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

The `evidence` report shows the per-bond chain beside the minimum
(`REQ-EVID.chain_rendered`): a clause with a proved model and an unchecked
implementation reads `L1/L1/L4`, which is a different situation from `L1/L1/L1`
and calls for different work.

## Still open

- The model↔implementation bond is L1 everywhere, because no binding declares
  coverage floors and L3 needs one met. See [04-coverage](04-coverage.md).
- No clause has been judged, so the requirement↔model bond is L1 too. That rung
  is a person's.
- Nobody has opened the desktop window (see [05-view](05-view.md)).
