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

private theorem hear_answered (expected : Nat) (line : String) (n : Nat)
    (output : Option Json) (error : Option String)
    (h : hear expected line = .answered n output error) :
    n = expected ∧ isExclusive output error = true ∧
      ∃ value fields, Json.parse line.trim = .ok value ∧ value.getObj? = .ok fields := by
  unfold hear at h
  split at h
  · cases h
  · rename_i value hparse
    split at h
    · cases h
    · rename_i fields hobj
      split at h
      · cases h
      · rename_i got _
        split at h
        · cases h
        · rename_i heq
          have hg : got = expected := by simpa using heq
          dsimp only at h
          split at h
          · split at h
            · cases h
            · split at h
              · cases h
                exact ⟨hg, by assumption, value, fields, hparse, hobj⟩
              · cases h
          · split at h
            · cases h
              exact ⟨hg, by assumption, value, fields, hparse, hobj⟩
            · cases h

/-- @proves REQ-DRT-PROTO.reply_exclusive -/
theorem a_reply_carries_exactly_one (output : Option Json) (error : Option String) :
    isExclusive output error = true ↔
      (output.isSome = true ∧ error = none) ∨ (output = none ∧ error.isSome = true) := by
  cases output <;> cases error <;> simp [isExclusive]

theorem an_answer_carries_exactly_one (expected : Nat) (line : String) (n : Nat)
    (output : Option Json) (error : Option String) (h : hear expected line = .answered n output error) :
    (output.isSome = true ∧ error = none) ∨ (output = none ∧ error.isSome = true) :=
  (a_reply_carries_exactly_one output error).mp (hear_answered expected line n output error h).2.1

/-- @proves REQ-DRT-PROTO.case_echoed -/
theorem a_reply_answers_the_case_asked (expected : Nat) (line : String) :
    (∀ n output error, hear expected line = .answered n output error → n = expected) ∧
    (∀ e got, hear expected line = .wrongCase e got → e = expected ∧ got ≠ expected) := by
  refine ⟨fun n output error h => (hear_answered expected line n output error h).1, ?_⟩
  intro e got h
  unfold hear at h
  split at h
  · cases h
  · split at h
    · cases h
    · split at h
      · cases h
      · rename_i g _
        split at h
        · rename_i heq
          cases h
          refine ⟨rfl, ?_⟩
          intro hc
          subst hc
          simp at heq
        · dsimp only at h
          split at h
          · split at h
            · cases h
            · split at h <;> cases h
          · split at h <;> cases h

/-- @proves REQ-DRT-PROTO.reply_one_line -/
theorem a_reply_is_one_json_object (expected : Nat) (line : String) :
    (∀ n output error, hear expected line = .answered n output error →
      ∃ value fields, Json.parse line.trim = .ok value ∧ value.getObj? = .ok fields) ∧
    (∀ message, Json.parse line.trim = .error message → hear expected line = .notAReply .notJson) := by
  refine ⟨fun n output error h => (hear_answered expected line n output error h).2.2, ?_⟩
  intro message h
  unfold hear
  rw [h]

private theorem duplicates_fold
    (f : List String × List String → String → List String × List String)
    (hf : ∀ seen dupes op, f (seen, dupes) op =
      if seen.contains op then (seen, if dupes.contains op then dupes else dupes ++ [op])
      else (seen ++ [op], dupes))
    (l seen dupes pre : List String)
    (hs : ∀ x, x ∈ seen ↔ x ∈ pre) (hd : ∀ x, x ∈ dupes ↔ 2 ≤ pre.count x) :
    ∀ x, x ∈ (l.foldl f (seen, dupes)).2 ↔ 2 ≤ (pre ++ l).count x := by
  induction l generalizing seen dupes pre with
  | nil => simpa using hd
  | cons op rest ih =>
    intro x
    simp only [List.foldl_cons, hf]
    have hpre : pre ++ op :: rest = (pre ++ [op]) ++ rest := by simp
    rw [hpre]
    by_cases hc : seen.contains op = true
    · have hmem : op ∈ pre := (hs op).mp (List.elem_iff.mp hc)
      have hpos : 0 < pre.count op := List.count_pos_iff_mem.mpr hmem
      rw [if_pos hc]
      apply ih
      · intro y
        rw [hs y]
        simp only [List.mem_append, List.mem_singleton]
        constructor
        · intro h; exact Or.inl h
        · intro h
          cases h with
          | inl h => exact h
          | inr h => subst h; exact hmem
      · intro y
        rw [List.count_append, List.count_singleton]
        by_cases hy : y = op
        · subst hy
          simp only [beq_self_eq_true, if_true]
          constructor
          · intro _; omega
          · intro _
            split
            · rename_i hdc; exact List.elem_iff.mp hdc
            · simp
        · have hne : (op == y) = false := by simp; exact fun h => hy h.symm
          rw [hne]
          simp only [Bool.false_eq_true, if_false, Nat.add_zero]
          rw [← hd y]
          split
          · exact Iff.rfl
          · simp [hy]
    · have hnot : op ∉ pre := fun h => hc (List.elem_iff.mpr ((hs op).mpr h))
      have hzero : pre.count op = 0 := by
        have := mt (List.count_pos_iff_mem (a := op) (l := pre)).mp hnot
        omega
      rw [if_neg hc]
      apply ih
      · intro y
        simp only [List.mem_append, List.mem_singleton]
        rw [hs y]
      · intro y
        rw [List.count_append, List.count_singleton]
        by_cases hy : y = op
        · subst hy
          simp only [beq_self_eq_true, if_true]
          rw [hd y, hzero]
          omega
        · have hne : (op == y) = false := by simp; exact fun h => hy h.symm
          rw [hne]
          simp only [Bool.false_eq_true, if_false, Nat.add_zero]
          exact hd y

/-- @proves REQ-DRT-PROTO.ops_unique -/
theorem duplicate_ops_are_exactly_the_repeated (ops : List String) :
    (∀ op, op ∈ duplicateOps ops ↔ 2 ≤ ops.count op) ∧
    (duplicateOps ops = [] ↔ ∀ op, ops.count op ≤ 1) := by
  have key : ∀ op, op ∈ duplicateOps ops ↔ 2 ≤ ops.count op := by
    intro op
    unfold duplicateOps
    rw [List.mem_mergeSort]
    refine (duplicates_fold _ ?_ ops [] [] [] (by simp) (by simp) op).trans ?_
    · intro _ _ _; rfl
    · simp
  refine ⟨key, ?_⟩
  constructor
  · intro h op
    have := key op
    rw [h] at this
    simp at this
    omega
  · intro h
    apply List.eq_nil_iff_forall_not_mem.mpr
    intro op hop
    have := (key op).mp hop
    have := h op
    omega

private theorem mem_eraseDups_loop (l r : List String) (x : String) :
    x ∈ List.eraseDups.loop l r ↔ x ∈ l ∨ x ∈ r := by
  induction l generalizing r with
  | nil => simp [List.eraseDups.loop]
  | cons a rest ih =>
    simp only [List.eraseDups.loop]
    split
    · rename_i he
      rw [ih]
      have ha : a ∈ r := List.elem_iff.mp he
      simp only [List.mem_cons]
      constructor
      · intro h; cases h with
        | inl h => exact Or.inl (Or.inr h)
        | inr h => exact Or.inr h
      · intro h
        cases h with
        | inl h => cases h with
          | inl h => subst h; exact Or.inr ha
          | inr h => exact Or.inl h
        | inr h => exact Or.inr h
    · rw [ih]
      simp only [List.mem_cons]
      constructor
      · intro h
        cases h with
        | inl h => exact Or.inl (Or.inr h)
        | inr h => cases h with
          | inl h => exact Or.inl (Or.inl h)
          | inr h => exact Or.inr h
      · intro h
        cases h with
        | inl h => cases h with
          | inl h => exact Or.inr (Or.inl h)
          | inr h => exact Or.inl h
        | inr h => exact Or.inr (Or.inr h)

private theorem nodup_eraseDups_loop (l r : List String) (hr : r.Nodup) :
    (List.eraseDups.loop l r).Nodup := by
  induction l generalizing r with
  | nil =>
    simp only [List.eraseDups.loop]
    unfold List.Nodup
    rw [List.pairwise_reverse]
    exact hr.imp (fun h e => h e.symm)
  | cons a rest ih =>
    simp only [List.eraseDups.loop]
    split
    · exact ih r hr
    · rename_i he
      apply ih
      apply List.nodup_cons.mpr
      refine ⟨?_, hr⟩
      intro hm
      have : List.elem a r = true := List.elem_iff.mpr hm
      simp_all

/-- @proves REQ-DRT-PROTO.runner_shared -/
theorem one_runner_serves_each_language (placed : List Placed) :
    (∀ p ∈ placed, ∃ s ∈ sharedRunners placed, s.language = p.language ∧ p.op ∈ s.ops) ∧
    (∀ s ∈ sharedRunners placed, ∀ op ∈ s.ops,
      ∃ p ∈ placed, p.language = s.language ∧ p.op = op) ∧
    ((sharedRunners placed).map (fun s => s.language)).Nodup := by
  unfold sharedRunners
  refine ⟨?_, ?_, ?_⟩
  · intro p hp
    refine ⟨_, List.mem_map_of_mem _ ?_, rfl, ?_⟩
    · rw [List.mem_mergeSort]
      unfold List.eraseDups
      rw [mem_eraseDups_loop]
      exact Or.inl (List.mem_map_of_mem _ hp)
    · unfold List.eraseDups
      rw [mem_eraseDups_loop]
      left
      apply List.mem_map_of_mem
      simp [hp]
  · intro s hs op hop
    rw [List.mem_map] at hs
    obtain ⟨l, _, rfl⟩ := hs
    unfold List.eraseDups at hop
    rw [mem_eraseDups_loop] at hop
    simp only [List.not_mem_nil, or_false, List.mem_map, List.mem_filter, beq_iff_eq] at hop
    obtain ⟨p, ⟨hp, hl⟩, rfl⟩ := hop
    exact ⟨p, hp, hl, rfl⟩
  · rw [List.map_map]
    have : ((fun (s : Shared) => s.language) ∘ fun l =>
        ({ language := l, ops := ((placed.filter (fun p => p.language == l)).map
          (fun p => p.op)).eraseDups } : Shared)) = id := rfl
    rw [this, List.map_id]
    rw [(List.mergeSort_perm _ _).nodup_iff]
    exact nodup_eraseDups_loop _ _ List.nodup_nil

/-- @proves REQ-DRT.error_is_an_answer -/
theorem an_error_is_compared_not_aborted (model implementation : Option Json)
    (failed : model = none ∨ implementation = none) :
    agree model implementation = true ↔ model = implementation := by
  cases model <;> cases implementation <;> simp_all [agree]

/-- @proves REQ-DRT.falsification_only -/
theorem agreement_is_never_proof (agreed : Bool) :
    drtLevel agreed ≠ Level.L4 ∧ (agreed = true → drtLevel agreed = Level.L3) := by
  cases agreed <;> simp [drtLevel]

private def hexValue (c : Char) : Nat :=
  if c = '0' then 0 else if c = '1' then 1 else if c = '2' then 2 else if c = '3' then 3
  else if c = '4' then 4 else if c = '5' then 5 else if c = '6' then 6 else if c = '7' then 7
  else if c = '8' then 8 else if c = '9' then 9 else if c = 'a' then 10 else if c = 'b' then 11
  else if c = 'c' then 12 else if c = 'd' then 13 else if c = 'e' then 14 else if c = 'f' then 15
  else 0

private theorem hexValue_digitChar : ∀ k : Fin 16, hexValue (Nat.digitChar k.val) = k.val := by
  decide

private def simpleEscape (d : Char) : Char :=
  if d = 'n' then '\n' else if d = 'r' then '\r' else if d = 't' then '\t'
  else if d = 'b' then Char.ofNat 8 else if d = 'f' then Char.ofNat 12 else d

private def unescape : List Char → List Char
  | c :: d :: rest =>
    if c = '\\' then
      if d = 'u' then
        match h : rest with
        | a :: b :: x :: y :: r =>
          Char.ofNat (4096 * hexValue a + 256 * hexValue b + 16 * hexValue x + hexValue y) :: unescape r
        | [] => [c, d]
        | [a] => [c, d, a]
        | [a, b] => [c, d, a, b]
        | [a, b, x] => [c, d, a, b, x]
      else simpleEscape d :: unescape rest
    else c :: unescape (d :: rest)
  | [] => []
  | [c] => [c]
termination_by l => l.length
decreasing_by
  all_goals simp_wf
  all_goals (try subst h)
  all_goals (try simp)
  all_goals omega

private theorem unescape_plain (c : Char) (rest : List Char) (h : c ≠ '\\') :
    unescape (c :: rest) = c :: unescape rest := by
  cases rest with
  | nil =>
    rw [unescape.eq_def]
    show [c] = c :: unescape []
    rw [unescape.eq_def]
  | cons d r => rw [unescape.eq_def]; simp [h]

private theorem unescape_simple (d : Char) (rest : List Char) (h : d ≠ 'u') :
    unescape ('\\' :: d :: rest) = simpleEscape d :: unescape rest := by
  rw [unescape.eq_def]; simp [h]

private theorem unescape_hex (a b x y : Char) (rest : List Char) :
    unescape ('\\' :: 'u' :: a :: b :: x :: y :: rest) =
      Char.ofNat (4096 * hexValue a + 256 * hexValue b + 16 * hexValue x + hexValue y) :: unescape rest := by
  rw [unescape.eq_def]; simp

private theorem digitChar_ne_newline (k : Nat) : Nat.digitChar k ≠ '\n' := by
  simp only [Nat.digitChar]
  by_cases h0 : k = 0 <;> simp [h0]
  by_cases h1 : k = 1 <;> simp [h1]
  by_cases h2 : k = 2 <;> simp [h2]
  by_cases h3 : k = 3 <;> simp [h3]
  by_cases h4 : k = 4 <;> simp [h4]
  by_cases h5 : k = 5 <;> simp [h5]
  by_cases h6 : k = 6 <;> simp [h6]
  by_cases h7 : k = 7 <;> simp [h7]
  by_cases h8 : k = 8 <;> simp [h8]
  by_cases h9 : k = 9 <;> simp [h9]
  by_cases h10 : k = 10 <;> simp [h10]
  by_cases h11 : k = 11 <;> simp [h11]
  by_cases h12 : k = 12 <;> simp [h12]
  by_cases h13 : k = 13 <;> simp [h13]
  by_cases h14 : k = 14 <;> simp [h14]
  by_cases h15 : k = 15 <;> simp [h15]


private theorem char_of_toNat (c : Char) (n : Nat) (h : c.toNat = n) : c = Char.ofNat n := by
  rw [← h, Char.ofNat_toNat]

private theorem unescape_escape (c : Char) (rest : List Char) :
    unescape ((escapeChar c).data ++ rest) = c :: unescape rest := by
  unfold escapeChar
  by_cases h34 : (c.toNat == 34) = true
  · rw [if_pos h34]
    show unescape ('\\' :: '"' :: rest) = _
    have : c = '"' := char_of_toNat c 34 (by simpa using h34)
    subst this
    rw [unescape_simple _ _ (by simp)]
    simp [simpleEscape]
  rw [if_neg h34]
  by_cases h92 : (c.toNat == 92) = true
  · rw [if_pos h92]
    show unescape ('\\' :: '\\' :: rest) = _
    have : c = '\\' := char_of_toNat c 92 (by simpa using h92)
    subst this
    rw [unescape_simple _ _ (by simp)]
    simp [simpleEscape]
  rw [if_neg h92]
  by_cases h10 : (c.toNat == 10) = true
  · rw [if_pos h10]
    show unescape ('\\' :: 'n' :: rest) = _
    have : c = '\n' := char_of_toNat c 10 (by simpa using h10)
    subst this
    rw [unescape_simple _ _ (by simp)]
    simp [simpleEscape]
  rw [if_neg h10]
  by_cases h13 : (c.toNat == 13) = true
  · rw [if_pos h13]
    show unescape ('\\' :: 'r' :: rest) = _
    have : c = '\r' := char_of_toNat c 13 (by simpa using h13)
    subst this
    rw [unescape_simple _ _ (by simp)]
    simp [simpleEscape]
  rw [if_neg h13]
  by_cases h9 : (c.toNat == 9) = true
  · rw [if_pos h9]
    show unescape ('\\' :: 't' :: rest) = _
    have : c = '\t' := char_of_toNat c 9 (by simpa using h9)
    subst this
    rw [unescape_simple _ _ (by simp)]
    simp [simpleEscape]
  rw [if_neg h9]
  by_cases h8 : (c.toNat == 8) = true
  · rw [if_pos h8]
    show unescape ('\\' :: 'b' :: rest) = _
    have : c = Char.ofNat 8 := char_of_toNat c 8 (by simpa using h8)
    subst this
    rw [unescape_simple _ _ (by simp)]
    simp [simpleEscape]
  rw [if_neg h8]
  by_cases h12 : (c.toNat == 12) = true
  · rw [if_pos h12]
    show unescape ('\\' :: 'f' :: rest) = _
    have : c = Char.ofNat 12 := char_of_toNat c 12 (by simpa using h12)
    subst this
    rw [unescape_simple _ _ (by simp)]
    simp [simpleEscape]
  rw [if_neg h12]
  by_cases hlow : c.toNat < 32
  · rw [if_pos hlow]
    show unescape ('\\' :: 'u' :: Nat.digitChar (c.toNat / 4096) :: Nat.digitChar ((c.toNat % 4096) / 256)
      :: Nat.digitChar ((c.toNat % 256) / 16) :: Nat.digitChar (c.toNat % 16) :: rest) = _
    rw [unescape_hex]
    rw [hexValue_digitChar ⟨_, (by omega : c.toNat / 4096 < 16)⟩,
      hexValue_digitChar ⟨_, (by omega : (c.toNat % 4096) / 256 < 16)⟩,
      hexValue_digitChar ⟨_, (by omega : (c.toNat % 256) / 16 < 16)⟩,
      hexValue_digitChar ⟨_, (by omega : c.toNat % 16 < 16)⟩]
    have : 4096 * (c.toNat / 4096) + 256 * ((c.toNat % 4096) / 256) + 16 * ((c.toNat % 256) / 16)
      + c.toNat % 16 = c.toNat := by omega
    simp only [this, Char.ofNat_toNat]
  · rw [if_neg hlow]
    show unescape (c :: rest) = _
    have hne : c ≠ '\\' := by
      intro h
      subst h
      simp at h92
    rw [unescape_plain c rest hne]

private theorem join_data (l : List String) (acc : String) :
    (l.foldl (fun r s => r ++ s) acc).data = acc.data ++ (l.map String.data).join := by
  induction l generalizing acc with
  | nil => simp
  | cons s rest ih => simp [List.foldl_cons, ih, String.data_append]

private theorem quoted_data (s : String) :
    (quoted s).data = '"' :: (((s.data.map escapeChar).map String.data).join ++ ['"']) := by
  unfold quoted String.join
  rw [String.data_append, String.data_append, join_data]
  rfl

private theorem unescape_join (cs : List Char) :
    unescape ((cs.map escapeChar).map String.data).join = cs := by
  induction cs with
  | nil => rw [unescape.eq_def]; rfl
  | cons c rest ih => simp only [List.map_cons, List.join_cons, unescape_escape, ih]

private theorem quoted_injective (a b : String) (h : quoted a = quoted b) : a = b := by
  have hd := congrArg String.data h
  rw [quoted_data, quoted_data] at hd
  have hj := List.append_cancel_right (List.cons.inj hd).2
  apply String.ext
  rw [← unescape_join a.data, ← unescape_join b.data, hj]

private theorem caseLine_parts (number : Nat) (op : String) (input : Json) :
    (caseLine number op input).data =
      (Char.ofNat 123 :: ("\"case\":" ++ toString number ++ ",\"op\":").data) ++ (quoted op).data ++
        ((",\"input\":" ++ written input).data ++ [Char.ofNat 125]) := by
  simp only [caseLine, braced, String.data_append, List.append_assoc]
  simp

private theorem toDigitsCore_digits (base fuel n : Nat) (ds : List Char) (c : Char)
    (h : c ∈ Nat.toDigitsCore base fuel n ds) : c ∈ ds ∨ ∃ k, c = Nat.digitChar k := by
  induction fuel generalizing n ds with
  | zero => exact Or.inl (by simpa [Nat.toDigitsCore] using h)
  | succ f ih =>
    simp only [Nat.toDigitsCore] at h
    split at h
    · rcases List.mem_cons.mp h with h | h
      · exact Or.inr ⟨_, h⟩
      · exact Or.inl h
    · rcases ih _ _ h with h | h
      · rcases List.mem_cons.mp h with h | h
        · exact Or.inr ⟨_, h⟩
        · exact Or.inl h
      · exact Or.inr h

private theorem number_has_no_newline (number : Nat) : '\n' ∉ (toString number).data := by
  intro h
  rcases toDigitsCore_digits 10 (number + 1) number [] '\n' h with h | ⟨k, hk⟩
  · simp at h
  · exact digitChar_ne_newline k hk.symm

private theorem escape_has_no_newline (c : Char) : '\n' ∉ (escapeChar c).data := by
  unfold escapeChar
  by_cases h34 : (c.toNat == 34) = true
  · rw [if_pos h34]; show '\n' ∉ ['\\', '\"']; simp
  rw [if_neg h34]
  by_cases h92 : (c.toNat == 92) = true
  · rw [if_pos h92]; show '\n' ∉ ['\\', '\\']; simp
  rw [if_neg h92]
  by_cases h10 : (c.toNat == 10) = true
  · rw [if_pos h10]; show '\n' ∉ ['\\', 'n']; simp
  rw [if_neg h10]
  by_cases h13 : (c.toNat == 13) = true
  · rw [if_pos h13]; show '\n' ∉ ['\\', 'r']; simp
  rw [if_neg h13]
  by_cases h9 : (c.toNat == 9) = true
  · rw [if_pos h9]; show '\n' ∉ ['\\', 't']; simp
  rw [if_neg h9]
  by_cases h8 : (c.toNat == 8) = true
  · rw [if_pos h8]; show '\n' ∉ ['\\', 'b']; simp
  rw [if_neg h8]
  by_cases h12 : (c.toNat == 12) = true
  · rw [if_pos h12]; show '\n' ∉ ['\\', 'f']; simp
  rw [if_neg h12]
  by_cases hlow : c.toNat < 32
  · rw [if_pos hlow]
    show '\n' ∉ ['\\', 'u', Nat.digitChar (c.toNat / 4096), Nat.digitChar ((c.toNat % 4096) / 256),
      Nat.digitChar ((c.toNat % 256) / 16), Nat.digitChar (c.toNat % 16)]
    have := fun k => Ne.symm (digitChar_ne_newline k)
    simp [this]
  · rw [if_neg hlow]
    show '\n' ∉ [c]
    intro hm
    have : c = '\n' := (by simpa using hm : '\n' = c).symm
    subst this
    simp at h10

private theorem quoted_has_no_newline (s : String) : '\n' ∉ (quoted s).data := by
  rw [quoted_data]
  simp only [List.mem_cons, List.mem_append, List.mem_join, List.mem_map, List.mem_singleton]
  intro h
  rcases h with h | ⟨_, ⟨_, ⟨c, _, rfl⟩, rfl⟩, hm⟩ | h
  · simp at h
  · exact escape_has_no_newline c hm
  · simp at h

/-- @proves REQ-DRT-PROTO.case_one_line -/
theorem a_case_is_one_object_on_one_line (number : Nat) (op : String) (input : Json)
    (inputOneLine : '\n' ∉ (written input).data) :
    '\n' ∉ (caseLine number op input).data ∧
    (caseLine number op input).data.head? = some (Char.ofNat 123) ∧
    (caseLine number op input).data.getLast? = some (Char.ofNat 125) := by
  rw [caseLine_parts]
  refine ⟨?_, by simp, ?_⟩
  · simp [String.data_append, number_has_no_newline number, quoted_has_no_newline op, inputOneLine]
  · rw [← List.append_assoc]
    exact List.getLast?_concat _

/-- @proves REQ-DRT-PROTO.case_names_op -/
theorem a_case_line_names_its_op (number : Nat) (op op' : String) (input : Json) :
    (∃ before after, (caseLine number op input).data =
      before ++ ",\"op\":".data ++ (quoted op).data ++ after) ∧
    (caseLine number op input = caseLine number op' input → op = op') := by
  refine ⟨⟨Char.ofNat 123 :: ("\"case\":" ++ toString number).data,
    (",\"input\":" ++ written input).data ++ [Char.ofNat 125], ?_⟩, ?_⟩
  · rw [caseLine_parts]
    simp only [String.data_append, List.append_assoc, List.cons_append]
  · intro h
    have hd := congrArg String.data h
    rw [caseLine_parts, caseLine_parts] at hd
    exact quoted_injective op op' (String.ext (List.append_cancel_left (List.append_cancel_right hd)))

end TraceLean.Protocol
