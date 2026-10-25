import Lean

namespace TraceLean.Setting

def isHex (c : Char) : Bool :=
  c.isDigit || (c >= 'a' && c <= 'f') || (c >= 'A' && c <= 'F')

/-- @models REQ-LOOK.setting_edited_in_place -/
def isColour (value : String) : Bool :=
  if value.startsWith "#" then
    let digits := value.toList.drop 1
    [3, 4, 6, 8].contains digits.length && digits.all isHex
  else false

end TraceLean.Setting
