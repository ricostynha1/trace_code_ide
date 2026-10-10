import TraceLean.Highlight
import TraceLean.Produce

/-!
# The checker's findings, each a way to the place it is about

Models `REQ-SHOW.findings_lead_somewhere`: a finding's `path:line` opens the
file there, and each requirement its message names opens that requirement.
-/

namespace TraceLean.FindingsView

open Lean (ToJson FromJson)
open TraceLean.View
open TraceLean.Produce
open TraceLean.Highlight

/-- One finding, as the view needs it; `line` is one-based. -/
structure Found where
  kind : String
  file : String
  line : Nat
  message : String
  deriving Repr, DecidableEq, Inhabited, ToJson, FromJson

/-- A row of the record: `kind: ` as an entry, `path:line` opening the file,
and the message with each requirement it names opening that requirement. -/
def findingRow (acc : String × List Span) (found : Found) : String × List Span :=
  let text := if acc.1.isEmpty then acc.1 else acc.1 ++ "\n"
  let at_ := text.length
  let head := found.kind ++ ": "
  let location := found.file ++ ":" ++ toString found.line
  let message := " " ++ found.message
  let placed := at_ + head.length
  let said := placed + location.length
  let named := (requirementNames message 0 message.length).map fun (range : Nat × Nat) =>
    ({ start := said + range.1, stop := said + range.2, role := Role.requirement,
       actions := ["trace.requirement"] } : Span)
  (text ++ head ++ location ++ message,
   acc.2 ++ [{ start := at_, stop := placed, role := Role.entry, actions := [] },
             { start := placed, stop := said, role := Role.path, actions := ["file.open"] }] ++ named)

/-- The findings as a record titled `title`, one row a finding, or a line
saying there is nothing to report.

@models REQ-SHOW.findings_lead_somewhere -/
def findingsView (title : String) (found : List Found) : Buffer :=
  let clean := "clean: nothing to report"
  let done :=
    match found with
    | [] => (clean, [({ start := 0, stop := clean.length, role := Role.entry, actions := [] } : Span)])
    | _ => found.foldl findingRow ("", [])
  { id := "record:" ++ title, kind := .record title, text := done.1,
    spans := tidy done.1.length done.2 }

end TraceLean.FindingsView
