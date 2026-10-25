import TraceLean.Highlight
import TraceLean.Produce

namespace TraceLean.FindingsView

open Lean (ToJson FromJson)
open TraceLean.View
open TraceLean.Produce
open TraceLean.Highlight

structure Found where
  kind : String
  file : String
  line : Nat
  message : String
  deriving Repr, DecidableEq, Inhabited, ToJson, FromJson

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

/-- @models REQ-SHOW.findings_lead_somewhere -/
def findingsView (title : String) (found : List Found) : Buffer :=
  let clean := "clean: nothing to report"
  let done :=
    match found with
    | [] => (clean, [({ start := 0, stop := clean.length, role := Role.entry, actions := [] } : Span)])
    | _ => found.foldl findingRow ("", [])
  { id := "record:" ++ title, kind := .record title, text := done.1,
    spans := tidy done.1.length done.2 }

def linked (b : Buffer) : List (String × List String) :=
  (b.spans.filter (!·.actions.isEmpty)).map
    (fun s => (String.mk ((b.text.toList.drop s.start).take (s.stop - s.start)), s.actions))

/-- @proves REQ-SHOW.findings_lead_somewhere -/
theorem a_finding_links_its_place_and_names :
    linked (findingsView "check" [⟨"dangling", "src/a.rs", 4, "REQ-X.y names nothing; see REQ-Z"⟩])
      = [("src/a.rs:4", ["file.open"]), ("REQ-X.y", ["trace.requirement"]),
         ("REQ-Z", ["trace.requirement"])] := by
  native_decide

end TraceLean.FindingsView
