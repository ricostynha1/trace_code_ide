import Lean

namespace TraceLean.DocLink

open Lean (ToJson FromJson)

structure DocLink where
  file : String
  target : String
  recordedHash : String
  deriving Repr, DecidableEq, Inhabited, ToJson, FromJson

inductive DocState where
  | current
  | inReview (was now : String)
  | dangling
  deriving Repr, DecidableEq, Inhabited, ToJson, FromJson

def lookupHash (current : List (String × String)) (target : String) : Option String :=
  (current.find? (·.1 == target)).map (·.2)

/-- @models REQ-DOCLINK.hash_moves_review
@models REQ-DOCLINK.dangling_reported -/
def docState (link : DocLink) (current : List (String × String)) : DocState :=
  match lookupHash current link.target with
  | none => .dangling
  | some now => if now == link.recordedHash then .current else .inReview link.recordedHash now

/-- @models REQ-DOCLINK.review_is_not_error -/
def docBlocks : DocState → Bool
  | .dangling => true
  | _ => false

/-- @proves REQ-DOCLINK.review_is_not_error -/
theorem review_is_distinct_and_does_not_block (was now : String) (h : was ≠ now) :
    docState ⟨"d.md", "T", was⟩ [("T", now)] = .inReview was now ∧
    docBlocks (.inReview was now) = false := by
  constructor
  · simp [docState, lookupHash, beq_iff_eq, Ne.symm h]
  · rfl

/-- @proves REQ-DOCLINK.hash_moves_review -/
theorem unmoved_is_current (hash : String) :
    docState ⟨"d.md", "T", hash⟩ [("T", hash)] = .current := by
  simp [docState, lookupHash]

def stripQuotes (s : String) : String :=
  let isQuote (c : Char) := c == '"' || c == '\''
  let chars := s.toList.dropWhile isQuote
  String.mk (chars.reverse.dropWhile isQuote).reverse

def trimRight (s : String) : String :=
  String.mk (s.toList.reverse.dropWhile Char.isWhitespace).reverse

def splitFirst (s : String) (sep : Char) : Option (String × String) :=
  match s.toList.findIdx? (· == sep) with
  | none => none
  | some i => some (String.mk (s.toList.take i), String.mk (s.toList.drop (i + 1)))

def splitLast (s : String) (sep : Char) : Option (String × String) :=
  let chars := s.toList
  match (chars.reverse.findIdx? (· == sep)) with
  | none => none
  | some j =>
    let i := chars.length - 1 - j
    some (String.mk (chars.take i), String.mk (chars.drop (i + 1)))

def docFrontmatter (lines : List String) : Option (List String) :=
  match lines with
  | [] => none
  | first :: rest =>
    if first != "---" || rest.isEmpty then none
    else
      match (rest.drop 1).findIdx? (fun line => line.startsWith "---") with
      | none => none
      | some i => some (rest.take (i + 1))

structure Declaring where
  targets : List String := []
  hashes : List (String × String) := []
  inHashes : Bool := false
  deriving Inhabited

def hashInsert (entries : List (String × String)) (key value : String) :
    List (String × String) :=
  (entries.filter (·.1 != key)) ++ [(key, value)]

def readDocLine (state : Declaring) (raw : String) : Declaring :=
  let line := trimRight raw
  if line.trim.isEmpty then state
  else if line.startsWith " " || line.startsWith "\t" then
    if state.inHashes then
      match splitLast line.trim ':' with
      | some (target, hash) =>
        { state with hashes := hashInsert state.hashes target.trim hash.trim }
      | none => state
    else state
  else
    let state := { state with inHashes := false }
    match splitFirst line ':' with
    | none => state
    | some (key, value) =>
      if key.trim == "describes" then
        let value := value.trim
        if value.startsWith "[" && value.endsWith "]" then
          let inner := (value.drop 1).dropRight 1
          let items := ((inner.splitOn ",").map (fun item => stripQuotes item.trim)).filter
            (!·.isEmpty)
          { state with targets := state.targets ++ items }
        else if !value.isEmpty then
          { state with targets := state.targets ++ [stripQuotes value] }
        else state
      else if key.trim == "described_hash" then
        if value.trim.isEmpty then { state with inHashes := true }
        else if state.targets.length == 1 then
          { state with
            hashes := hashInsert state.hashes (state.targets.getD 0 "") value.trim }
        else state
      else state

/-- @models REQ-DOCLINK.declares_target
@models REQ-DOCLINK.records_hash
@models REQ-DOCLINK.decisions_exempt -/
def declaredIn (lines : List String) : List (String × String) :=
  let lines := (String.intercalate "\n" lines).splitOn "\n"
  match docFrontmatter lines with
  | none => []
  | some frontmatter =>
    let state := frontmatter.foldl readDocLine {}
    state.targets.map (fun target =>
      (target, ((state.hashes.find? (·.1 == target)).map (·.2)).getD ""))

/-- @models REQ-DOCLINK.confirmation_is_human -/
def recordedFrontmatter (targets : List String) (current : List (String × String)) :
    String :=
  let header := "describes: [" ++ String.intercalate ", " targets ++ "]\ndescribed_hash:\n"
  targets.foldl
    (fun out target =>
      let hash := ((current.find? (·.1 == target)).map (·.2)).getD "?"
      out ++ "  " ++ target ++ ": " ++ hash ++ "\n")
    header

def keyOf (raw : String) : Option String :=
  if (trimRight raw).startsWith " " || (trimRight raw).startsWith "\t" then none
  else (splitFirst (trimRight raw) ':').map (fun kv => kv.1.trim)

def HashSrc (raw : String) (v : String) : Prop :=
  (∃ a b, splitLast (trimRight raw).trim ':' = some (a, b) ∧ v = b.trim) ∨
    (∃ a b, splitFirst (trimRight raw) ':' = some (a, b) ∧ v = b.trim)

theorem targets_step (s : Declaring) (raw : String) :
    (readDocLine s raw).targets = s.targets ∨
      (keyOf raw = some "describes" ∧ ∃ items, (readDocLine s raw).targets = s.targets ++ items) := by
  unfold readDocLine
  simp only []
  split
  · exact Or.inl rfl
  · split
    · split <;> (try split) <;> exact Or.inl rfl
    · rename_i _ notIndented
      split
      · exact Or.inl rfl
      · rename_i key value hs
        have k : keyOf raw = some key.trim := by
          simp only [keyOf, hs, Option.map_some']
          rw [if_neg notIndented]
        split
        · rename_i hk
          have kd : keyOf raw = some "describes" := by
            rw [k]; simp only [beq_iff_eq] at hk; rw [hk]
          split
          · exact Or.inr ⟨kd, _, rfl⟩
          · split
            · exact Or.inr ⟨kd, _, rfl⟩
            · exact Or.inl rfl
        · split
          · split
            · exact Or.inl rfl
            · split <;> exact Or.inl rfl
          · exact Or.inl rfl

theorem mem_hash_insert (entries : List (String × String)) (key value : String)
    (kv : String × String) (h : kv ∈ hashInsert entries key value) :
    kv ∈ entries ∨ kv = (key, value) := by
  simp only [hashInsert, List.mem_append, List.mem_filter, List.mem_singleton] at h
  rcases h with h | h
  · exact Or.inl h.1
  · exact Or.inr h

theorem hashes_step (s : Declaring) (raw : String) :
    ∀ kv ∈ (readDocLine s raw).hashes, kv ∈ s.hashes ∨ HashSrc raw kv.2 := by
  intro kv h
  unfold readDocLine at h
  simp only [] at h
  split at h
  · exact Or.inl h
  · split at h
    · split at h
      · split at h
        · rename_i target hash hs
          rcases mem_hash_insert _ _ _ kv h with h | h
          · exact Or.inl h
          · subst h
            exact Or.inr (Or.inl ⟨target, hash, hs, rfl⟩)
        · exact Or.inl h
      · exact Or.inl h
    · split at h
      · exact Or.inl h
      · rename_i key value hs
        split at h
        · split at h
          · exact Or.inl h
          · split at h
            · exact Or.inl h
            · exact Or.inl h
        · split at h
          · split at h
            · exact Or.inl h
            · split at h
              · rcases mem_hash_insert _ _ _ kv h with h | h
                · exact Or.inl h
                · subst h
                  exact Or.inr (Or.inr ⟨key, value, hs, rfl⟩)
              · exact Or.inl h
          · exact Or.inl h

theorem fold_targets (lines : List String) : ∀ s : Declaring, ∀ t ∈ (lines.foldl readDocLine s).targets,
    t ∈ s.targets ∨ ∃ raw ∈ lines, keyOf raw = some "describes" := by
  induction lines with
  | nil => intro s t h; exact Or.inl h
  | cons raw rest ih =>
    intro s t h
    rcases ih (readDocLine s raw) t h with h | ⟨r, hr, kr⟩
    · rcases targets_step s raw with e | ⟨kd, items, e⟩
      · rw [e] at h; exact Or.inl h
      · rw [e, List.mem_append] at h
        rcases h with h | _
        · exact Or.inl h
        · exact Or.inr ⟨raw, List.mem_cons_self _ _, kd⟩
    · exact Or.inr ⟨r, List.mem_cons_of_mem _ hr, kr⟩

theorem fold_hashes (lines : List String) : ∀ s : Declaring, ∀ kv ∈ (lines.foldl readDocLine s).hashes,
    kv ∈ s.hashes ∨ ∃ raw ∈ lines, HashSrc raw kv.2 := by
  induction lines with
  | nil => intro s kv h; exact Or.inl h
  | cons raw rest ih =>
    intro s kv h
    rcases ih (readDocLine s raw) kv h with h | ⟨r, hr, kr⟩
    · rcases hashes_step s raw kv h with h | h
      · exact Or.inl h
      · exact Or.inr ⟨raw, List.mem_cons_self _ _, h⟩
    · exact Or.inr ⟨r, List.mem_cons_of_mem _ hr, kr⟩

theorem frontmatter_sub (lines fm : List String) (h : docFrontmatter lines = some fm) :
    ∀ x ∈ fm, x ∈ lines := by
  intro x hx
  unfold docFrontmatter at h
  split at h
  · cases h
  · rename_i first rest
    split at h
    · cases h
    · split at h
      · cases h
      · cases h
        exact List.mem_cons_of_mem _ (List.mem_of_mem_take hx)

/-- @proves REQ-DOCLINK.declares_target -/
theorem every_declared_target_is_declared_by_a_describes_line (lines : List String) :
    ∀ e ∈ declaredIn lines, ∃ raw ∈ (String.intercalate "\n" lines).splitOn "\n",
      keyOf raw = some "describes" := by
  intro e he
  unfold declaredIn at he
  simp only [] at he
  split at he
  · cases he
  · rename_i fm hfm
    simp only [List.mem_map] at he
    obtain ⟨t, ht, _⟩ := he
    rcases fold_targets fm {} t ht with h | ⟨raw, hr, kr⟩
    · cases h
    · exact ⟨raw, frontmatter_sub _ fm hfm raw hr, kr⟩

/-- @proves REQ-DOCLINK.decisions_exempt -/
theorem a_document_without_a_target_declares_no_hash (lines : List String)
    (noTarget : ∀ raw ∈ (String.intercalate "\n" lines).splitOn "\n", keyOf raw ≠ some "describes") :
    declaredIn lines = [] := by
  cases h : declaredIn lines with
  | nil => rfl
  | cons e rest =>
    obtain ⟨raw, hr, kr⟩ := every_declared_target_is_declared_by_a_describes_line lines e
      (by rw [h]; exact List.mem_cons_self _ _)
    exact absurd kr (noTarget raw hr)

/-- @proves REQ-DOCLINK.records_hash -/
theorem every_declared_hash_is_one_the_document_records (lines : List String) :
    ∀ e ∈ declaredIn lines, e.2 = "" ∨
      ∃ raw ∈ (String.intercalate "\n" lines).splitOn "\n", HashSrc raw e.2 := by
  intro e he
  unfold declaredIn at he
  simp only [] at he
  split at he
  · cases he
  · rename_i fm hfm
    simp only [List.mem_map] at he
    obtain ⟨t, _, rfl⟩ := he
    simp only []
    cases hf : (List.foldl readDocLine {} fm).hashes.find? (·.1 == t) with
    | none => exact Or.inl rfl
    | some kv =>
      right
      have mem := List.mem_of_find?_eq_some hf
      rcases fold_hashes fm {} kv mem with h | ⟨raw, hr, src⟩
      · cases h
      · exact ⟨raw, frontmatter_sub _ fm hfm raw hr, src⟩

theorem join_acc (l : List String) : ∀ acc : String,
    l.foldl (fun r s => r ++ s) acc = acc ++ String.join l := by
  induction l with
  | nil => intro acc; simp [String.join]
  | cons x rest ih =>
    intro acc
    simp only [List.foldl, String.join]
    rw [ih, ih (_ ++ x), String.empty_append, String.append_assoc]

theorem recorded_fold (targets : List String) (current : List (String × String)) :
    ∀ out : String, targets.foldl
      (fun out target =>
        let hash := ((current.find? (·.1 == target)).map (·.2)).getD "?"
        out ++ "  " ++ target ++ ": " ++ hash ++ "\n") out =
      out ++ String.join (targets.map (fun target =>
        "  " ++ target ++ ": " ++ (lookupHash current target).getD "?" ++ "\n")) := by
  induction targets with
  | nil => intro out; simp [String.join]
  | cons t rest ih =>
    intro out
    simp only [List.foldl, ih, List.map_cons, lookupHash]
    simp only [String.join, List.foldl_cons, String.empty_append]
    simp only [join_acc, String.empty_append, String.append_assoc]

/-- @proves REQ-DOCLINK.confirmation_is_human -/
theorem only_a_rerecorded_hash_is_current (targets : List String) (current : List (String × String))
    (link : DocLink) :
    recordedFrontmatter targets current =
      "describes: [" ++ String.intercalate ", " targets ++ "]\ndescribed_hash:\n" ++
        String.join (targets.map (fun target =>
          "  " ++ target ++ ": " ++ (lookupHash current target).getD "?" ++ "\n")) ∧
    (docState link current = .current ↔ lookupHash current link.target = some link.recordedHash) := by
  constructor
  · exact recorded_fold targets current _
  · unfold docState
    cases lookupHash current link.target with
    | none => simp
    | some now =>
      by_cases e : now = link.recordedHash
      · simp [e]
      · have : (now == link.recordedHash) = false := by simp [e]
        simp only [this, Option.some.injEq]
        constructor
        · intro c; cases c
        · intro c; exact absurd c e

end TraceLean.DocLink
