import TraceLean.Produce

namespace TraceLean.Welcome

open TraceLean.View
open TraceLean.Produce

def introLines : List String :=
  [ "TraceLean — an editor that knows why your code exists",
    "",
    "Requirements live in reqs/. Code, tests and Lean models name the clause",
    "they serve (@implements, @tests, @models, @proves); the gutter chips and",
    "the letters in the file tree show where each clause is met.",
    "",
    "🔗 on the left shows what the open file claims and what else claims it;",
    "🕸 shows the design, roots first. Put the cursor on a requirement and the",
    "bar at the bottom offers to open it, or to gather what an agent needs to",
    "change it (Space c o). Space shows every key; F1 lists them all.",
    "",
    "Start here" ]

def pushPiece (acc : String × List Span) (piece : String) (role : Role)
    (actions : List String) : String × List Span :=
  let start := acc.1.length
  let marked := role != Role.plain || !actions.isEmpty
  let spans :=
    if marked then acc.2 ++ [({ start := start, stop := start + piece.length, role := role,
                                actions := actions } : Span)]
    else acc.2
  (acc.1 ++ piece, spans)

def pushFolder (acc : String × List Span) (folder : String) : String × List Span :=
  pushPiece (pushPiece acc "\n  " Role.plain []) folder Role.path ["file.open"]

def recentRows (acc : String × List Span) (recent : List String) : String × List Span :=
  recent.foldl pushFolder (pushPiece (pushPiece acc "\n\n" Role.plain []) "Recent" Role.heading [])

def shifted (shift : Nat) (span : Span) : Span :=
  { span with start := span.start + shift, stop := span.stop + shift }

/-- @models REQ-SHOW.recent_reopens -/
def welcome (starts : List MenuEntry) (recent : List String) : Buffer :=
  let menu := menuBuffer "welcome" starts
  let intro := String.intercalate "\n" introLines ++ "\n"
  let shift := intro.length
  let title := (introLines.headD "").length
  let heading := introLines.getLastD ""
  let headingAt := shift - 1 - heading.length
  let spans :=
    [({ start := 0, stop := title, role := Role.heading, actions := [] } : Span),
     ({ start := headingAt, stop := headingAt + heading.length, role := Role.heading,
        actions := [] } : Span)]
      ++ menu.spans.map (shifted shift)
  let text := intro ++ menu.text
  if recent.isEmpty then { menu with text := text, spans := tidy text.length spans }
  else
    let rows := recentRows (text, spans) recent
    { menu with text := rows.1, spans := tidy rows.1.length rows.2 }

def covered (b : Buffer) (s : Span) : String :=
  String.mk ((b.text.toList.drop s.start).take (s.stop - s.start))

theorem length_data (s : String) : s.length = s.data.length := by
  cases s
  rfl

theorem pushPiece_grows (acc : String × List Span) (piece : String) (role : Role)
    (actions : List String) :
    (pushPiece acc piece role actions).1 = acc.1 ++ piece ∧
    ∃ extra, (pushPiece acc piece role actions).2 = acc.2 ++ extra := by
  refine ⟨rfl, ?_⟩
  unfold pushPiece
  simp only
  split
  · exact ⟨_, rfl⟩
  · exact ⟨[], by simp⟩

theorem folders_grow : ∀ (recent : List String) (acc : String × List Span),
    (∃ rest, (recent.foldl pushFolder acc).1.data = acc.1.data ++ rest) ∧
    ∃ extra, (recent.foldl pushFolder acc).2 = acc.2 ++ extra
  | [], acc => ⟨⟨[], by simp⟩, ⟨[], by simp⟩⟩
  | folder :: rest, acc => by
    obtain ⟨⟨r, hr⟩, ⟨e, he⟩⟩ := folders_grow rest (pushFolder acc folder)
    obtain ⟨h1, ⟨e1, he1⟩⟩ := pushPiece_grows acc "\n  " Role.plain []
    obtain ⟨h2, ⟨e2, he2⟩⟩ :=
      pushPiece_grows (pushPiece acc "\n  " Role.plain []) folder Role.path ["file.open"]
    simp only [List.foldl_cons]
    refine ⟨⟨("\n  " ++ folder).data ++ r, ?_⟩, ⟨e1 ++ e2 ++ e, ?_⟩⟩
    · rw [hr]
      simp [pushFolder, h2, h1]
    · rw [he]
      simp [pushFolder, he2, he1]

theorem folder_span (folder : String) (pre post : List String) (acc : String × List Span) :
    ∃ sp ∈ ((pre ++ folder :: post).foldl pushFolder acc).2,
      sp.role = Role.path ∧ sp.actions = ["file.open"] ∧
      ((((pre ++ folder :: post).foldl pushFolder acc).1.data.drop sp.start).take
        (sp.stop - sp.start)) = folder.data := by
  rw [List.foldl_append, List.foldl_cons]
  generalize pre.foldl pushFolder acc = acc1
  obtain ⟨⟨r, hr⟩, ⟨e, he⟩⟩ := folders_grow post (pushFolder acc1 folder)
  rw [hr, he]
  let a := (acc1.1 ++ "\n  ").length
  refine ⟨⟨a, a + folder.length, Role.path, ["file.open"]⟩, ?_, rfl, rfl, ?_⟩
  · simp [pushFolder, pushPiece, a]
  · simp only [pushFolder, pushPiece, a, length_data, String.data_append]
    simp [List.drop_append_eq_append_drop, List.take_append_eq_append_take]
    have e1 : ∀ n m : Nat, n + 3 + m - (n + 3) = m := by intros; omega
    have e2 : ∀ n : Nat, n - (n + 3) = 0 := by intros; omega
    have e3 : ∀ n : Nat, n + 3 - n = 3 := by intros; omega
    simp [e1, e2, e3, List.drop_eq_nil_of_le]

/-- @proves REQ-SHOW.recent_reopens -/
theorem welcome_lists_recent (starts : List MenuEntry) (recent : List String)
    (h : recent ≠ []) :
    ∃ rows : String × List Span,
      (welcome starts recent).text = rows.1 ∧
      (welcome starts recent).spans = tidy rows.1.length rows.2 ∧
      ∀ folder ∈ recent, ∃ sp ∈ rows.2, sp.role = Role.path ∧ sp.actions = ["file.open"] ∧
        (rows.1.data.drop sp.start).take (sp.stop - sp.start) = folder.data := by
  unfold welcome
  simp only [List.isEmpty_iff, h, if_false]
  refine ⟨_, rfl, rfl, ?_⟩
  intro folder hm
  obtain ⟨pre, post, hsplit⟩ := List.append_of_mem hm
  unfold recentRows
  rw [hsplit]
  exact folder_span folder pre post _

end TraceLean.Welcome
