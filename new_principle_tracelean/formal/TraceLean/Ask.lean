import Lean

/-!
# Asking for a name

Models `REQ-NAV.name_is_asked_for`: what one key does to an answer being typed
on the status line. Text goes into it, Backspace takes the last character,
Enter hands it over (trimmed, and nothing if it is empty), Escape drops it.

Written in the subset the grammar reads (ADR-0008).
-/

namespace TraceLean.Ask

/-- The outcome and the answer: `typing` with what is typed so far, `done` with
the answer to act on, or `cancelled`.

@models REQ-NAV.name_is_asked_for -/
def askStep (answer key : String) : String × String :=
  if key == "Escape" then ("cancelled", answer)
  else if key == "Enter" then
    if answer.trim.isEmpty then ("cancelled", answer) else ("done", answer.trim)
  else if key == "Backspace" then ("typing", String.mk answer.toList.dropLast)
  else if key == "Space" then ("typing", answer ++ " ")
  else if key.length == 1 then ("typing", answer ++ key)
  else ("typing", answer)

end TraceLean.Ask
