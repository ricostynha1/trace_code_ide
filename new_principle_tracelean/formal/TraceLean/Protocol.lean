import TraceLean.Evidence
import Lean

/-!
# The conformance protocol

Models `REQ-DRT-PROTO`. One JSON object per line in each direction, and a
handful of ways that can go wrong.

The protocol is the entire coupling between TraceLean and the code it checks, so
it is also the place where a mistake is invisible: a runner that answers the
wrong case, or answers with both an output and an error, produces a comparison
that looks like data. Reading a line is therefore a total function from the line
to a named conclusion, and every conclusion is one somebody can act on.
-/

namespace TraceLean.Protocol

open TraceLean.Evidence

open Lean (ToJson FromJson Json)

/-- Why a line is not a reply. Named rather than a message, because the two
sides of a differential test cannot be expected to phrase an error the same way
— and because a report saying `notAnObject` is more use than one saying
`expected value at line 1 column 1`. -/
inductive NotAReply where
  /-- The line is not JSON at all. -/
  | notJson
  /-- It parsed, but into something other than an object. -/
  | notAnObject
  /-- No `case` field, or one that is not a number. -/
  | noCaseNumber
  /-- An `error` field that is not a string. -/
  | errorNotAString
  deriving Repr, DecidableEq, Inhabited, ToJson, FromJson

/-- What reading one line from a runner concluded.

`wrongCase` is the conclusion the stage-0 implementation could not reach: it
never compared the number it got back with the number it asked, so a runner one
reply behind was read as answering the current case. -/
inductive Heard where
  /-- A well-formed reply to the case that was asked. -/
  | answered (caseNumber : Nat) (output : Option Json) (error : Option String)
  /-- A well-formed reply to a different case. -/
  | wrongCase (expected got : Nat)
  /-- Not a reply. -/
  | notAReply (reason : NotAReply)
  /-- Both an output and an error, or neither. -/
  | notExclusive (caseNumber : Nat)
  deriving Inhabited, ToJson, FromJson

/-- Whether a reply carries exactly one of an output and an error.

`null` is an ordinary answer — a model returning `none` produces exactly that —
so presence is about the field being there, not about it being non-null. The
derived decoder could not make that distinction, which made a legitimate answer
look like a runner that could not speak.

@models REQ-DRT-PROTO.reply_exclusive -/
def isExclusive (output : Option Json) (error : Option String) : Bool :=
  output.isSome != error.isSome

/--
Read one line of a runner's output, given the case that was asked.

@models REQ-DRT-PROTO.reply_one_line
@models REQ-DRT-PROTO.case_echoed
-/
def hear (expected : Nat) (line : String) : Heard :=
  match Json.parse line.trim with
  | .error _ => .notAReply .notJson
  | .ok value =>
    match value.getObj? with
    | .error _ => .notAReply .notAnObject
    | .ok fields =>
      match (Json.getObjVal? value "case").toOption.bind (·.getNat?.toOption) with
      | none => .notAReply .noCaseNumber
      | some got =>
        -- `match` rather than `if … then … else …`: the grammar that reads
        -- these annotations cannot follow a run of `let` bindings in an `else`
        -- (ADR-0008).
        match got == expected with
        | false => .wrongCase expected got
        | true =>
          let output := (fields.find compare "output")
          let errorField := (fields.find compare "error")
          match errorField with
          | some e =>
            match e.getStr? with
            | .error _ => .notAReply .errorNotAString
            | .ok message =>
              if isExclusive output (some message) then .answered got output (some message)
              else .notExclusive got
          | none =>
            if isExclusive output none then .answered got output none
            else .notExclusive got

/-! ## Dispatch

`ops_unique` and `runner_shared`. One runner process serves every binding of
its language, so its dispatch is a table keyed by op. Two bindings with the same
op means the first arm wins and the second requirement is silently answered by
the wrong function — which is what happened in stage 0, where ops defaulted to
`"default"`.
-/

/-- The ops that appear more than once, sorted and without repeats.

Empty is the only acceptable answer. Reporting *which* rather than a bool is
what lets the message name the requirement whose answers were being produced by
somebody else's function.

@models REQ-DRT-PROTO.ops_unique -/
def duplicateOps (ops : List String) : List String :=
  let dupes := ops.foldl
    (fun (acc : List String × List String) op =>
      let (seen, dupes) := acc
      if seen.contains op then
        (seen, if dupes.contains op then dupes else dupes ++ [op])
      else (seen ++ [op], dupes))
    ([], [])
  dupes.2.mergeSort (· <= ·)

/-- Which conclusion was reached, as a name.

`Heard` carries a `Json` payload, which has no decidable equality, so the
conclusions are compared by name where a proof needs to compare them at all.
The name is also what a report shows. -/
def Heard.kind : Heard → String
  | .answered .. => "answered"
  | .wrongCase .. => "wrongCase"
  | .notAReply _ => "notAReply"
  | .notExclusive _ => "notExclusive"

/-- A JSON object written without a literal brace.

The grammar that reads these annotations treats `{` inside a string as the start
of an interpolation and stops there, so the sample replies below are built
rather than written out (ADR-0008). -/
private def obj (inner : String) : String :=
  String.mk [Char.ofNat 123] ++ inner ++ String.mk [Char.ofNat 125]

/-- A reply carrying neither an output nor an error is not a weaker answer; it
is a runner that is not speaking the protocol.

@proves REQ-DRT-PROTO.reply_exclusive -/
theorem neither_is_not_an_answer :
    (hear 0 (obj "\"case\":0")).kind = "notExclusive" := by
  native_decide

/-- A reply to a different case is distinguished from an answer.

@proves REQ-DRT-PROTO.case_echoed -/
theorem a_reply_to_another_case_is_not_an_answer :
    (hear 7 (obj "\"case\":6,\"output\":1")).kind = "wrongCase" := by
  native_decide

/-- An empty line is not a reply, and says which way it failed.

@proves REQ-DRT-PROTO.reply_one_line -/
theorem an_empty_line_is_not_json :
    (hear 0 "").kind = "notAReply" := by
  native_decide

/-- Nothing is duplicated in nothing.

@proves REQ-DRT-PROTO.ops_unique -/
theorem no_ops_no_collisions : duplicateOps [] = [] := by
  native_decide

/-! ## Writing a case

`case_one_line` and `case_names_op`. A case is written as one JSON object, its
keys in the order the protocol is documented in — `case`, `op`, `input` — and
with every character that could end a line escaped. The escaping is spelled
out here, as the implementation's JSON writer does it, rather than borrowed from
`Json.compress`: that writer spells a tab `\u0009` where the implementation's
spells it `\t`, and the line is what is being compared, byte for byte.
-/

/-- A character as a four-digit lower-case hex escape. -/
def hexEscape (c : Char) : String :=
  let n := c.toNat
  "\\u" ++ String.mk [Nat.digitChar (n / 4096), Nat.digitChar ((n % 4096) / 256),
    Nat.digitChar ((n % 256) / 16), Nat.digitChar (n % 16)]

/-- One character as it appears inside a written JSON string. Compared by code
rather than written as a character literal, which the grammar that reads these
annotations does not take for every escape (ADR-0008). -/
def escapeChar (c : Char) : String :=
  let n := c.toNat
  if n == 34 then "\\\""
  else if n == 92 then "\\\\"
  else if n == 10 then "\\n"
  else if n == 13 then "\\r"
  else if n == 9 then "\\t"
  else if n == 8 then "\\b"
  else if n == 12 then "\\f"
  else if n < 32 then hexEscape c
  else String.singleton c

/-- A string as a JSON string literal. -/
def quoted (s : String) : String :=
  "\"" ++ String.join (s.toList.map escapeChar) ++ "\""

/-- Text between braces, built rather than written: the grammar that reads these
annotations takes a brace inside a string for an interpolation (ADR-0008). -/
def braced (inner : String) : String :=
  String.mk [Char.ofNat 123] ++ inner ++ String.mk [Char.ofNat 125]

/-- A JSON value on one line, object keys in ascending order. -/
partial def written : Json → String
  | .null => "null"
  | .bool b => if b then "true" else "false"
  | .num n => n.toString
  | .str s => quoted s
  | .arr items => "[" ++ ",".intercalate (items.toList.map written) ++ "]"
  | .obj fields =>
    let pairs := fields.fold (fun (acc : List String) k v => acc ++ [quoted k ++ ":" ++ written v]) []
    braced (",".intercalate pairs)

/-- The line a case is written as, naming the op it exercises.

@models REQ-DRT-PROTO.case_one_line
@models REQ-DRT-PROTO.case_names_op -/
def caseLine (number : Nat) (op : String) (input : Json) : String :=
  braced ("\"case\":" ++ toString number ++ ",\"op\":" ++ quoted op ++ ",\"input\":" ++ written input)

/-- A line break inside the input is escaped, so the case stays on one line.

@proves REQ-DRT-PROTO.case_one_line -/
theorem a_newline_in_the_input_is_escaped :
    (caseLine 1 "x" (Json.str (String.mk [Char.ofNat 10]))).contains (Char.ofNat 10) = false := by
  native_decide

/-! ## A runner that did not answer

`failure_named`. Reading a line is one thing that can happen when a case is
asked; a runner can also fail to start, say nothing in time, or have its pipe
break or close. Each is concluded as its own outcome, and only a line that
`hear` reads as an answer is one.
-/

/-- What happened when a runner was asked a case. `read` with `none` is the end
of the runner's output: how a process that exited looks from the reading side. -/
inductive Event where
  | couldNotStart (reason : String)
  | noReplyInTime
  | pipeBroke (reason : String)
  | read (expected : Nat) (got : Option String)
  deriving Inhabited, ToJson, FromJson

/-- What one asked case came to. -/
inductive Outcome where
  | cannotStart (reason : String)
  | timedOut
  | died (reason : String)
  | heard (conclusion : Heard)
  deriving Inhabited, ToJson, FromJson

/-- Conclude what an event means.

@models REQ-DRT-PROTO.failure_named -/
def outcome : Event → Outcome
  | .couldNotStart reason => .cannotStart reason
  | .noReplyInTime => .timedOut
  | .pipeBroke reason => .died reason
  | .read _ none => .died "closed its pipe"
  | .read expected (some line) => .heard (hear expected line)

/-- Whether an outcome is an answer. -/
def Outcome.answered : Outcome → Bool
  | .heard (.answered ..) => true
  | _ => false

/-- No failure of the process is an answer, whatever it carries.

@proves REQ-DRT-PROTO.failure_named -/
theorem a_failed_runner_did_not_answer (reason : String) (n : Nat) :
    (outcome (.couldNotStart reason)).answered = false ∧
    (outcome .noReplyInTime).answered = false ∧
    (outcome (.pipeBroke reason)).answered = false ∧
    (outcome (.read n none)).answered = false := by
  simp [outcome, Outcome.answered]

/-! ## One runner per language

`runner_shared`. The plan the harness builds runners from: one entry per
language, each carrying every op of that language once.
-/

/-- One binding's implementation, by op and language. -/
structure Placed where
  op : String
  language : String
  deriving Inhabited, ToJson, FromJson

/-- One runner process: its language and every op it answers. -/
structure Shared where
  language : String
  ops : List String
  deriving Inhabited, ToJson, FromJson

/-- The runner processes a project needs: one per language, in language order,
each answering every op of that language once, in declaration order.

@models REQ-DRT-PROTO.runner_shared -/
def sharedRunners (placed : List Placed) : List Shared :=
  let languages := ((placed.map (fun p => p.language)).eraseDups).mergeSort (· <= ·)
  languages.map (fun l =>
    { language := l, ops := ((placed.filter (fun p => p.language == l)).map (fun p => p.op)).eraseDups })

/-- Two bindings of one language share one runner.

@proves REQ-DRT-PROTO.runner_shared -/
theorem one_language_one_runner :
    (sharedRunners [{ op := "a", language := "rust" }, { op := "b", language := "rust" }]).length = 1 := by
  native_decide

/-! ## Comparing two answers

`REQ-DRT.error_is_an_answer` and `REQ-DRT.falsification_only`.

An error on one side against an output on the other is a divergence, not an
aborted run. Both sides rejecting an input is *agreement*: the model refusing
what the implementation also refuses is the two behaving alike, and treating it
as a failed run would make every partial function untestable.
-/

/-- Two replies agree when they are the same answer.

An absent output on both sides means both refused; the error text itself is not
compared, because two implementations cannot be expected to phrase a refusal the
same way and the phrasing is not the behaviour.

@models REQ-DRT.error_is_an_answer -/
def agree (model implementation : Option Json) : Bool :=
  match model, implementation with
  | some a, some b => a == b
  | none, none => true
  | _, _ => false

/-- The level a differential run establishes.

A run that found no disagreement is evidence at L3 and never higher. It has
failed to falsify, which is not the same as having proved: the next case might
have disagreed, and the ladder has a separate rung for a proof precisely so that
the difference stays visible.

@models REQ-DRT.falsification_only -/
def drtLevel (agreed : Bool) : Level :=
  if agreed then Level.L3 else Level.L1

/-- Both sides refusing is agreement, and it is the case that would otherwise
make every partial function untestable.

@proves REQ-DRT.error_is_an_answer -/
theorem two_refusals_agree : agree none none = true := by
  native_decide

/-- An error against an answer is a divergence, in either direction.

@proves REQ-DRT.error_is_an_answer -/
theorem a_refusal_against_an_answer_is_a_divergence :
    agree none (some (Json.num 1)) = false ∧ agree (some (Json.num 1)) none = false := by
  native_decide

/-- A clean run never reaches the rung a proof occupies.

@proves REQ-DRT.falsification_only -/
theorem finding_nothing_is_not_a_proof : drtLevel true ≠ Level.L4 := by
  native_decide

/-- A case names, in its op, the entry point it exercises, after its number and
before its input.

@proves REQ-DRT-PROTO.case_names_op -/
theorem a_case_names_its_op :
    ((caseLine 3 "Lsp.lower" (Json.num 1)).splitOn "\"case\":3,\"op\":\"Lsp.lower\",\"input\":1").length
      = 2 := by
  native_decide

end TraceLean.Protocol
