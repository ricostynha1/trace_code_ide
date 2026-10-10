import TraceLean.Produce

/-!
# The page a project opens on

Models `REQ-SHOW.recent_reopens`: where to start, then the folders opened
before, each a path that opens it as the project.
-/

namespace TraceLean.Welcome

open TraceLean.View
open TraceLean.Produce

/-- What the page says before its starts. -/
def introLines : List String :=
  [ "TraceLean — an editor that knows why your code exists",
    "",
    "Requirements live in reqs/. Code, tests and Lean models name the clause",
    "they serve (@implements, @tests, @models, @proves); the gutter chips and",
    "the letters in the file tree show where each clause is met.",
    "",
    "🔗 on the left shows what the open file claims and what else claims it;",
    "📋 lists the requirements. Put the cursor on a requirement name and the",
    "bar at the bottom offers to open it, or to gather what an agent needs to",
    "change it (Space c o). Space shows every key; F1 lists them all.",
    "",
    "Start here" ]

/-- A piece of text added at the end, marked when it has a role or actions. -/
def pushPiece (acc : String × List Span) (piece : String) (role : Role)
    (actions : List String) : String × List Span :=
  let start := acc.1.length
  let marked := role != Role.plain || !actions.isEmpty
  let spans :=
    if marked then acc.2 ++ [({ start := start, stop := start + piece.length, role := role,
                                actions := actions } : Span)]
    else acc.2
  (acc.1 ++ piece, spans)

/-- One recent folder: a path that opens it. -/
def pushFolder (acc : String × List Span) (folder : String) : String × List Span :=
  pushPiece (pushPiece acc "\n  " Role.plain []) folder Role.path ["file.open"]

/-- The `Recent` heading and a row a folder. -/
def recentRows (acc : String × List Span) (recent : List String) : String × List Span :=
  recent.foldl pushFolder (pushPiece (pushPiece acc "\n\n" Role.plain []) "Recent" Role.heading [])

/-- A menu span moved down past the introduction. -/
def shifted (shift : Nat) (span : Span) : Span :=
  { span with start := span.start + shift, stop := span.stop + shift }

/-- The starts as a menu, and under a `Recent` heading each folder in `recent`
as a path that opens it.

@models REQ-SHOW.recent_reopens -/
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

end TraceLean.Welcome
