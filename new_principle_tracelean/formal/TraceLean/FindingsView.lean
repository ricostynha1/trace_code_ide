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

theorem runWhile_le (cs : List Char) (stop : Nat) (keep : Char → Bool) :
    ∀ (fuel i : Nat), i ≤ stop → runWhile cs stop keep fuel i ≤ stop := by
  intro fuel
  induction fuel with
  | zero => intro i h; exact h
  | succ fuel ih =>
    intro i h
    unfold runWhile
    split
    · rename_i hc
      simp only [Bool.and_eq_true, decide_eq_true_eq] at hc
      exact ih _ hc.1
    · exact h

theorem groups_le (cs : List Char) (stop : Nat) :
    ∀ (fuel : Nat) (state : Nat × Nat), state.1 ≤ stop → (groups cs stop fuel state).1 ≤ stop := by
  intro fuel
  induction fuel with
  | zero => intro state h; exact h
  | succ fuel ih =>
    intro state h
    unfold groups
    split
    · rename_i hc
      simp only [Bool.and_eq_true, decide_eq_true_eq] at hc
      exact ih _ (runWhile_le cs stop _ _ _ (by omega))
    · exact h

theorem referenceAt_bounds {cs : List Char} {start stop e : Nat} (hs : start ≤ stop)
    (h : referenceAt cs start stop = some e) : start < e ∧ e ≤ stop := by
  unfold referenceAt at h
  simp only at h
  have c1 := runWhile_ge cs stop Char.isUpper (stop + 1) start
  have c2 := runWhile_le cs stop Char.isUpper (stop + 1) start hs
  have g1 := groups_ge cs stop (stop + 1) (runWhile cs stop Char.isUpper (stop + 1) start, 0)
  have g2 := groups_le cs stop (stop + 1) (runWhile cs stop Char.isUpper (stop + 1) start, 0) c2
  simp only at g1 g2
  split at h
  · cases h
  · rename_i hcap
    split at h
    · cases h
    · split at h
      · rename_i hp
        simp only [Bool.and_eq_true, decide_eq_true_eq] at hp
        split at h
        · cases h
        · cases h
          have r1 := runWhile_ge cs stop keyChar (stop + 1)
            ((groups cs stop (stop + 1) (runWhile cs stop Char.isUpper (stop + 1) start, 0)).fst + 1)
          have r2 := runWhile_le cs stop keyChar (stop + 1) _ (Nat.le_of_lt hp.1.1)
          omega
      · split at h
        · cases h
        · cases h
          omega

theorem names_fit (cs : List Char) (first stop d : Nat) (role : Role) (acts : List String) :
    ∀ (fuel place : Nat), place ≤ stop →
      Fits (d + place) ((namesFrom cs first stop fuel place).map fun (r : Nat × Nat) =>
        ({ start := d + r.1, stop := d + r.2, role := role, actions := acts } : Span)) (d + stop) ∧
      ∀ r ∈ namesFrom cs first stop fuel place, r.1 < r.2 ∧ r.2 ≤ stop := by
  intro fuel
  induction fuel with
  | zero => intro place h; exact ⟨by show d + place ≤ d + stop; omega, fun r hr => by cases hr⟩
  | succ fuel ih =>
    intro place h
    unfold namesFrom
    split
    · rename_i hp
      split
      · rename_i ended hr
        have b : place < ended ∧ ended ≤ stop := by
          split at hr
          · exact referenceAt_bounds h hr
          · cases hr
        have ⟨f, g⟩ := ih ended b.2
        refine ⟨⟨Nat.le_refl _, by show d + place ≤ d + ended; omega, f⟩, ?_⟩
        intro r hr'
        cases hr' with
        | head => exact b
        | tail _ hr' => exact g r hr'
      · have ⟨f, g⟩ := ih (place + 1) hp
        exact ⟨fits_lower (by omega) f, g⟩
    · exact ⟨by show d + place ≤ d + stop; omega, fun r hr => by cases hr⟩

def Has (acc : String × List Span) (said : String) (acts : List String) : Prop :=
  ∃ s ∈ acc.2, Shows acc.1 s said ∧ s.actions = acts

def Wants (acc : String × List Span) (found : Found) : Prop :=
  Has acc (found.file ++ ":" ++ toString found.line) ["file.open"] ∧
    ∀ r ∈ requirementNames (" " ++ found.message) 0 (" " ++ found.message).length,
      Has acc (String.mk (((" " ++ found.message).toList.drop r.1).take (r.2 - r.1))) ["trace.requirement"]

theorem has_grow {acc next : String × List Span} {more : String} {new : List Span}
    (h1 : next.1 = acc.1 ++ more) (h2 : next.2 = acc.2 ++ new) {said : String} {acts : List String}
    (h : Has acc said acts) : Has next said acts := by
  obtain ⟨s, hs, sh, ha⟩ := h
  exact ⟨s, by rw [h2]; exact List.mem_append_left _ hs, by rw [h1]; exact shows_extend _ sh, ha⟩

theorem split_three (l : List Char) (a b : Nat) (hab : a ≤ b) (hb : b ≤ l.length) :
    l = l.take a ++ (l.drop a).take (b - a) ++ l.drop b ∧ (l.take a).length = a ∧
      ((l.drop a).take (b - a)).length = b - a := by
  refine ⟨?_, by simp; omega, by simp; omega⟩
  rw [List.append_assoc]
  have : l.drop b = (l.drop a).drop (b - a) := by rw [List.drop_drop]; congr 1; omega
  rw [this, List.take_append_drop, List.take_append_drop]

theorem row_step (acc : String × List Span) (found : Found) (fits : Fits 0 acc.2 acc.1.length)
    (strict : ∀ s ∈ acc.2, s.start < s.stop) :
    Fits 0 (findingRow acc found).2 (findingRow acc found).1.length ∧
      (∀ s ∈ (findingRow acc found).2, s.start < s.stop) ∧ Wants (findingRow acc found) found := by
  obtain ⟨sep, hsep⟩ : ∃ sep, (if acc.1.isEmpty then acc.1 else acc.1 ++ "\n") = acc.1 ++ sep := by
    split
    · exact ⟨"", by simp⟩
    · exact ⟨"\n", rfl⟩
  have hrow : findingRow acc found =
      (acc.1 ++ sep ++ (found.kind ++ ": ") ++ (found.file ++ ":" ++ toString found.line) ++
          (" " ++ found.message),
        acc.2 ++ ([{ start := (acc.1 ++ sep).length, stop := (acc.1 ++ sep).length + (found.kind ++ ": ").length,
                      role := Role.entry, actions := [] },
                    { start := (acc.1 ++ sep).length + (found.kind ++ ": ").length,
                      stop := (acc.1 ++ sep).length + (found.kind ++ ": ").length +
                        (found.file ++ ":" ++ toString found.line).length,
                      role := Role.path, actions := ["file.open"] }] ++
          (requirementNames (" " ++ found.message) 0 (" " ++ found.message).length).map
            fun (range : Nat × Nat) =>
              ({ start := (acc.1 ++ sep).length + (found.kind ++ ": ").length +
                    (found.file ++ ":" ++ toString found.line).length + range.1,
                 stop := (acc.1 ++ sep).length + (found.kind ++ ": ").length +
                    (found.file ++ ":" ++ toString found.line).length + range.2,
                 role := Role.requirement, actions := ["trace.requirement"] } : Span))) := by
    unfold findingRow
    simp only [hsep, List.append_assoc, List.cons_append, List.nil_append]
  rw [hrow]
  unfold Wants Has
  have hh : 0 < (found.kind ++ ": ").length := by
    rw [String.length_append]
    have : (": " : String).length = 2 := rfl
    omega
  generalize found.kind ++ ": " = head at hh
  generalize hloc : found.file ++ ":" ++ toString found.line = loc
  generalize hmsg : " " ++ found.message = msg
  have hl : 0 < loc.length := by
    rw [← hloc, String.length_append, String.length_append]
    have : (":" : String).length = 1 := rfl
    omega
  have names := names_fit msg.toList 0 msg.toList.length
    ((acc.1 ++ sep).length + head.length + loc.length) Role.requirement ["trace.requirement"]
    (msg.toList.length + 1) 0 (Nat.zero_le _)
  have hstop : min msg.length msg.toList.length = msg.toList.length := Nat.min_self _
  have hstart : min 0 msg.toList.length = 0 := Nat.zero_min _
  unfold requirementNames
  simp only [hstop, hstart, Nat.add_zero] at names ⊢
  have hlen : ∀ t : String, t.length = t.toList.length := fun _ => rfl
  refine ⟨?_, ?_, ?_, ?_⟩
  · simp only [String.length_append] at names ⊢
    refine fits_append fits ⟨?_, ?_, ?_, ?_, fits_upper names.1 ?_⟩
    all_goals ((try dsimp only); (try rw [hlen msg]); omega)
  · intro s hs
    simp only [List.mem_append, List.mem_cons, List.mem_map, List.mem_singleton] at hs
    rcases hs with hs | (rfl | rfl | hs) | ⟨r, hr, rfl⟩
    · exact strict s hs
    · dsimp only; omega
    · dsimp only; omega
    · cases hs
    · have := (names.2 r hr).1; dsimp only; omega
  · refine ⟨_, List.mem_append_right _ (List.mem_append_left _ (List.mem_cons_of_mem _ (List.mem_singleton_self _))),
      ⟨acc.1 ++ sep ++ head, msg, rfl, by simp only [String.length_append], rfl⟩, rfl⟩
  · intro r hr
    have hb := names.2 r hr
    refine ⟨_, List.mem_append_right _ (List.mem_append_right _ (List.mem_map_of_mem _ hr)), ?_, rfl⟩
    obtain ⟨sp, l1, l2⟩ := split_three msg.toList r.1 r.2 (Nat.le_of_lt hb.1) hb.2
    refine ⟨acc.1 ++ sep ++ head ++ loc ++ String.mk (msg.toList.take r.1), String.mk (msg.toList.drop r.2), ?_, ?_, ?_⟩
    · have : msg = String.mk (msg.toList.take r.1) ++ String.mk ((msg.toList.drop r.1).take (r.2 - r.1)) ++
          String.mk (msg.toList.drop r.2) := by
        cases msg with
        | mk d => exact congrArg String.mk sp
      conv => lhs; rw [this]
      simp only [String.append_assoc]
    · have e : (String.mk (msg.toList.take r.1)).length = r.1 := l1
      simp only [String.length_append, e]
    · have e : (String.mk ((msg.toList.drop r.1).take (r.2 - r.1))).length = r.2 - r.1 := l2
      dsimp only
      simp only [String.length_append, e]
      omega

theorem row_step' (acc : String × List Span) (found : Found) :
    (∃ more, (findingRow acc found).1 = acc.1 ++ more) ∧ (∃ new, (findingRow acc found).2 = acc.2 ++ new) := by
  unfold findingRow
  simp only []
  refine ⟨?_, ⟨_, List.append_assoc _ _ _⟩⟩
  split
  · exact ⟨(found.kind ++ ": ") ++ (found.file ++ ":" ++ toString found.line) ++ (" " ++ found.message),
      by simp only [String.append_assoc]⟩
  · exact ⟨"\n" ++ (found.kind ++ ": ") ++ (found.file ++ ":" ++ toString found.line) ++ (" " ++ found.message),
      by simp only [String.append_assoc]⟩

theorem rows_fold : ∀ (found : List Found) (acc : String × List Span),
    Fits 0 acc.2 acc.1.length → (∀ s ∈ acc.2, s.start < s.stop) →
      Fits 0 (found.foldl findingRow acc).2 (found.foldl findingRow acc).1.length ∧
      (∀ s ∈ (found.foldl findingRow acc).2, s.start < s.stop) ∧
      ∀ f ∈ found, Wants (found.foldl findingRow acc) f := by
  intro found
  induction found with
  | nil => intro acc f s; exact ⟨f, s, fun _ h => by cases h⟩
  | cons x rest ih =>
    intro acc f s
    obtain ⟨f', s', w⟩ := row_step acc x f s
    obtain ⟨F, S, W⟩ := ih _ f' s'
    refine ⟨F, S, ?_⟩
    intro g hg
    cases hg with
    | tail _ hg => exact W g hg
    | head =>
      have keep : ∀ (l : List Found) (a : String × List Span), ∀ said acts, Has a said acts →
          Has (l.foldl findingRow a) said acts := by
        intro l
        induction l with
        | nil => intro a said acts h; exact h
        | cons y ys ihy =>
          intro a said acts h
          obtain ⟨⟨m, e1⟩, ⟨n, e2⟩⟩ := row_step' a y
          exact ihy _ _ _ (has_grow e1 e2 h)
      exact ⟨keep rest _ _ _ w.1, fun r hr => keep rest _ _ _ (w.2 r hr)⟩

/-- @proves REQ-SHOW.findings_lead_somewhere -/
theorem a_finding_leads_to_its_place_and_requirements (title : String) (found : List Found) :
    ∀ f ∈ found,
      (f.file ++ ":" ++ toString f.line, ["file.open"]) ∈ linked (findingsView title found) ∧
      ∀ r ∈ requirementNames (" " ++ f.message) 0 (" " ++ f.message).length,
        (String.mk (((" " ++ f.message).toList.drop r.1).take (r.2 - r.1)), ["trace.requirement"]) ∈
          linked (findingsView title found) := by
  intro f hf
  cases found with
  | nil => cases hf
  | cons x rest =>
    obtain ⟨F, S, W⟩ := rows_fold (x :: rest) ("", []) (Nat.zero_le _) (fun _ h => by cases h)
    have lead : ∀ said acts, acts ≠ [] → Has ((x :: rest).foldl findingRow ("", [])) said acts →
        (said, acts) ∈ linked (findingsView title (x :: rest)) := by
      intro said acts ne ⟨s, hs, sh, ha⟩
      unfold linked findingsView
      simp only []
      have all : ((x :: rest).foldl findingRow ("", [])).2.filter (fun s => decide (s.start < s.stop)) =
          ((x :: rest).foldl findingRow ("", [])).2 := List.filter_eq_self.mpr (fun s hs => by simp [S s hs])
      rw [tidy_of_fits F, all]
      refine List.mem_map.mpr ⟨s, List.mem_filter.mpr ⟨hs, by cases acts <;> simp_all⟩, ?_⟩
      have := textUnder_of_shows (b := { id := "", kind := .record "", text := _, spans := [] }) sh
      unfold textUnder at this
      rw [this, ha]
    exact ⟨lead _ _ (by simp) (W f hf).1, fun r hr => lead _ _ (by simp) ((W f hf).2 r hr)⟩

end TraceLean.FindingsView
