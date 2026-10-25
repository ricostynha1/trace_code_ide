import TraceLean.Evidence
import Lean

namespace TraceLean.Protocol

open TraceLean.Evidence

open Lean (ToJson FromJson Json)

inductive NotAReply where
  | notJson
  | notAnObject
  | noCaseNumber
  | errorNotAString
  deriving Repr, DecidableEq, Inhabited, ToJson, FromJson

inductive Heard where
  | answered (caseNumber : Nat) (output : Option Json) (error : Option String)
  | wrongCase (expected got : Nat)
  | notAReply (reason : NotAReply)
  | notExclusive (caseNumber : Nat)
  deriving Inhabited, ToJson, FromJson

/-- @models REQ-DRT-PROTO.reply_exclusive -/
def isExclusive (output : Option Json) (error : Option String) : Bool :=
  output.isSome != error.isSome

/-- @models REQ-DRT-PROTO.reply_one_line
@models REQ-DRT-PROTO.case_echoed -/
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

/-- @models REQ-DRT-PROTO.ops_unique -/
def duplicateOps (ops : List String) : List String :=
  let dupes := ops.foldl
    (fun (acc : List String × List String) op =>
      let (seen, dupes) := acc
      if seen.contains op then
        (seen, if dupes.contains op then dupes else dupes ++ [op])
      else (seen ++ [op], dupes))
    ([], [])
  dupes.2.mergeSort (· <= ·)

def Heard.kind : Heard → String
  | .answered .. => "answered"
  | .wrongCase .. => "wrongCase"
  | .notAReply _ => "notAReply"
  | .notExclusive _ => "notExclusive"

private def obj (inner : String) : String :=
  String.mk [Char.ofNat 123] ++ inner ++ String.mk [Char.ofNat 125]

def hexEscape (c : Char) : String :=
  let n := c.toNat
  "\\u" ++ String.mk [Nat.digitChar (n / 4096), Nat.digitChar ((n % 4096) / 256),
    Nat.digitChar ((n % 256) / 16), Nat.digitChar (n % 16)]

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

def quoted (s : String) : String :=
  "\"" ++ String.join (s.toList.map escapeChar) ++ "\""

def braced (inner : String) : String :=
  String.mk [Char.ofNat 123] ++ inner ++ String.mk [Char.ofNat 125]

partial def written : Json → String
  | .null => "null"
  | .bool b => if b then "true" else "false"
  | .num n => n.toString
  | .str s => quoted s
  | .arr items => "[" ++ ",".intercalate (items.toList.map written) ++ "]"
  | .obj fields =>
    let pairs := fields.fold (fun (acc : List String) k v => acc ++ [quoted k ++ ":" ++ written v]) []
    braced (",".intercalate pairs)

/-- @models REQ-DRT-PROTO.case_one_line
@models REQ-DRT-PROTO.case_names_op -/
def caseLine (number : Nat) (op : String) (input : Json) : String :=
  braced ("\"case\":" ++ toString number ++ ",\"op\":" ++ quoted op ++ ",\"input\":" ++ written input)

inductive Event where
  | couldNotStart (reason : String)
  | noReplyInTime
  | pipeBroke (reason : String)
  | read (expected : Nat) (got : Option String)
  deriving Inhabited, ToJson, FromJson

inductive Outcome where
  | cannotStart (reason : String)
  | timedOut
  | died (reason : String)
  | heard (conclusion : Heard)
  deriving Inhabited, ToJson, FromJson

/-- @models REQ-DRT-PROTO.failure_named -/
def outcome : Event → Outcome
  | .couldNotStart reason => .cannotStart reason
  | .noReplyInTime => .timedOut
  | .pipeBroke reason => .died reason
  | .read _ none => .died "closed its pipe"
  | .read expected (some line) => .heard (hear expected line)

def Outcome.answered : Outcome → Bool
  | .heard (.answered ..) => true
  | _ => false

/-- @proves REQ-DRT-PROTO.failure_named -/
theorem a_failed_runner_did_not_answer (reason : String) (n : Nat) :
    (outcome (.couldNotStart reason)).answered = false ∧
    (outcome .noReplyInTime).answered = false ∧
    (outcome (.pipeBroke reason)).answered = false ∧
    (outcome (.read n none)).answered = false := by
  simp [outcome, Outcome.answered]

structure Placed where
  op : String
  language : String
  deriving Inhabited, ToJson, FromJson

structure Shared where
  language : String
  ops : List String
  deriving Inhabited, ToJson, FromJson

/-- @models REQ-DRT-PROTO.runner_shared -/
def sharedRunners (placed : List Placed) : List Shared :=
  let languages := ((placed.map (fun p => p.language)).eraseDups).mergeSort (· <= ·)
  languages.map (fun l =>
    { language := l, ops := ((placed.filter (fun p => p.language == l)).map (fun p => p.op)).eraseDups })

/-- @models REQ-DRT.error_is_an_answer -/
def agree (model implementation : Option Json) : Bool :=
  match model, implementation with
  | some a, some b => a == b
  | none, none => true
  | _, _ => false

/-- @models REQ-DRT.falsification_only -/
def drtLevel (agreed : Bool) : Level :=
  if agreed then Level.L3 else Level.L1

end TraceLean.Protocol
