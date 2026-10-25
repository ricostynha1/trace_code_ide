import Lean

namespace TraceLean.Ask

/-- @models REQ-NAV.name_is_asked_for -/
def askStep (answer key : String) : String × String :=
  if key == "Escape" then ("cancelled", answer)
  else if key == "Enter" then
    if answer.trim.isEmpty then ("cancelled", answer) else ("done", answer.trim)
  else if key == "Backspace" then ("typing", String.mk answer.toList.dropLast)
  else if key == "Space" then ("typing", answer ++ " ")
  else if key.length == 1 then ("typing", answer ++ key)
  else ("typing", answer)

end TraceLean.Ask
