# Judging advice — REQ-TRANSCRIPT

## Independent review — 2026-09-20

Second opinion, produced by a reviewer who did not write these models. Advice,
not a record (`REQ-JUDGE.advice_is_not_evidence`).

| Clause | Verdict |
|---|---|
| `read_only` | unmodelable |
| `partial_line_held` | agrees |
| `unknown_preserved` | drift |
| `no_interpretation` | unmodelable |
| `absent_is_fine` | drift |

Findings evaluated against the built model with `lake env lean`, not read off
the source.

### `read_only` — unmodelable

Nothing models it; `--judge` says so. A value-to-value function cannot write,
so `readTranscript` satisfies the clause by having no way to break it, and a
model of a read could not tell a reader that also truncated from one that did
not. Already carried as `@structural` in `crates/core/tests/properties.rs`.
This verdict agrees with that and adds nothing.

### `no_interpretation` — unmodelable

Same shape. The clause is a claim about what no code path does with the result,
which is a property of the whole tree, not of a function. Already
`@structural`. Note that this is the clause `ARCH-NO-DRIVING` leans on, so the
strongest bond it will ever have is a structural one — worth knowing when the
requirement leaves draft.

### `unknown_preserved` — drift

Clause: *a record whose shape is not recognised shall be reported as
unrecognised and shall not abort the read.* The second half holds — the fold
never aborts. The first does not.

`classify` reports `unrecognised` only when a line yielded **neither** an event
**nor** usage. A record recognised in one part and unrecognised in another is
reported as neither.

Input, a record of the shape a real agent transcript actually writes — text as
an array of content blocks:

```
{"type":"assistant","message":{"model":"opus","content":[{"type":"text","text":"hello"}],"usage":{"output_tokens":7}}}
```

The clause demands the unrecognised part be reported. `readTranscript`
evaluates to `events := []`, `unrecognised := []`, `usage := [opus, output 7]`.
The text `"hello"` is dropped and nothing anywhere says a shape was not
understood. `strField` wants `content` to be a string; it is an array, so the
event is silently lost, and the presence of `usage` suppresses the
`unrecognised` entry that would have reported it. This is precisely the failure
the module header says it exists to prevent: "a reader that discarded what it
did not understand would silently lose everything about a tool it had not been
taught, and would look like it was working."

Second input, the same defect from the other side:

```
{"note":"hello","usage":123}
```

Not a usage record by any reading — `usage` holds a number. The clause demands
it be reported as unrecognised. `readTranscript` evaluates to `unrecognised :=
[]` and `usage := [{ model := "unknown", 0, 0, 0, 0 }]`. `usageIn` asks only
whether the key is present, and `natField` returns 0 for anything that is not a
number under an object, so any JSON object with a `usage` key of any shape
becomes a usage record. It then reaches `REQ-COST`: `spendOf` reports
`unpriced := ["unknown"]`, so the cost line announces an unpriced model for a
record that reported no usage.

Both cases follow from one line:

```lean
if read.event.isNone && read.usage.isNone then acc.2.1 ++ [line] else acc.2.1
```

The clause is about a record's *shape* not being recognised; the model tests
whether *anything at all* was extracted. The Rust twin has the same conjunction
and the same `holder.get("usage")?`, so the differential tests agree with the
model and cannot see this. Only a reading of the clause does.

### `partial_line_held` — agrees, with two notes

`readTranscript` holds everything after the final newline, and a record that
has not finished being written is exactly such a thing. `no_newline_is_all_held`
is a genuine universal. The clause is met.

*Wider than the words, deliberately, and it costs a record.* The model holds
the trailing segment whether or not it is finished. Input
`{"type":"say","text":"done"}` with no trailing newline evaluates to
`events := []`, `held := "{\"type\":\"say\",\"text\":\"done\"}"`. If the tool
has exited and will never write that newline, the record is held for ever and
never reported. The clause has no converse obligation, so this is not drift —
but it is a real record lost, and nothing in the requirement says what closes a
transcript.

*`readChunks` does not produce the shape its comment claims.* It joins chunks
with `"\n"`, which makes every boundary but the last a *record* boundary.
Evaluated: `readChunks ["{\"type\":\"say\",\"te", "xt\":\"one\"}"]` gives
`unrecognised := ["{\"type\":\"say\",\"te"]`, `held := "xt\":\"one\"}"`. The
generator can only ever cut the final record mid-write; earlier fragments are
manufactured into `unrecognised` entries instead. Since `readChunks` is also
`@drt REQ-TRANSCRIPT.unknown_preserved`, the differential corpus is being fed
unrecognised records that no tool would write, while the mid-write records the
comment promises are one per input.

### `absent_is_fine` — drift

Clause: *a tool that writes no transcript shall be fully supported, with
observation of the workspace alone.* Two obligations. The model speaks to
neither directly.

Input: a session where the transcript file does not exist. The clause demands
the sandbox station be fully supported, working from the workspace diff. The
bound model is `readTranscript : String → TranscriptRead`, which has no input
that means "no file" — `""` is a file that exists and is empty, a different
thing — and no output that mentions a workspace. `empty_is_empty` proves
`readTranscript "" = ⟨[], [], "", []⟩`, which is a statement about the empty
string, not about absence, and one that holds by construction: `"".splitOn "\n"
= [""]`, so no other result was reachable.

Read as a whole-system claim, the clause is nearer `unmodelable` than `drift` —
its second half is the same kind of thing as `read_only` and
`no_interpretation`, a property of the tree rather than of a function. Either
verdict is defensible; what is not defensible is `agrees`, because nothing
under `@models REQ-TRANSCRIPT.absent_is_fine` mentions a workspace at all. The
cleanest repair is the one the sibling clauses already took: split the
modelable half ("an absent transcript is not an error") from the structural
half and mark the second `@structural`, as `crates/core/tests/properties.rs`
does for the other two.

## Independent review — 2026-10-09

Produced by claude-review simulating the human approver. Advice, not a record
(`REQ-JUDGE.advice_is_not_evidence`).

| Clause | Verdict |
|---|---|
| `partial_line_held` | agrees, with a note |
| `unknown_preserved` | agrees |
| `absent_is_fine` | drift |

### `unknown_preserved` — agrees

The 2026-09-20 drift is fixed. A line with no event is now unrecognised when it
claimed a `type` *or* yielded no usage, so the content-block record is reported
(and its usage kept), and `usageIn` requires `usage` to be an object, so
`{"usage":123}` is unrecognised rather than a usage of model `unknown`.

### `partial_line_held` — note

Unchanged from 2026-09-20: met by `readTranscript`; `readChunks` joins with
newlines, so only the last chunk can be cut mid-record, and a final record never
terminated is held forever.

### `absent_is_fine` — drift

Unchanged: `readTranscript` has no input meaning "no transcript" and no output
about the workspace; `empty_is_empty` is about the empty string. The
workspace-alone half is unmodelled.
