import TraceLean.View
import TraceLean.Keymap
import TraceLean.Act

namespace TraceLean.Offer

open Lean (ToJson FromJson)
open TraceLean.View
open TraceLean.Keymap

structure Offered where
  group : String
  action : String
  target : Option String
  keys : Option String
  deriving Repr, DecidableEq, Inhabited, ToJson, FromJson

def scanBindings (action : String) (route : List String) :
    List (String × Binding) → List String × List (String × List String) →
      String ⊕ (List String × List (String × List String))
  | [], state => .inr state
  | (key, binding) :: rest, (seen, next) =>
    match binding with
    | .dispatch bound _ =>
      if bound == action then .inl (" ".intercalate (route ++ [key]))
      else scanBindings action route rest (seen, next)
    | .enter entered _ =>
      if seen.contains entered then scanBindings action route rest (seen, next)
      else scanBindings action route rest (seen ++ [entered], next ++ [(entered, route ++ [key])])

def scanLevel (keymap : Keymap) (action : String) :
    List (String × List String) → List String × List (String × List String) →
      String ⊕ (List String × List (String × List String))
  | [], state => .inr state
  | (mode, route) :: rest, state =>
    match keymap.mode mode with
    | none => scanLevel keymap action rest state
    | some found =>
      match scanBindings action route found.bindings state with
      | .inl keys => .inl keys
      | .inr state => scanLevel keymap action rest state

def search (keymap : Keymap) (action : String) :
    Nat → List (String × List String) → List String → Option String
  | 0, _, _ => none
  | fuel + 1, frontier, seen =>
    if frontier.isEmpty then none
    else
      match scanLevel keymap action frontier (seen, []) with
      | .inl keys => some keys
      | .inr (seen, next) => search keymap action fuel next seen

def keysFor (keymap : Keymap) (action : String) : Option String :=
  search keymap action (keymap.modes.length + 1) [(keymap.root, [])] [keymap.root]

def under (buffer : Buffer) (offset : Nat) : Option String :=
  let cs := buffer.text.toList
  (buffer.spans.find? (fun span => span.start ≤ offset && offset < span.stop)).map fun span =>
    let start := min span.start cs.length
    String.mk ((cs.drop start).take (min span.stop cs.length - start))

def entry (keymap : Keymap) (group action : String) (target : Option String) : Offered :=
  { group := group, action := action, target := target, keys := keysFor keymap action }

def pointedAt (keymap : Keymap) (here : Option String) : List String → List Offered
  | [] => []
  | action :: rest =>
    let target := if action == "observe.accept_file" || action == "observe.reject_file" then none else here
    let extra :=
      if action == "observe.diff" then
        [entry keymap "here" "observe.accept_file" here, entry keymap "here" "observe.reject_file" here]
      else []
    entry keymap "here" action target :: extra ++ pointedAt keymap here rest

def ofBuffer (keymap : Keymap) (kind : BufferKind) : List Offered :=
  let e := entry keymap "file"
  [e "file.copy_line" none, e "file.copy" none] ++
  match kind with
  | .file path =>
    [e "file.definition" none, e "file.references" none, e "file.save" none,
     e "history.undo" none, e "history.redo" none, e "trace.evidence" none,
     e "file.rename" (some path), e "file.delete" (some path), e "file.copy_path" (some path)]
  | .directory path => [e "file.new" none, e "file.open" (some path)]
  | .review target =>
    [e "observe.accept" none, e "observe.reject" none,
     e "observe.accept_file" (some target), e "observe.reject_file" (some target)]
  | .record title =>
    (if title == "sandbox" then [e "sandbox.new" none, e "sandbox.copy" none, e "observe.start" none] else []) ++
    (if title == "observed" || title == "sandbox" then [e "observe.accept" none, e "observe.reject" none] else []) ++
    (if title == "sandbox" then [e "sandbox.end" none] else [])
  | .menu title =>
    (if title == "design" then [e "design.expand_everything" none, e "design.fold_all" none] else []) ++
    (if title == "requirements" || title == "design" then
      [e "design.open_only" none, e "trace.new_requirement" none] else [])

def places : List String :=
  ["screen.station.project", "screen.station.trace", "screen.station.design", "screen.station.sandbox", "observe.start", "trace.check",
   "trace.findings", "history.tree"]

def known (action : String) : Bool :=
  match TraceLean.Act.dispatch action { kind := BufferKind.menu "", offset := 0, under := none } {} [] with
  | .refuse (.unknownAction _) => false
  | _ => true

/-- @models REQ-ACT.everything_is_offered -/
def offers (buffer : Buffer) (offset : Nat) (keymap : Keymap) : List Offered :=
  let here := under buffer offset
  let declared := actionsAt buffer offset
  let row :=
    match buffer.kind, here with
    | .directory _, some path =>
      (if declared.contains "file.open" then ["file.rename", "file.delete", "file.copy_path"] else []).map
        (fun a => entry keymap "here" a (some path))
    | _, _ => []
  (pointedAt keymap here declared ++ row ++ ofBuffer keymap buffer.kind ++
    ["screen.split.across", "screen.split.down", "screen.close"].map (fun a => entry keymap "panes" a none) ++
    places.map (fun a => entry keymap "go" a none)).filter (fun o => known o.action)

def leader : Keymap :=
  ⟨"Normal", [("Normal", ⟨none, [("Space", .enter "Leader" "leader")]⟩),
              ("Leader", ⟨some "Normal", [("g", .enter "Design" "design")]⟩),
              ("Design", ⟨some "Leader", [("e", .dispatch "design.expand_everything" "all")]⟩)]⟩

def marked : Buffer :=
  { id := "d", kind := .menu "design", text := "▸ REQ-A",
    spans := [⟨0, 1, .entry, ["design.toggle REQ-A", "file.explode"]⟩] }

theorem pointedAt_keys (keymap : Keymap) (here : Option String) (actions : List String) :
    ∀ o ∈ pointedAt keymap here actions, o.keys = keysFor keymap o.action := by
  induction actions with
  | nil => simp [pointedAt]
  | cons a rest ih =>
    intro o ho
    simp only [pointedAt, List.cons_append, List.mem_cons, List.mem_append] at ho
    rcases ho with h | h | h
    · subst h
      rfl
    · split at h <;> simp_all [entry]
      rcases h with h | h <;> subst h <;> rfl
    · exact ih o h

theorem pointedAt_offers (keymap : Keymap) (here : Option String) (actions : List String) :
    ∀ a ∈ actions, ∃ o ∈ pointedAt keymap here actions, o.action = a ∧ o.group = "here" := by
  induction actions with
  | nil => simp
  | cons b rest ih =>
    intro a ha
    simp only [List.mem_cons] at ha
    rcases ha with h | h
    · subst h
      exact ⟨_, List.mem_cons_self _ _, rfl, rfl⟩
    · obtain ⟨o, ho, h1, h2⟩ := ih a h
      exact ⟨o, by simp [pointedAt, ho], h1, h2⟩

theorem ofBuffer_keys (keymap : Keymap) (kind : BufferKind) :
    ∀ o ∈ ofBuffer keymap kind, o.keys = keysFor keymap o.action := by
  cases kind <;> simp [ofBuffer, entry] <;> (repeat' split) <;> simp [or_imp, forall_and]

/-- @proves REQ-ACT.everything_is_offered -/
theorem every_applicable_action_is_offered (buffer : Buffer) (offset : Nat) (keymap : Keymap) :
    (∀ o ∈ offers buffer offset keymap, known o.action = true ∧ o.keys = keysFor keymap o.action) ∧
    (∀ a ∈ actionsAt buffer offset, known a = true →
      ∃ o ∈ offers buffer offset keymap, o.action = a ∧ o.group = "here") ∧
    (∀ o ∈ ofBuffer keymap buffer.kind, known o.action = true → o ∈ offers buffer offset keymap) ∧
    (∀ a ∈ places, known a = true →
      ∃ o ∈ offers buffer offset keymap, o.action = a ∧ o.group = "go") := by
  refine ⟨?_, ?_, ?_, ?_⟩
  · intro o ho
    simp only [offers, List.mem_filter, List.mem_append, List.mem_map] at ho
    refine ⟨ho.2, ?_⟩
    rcases ho.1 with ((((h | h) | h) | h) | h)
    · exact pointedAt_keys keymap _ _ o h
    · split at h
      · simp only [List.mem_map] at h
        obtain ⟨_, _, rfl⟩ := h
        rfl
      · simp at h
    · exact ofBuffer_keys keymap _ o h
    · obtain ⟨_, _, rfl⟩ := h
      rfl
    · obtain ⟨_, _, rfl⟩ := h
      rfl
  · intro a ha hk
    obtain ⟨o, ho, h1, h2⟩ := pointedAt_offers keymap (under buffer offset) _ a ha
    refine ⟨o, ?_, h1, h2⟩
    simp only [offers, List.mem_filter, List.mem_append]
    exact ⟨Or.inl (Or.inl (Or.inl (Or.inl ho))), h1 ▸ hk⟩
  · intro o ho hk
    simp only [offers, List.mem_filter, List.mem_append]
    exact ⟨Or.inl (Or.inl (Or.inr ho)), hk⟩
  · intro a ha hk
    refine ⟨entry keymap "go" a none, ?_, rfl, rfl⟩
    simp only [offers, List.mem_filter, List.mem_append, List.mem_map]
    exact ⟨Or.inr ⟨a, ha, rfl⟩, hk⟩

end TraceLean.Offer
