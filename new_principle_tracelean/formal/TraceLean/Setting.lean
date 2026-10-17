import Lean

/-!
# A setting's value

Models `REQ-LOOK.setting_edited_in_place`: what a colour setting accepts. A
value that is not `#` and three, four, six or eight hexadecimal digits is
refused and changes nothing.

Written in the subset the grammar reads (ADR-0008).
-/

namespace TraceLean.Setting

/-- A hexadecimal digit, of either case. -/
def isHex (c : Char) : Bool :=
  c.isDigit || (c >= 'a' && c <= 'f') || (c >= 'A' && c <= 'F')

/-- Whether a value is a colour a theme can hold.

@models REQ-LOOK.setting_edited_in_place -/
def isColour (value : String) : Bool :=
  if value.startsWith "#" then
    let digits := value.toList.drop 1
    [3, 4, 6, 8].contains digits.length && digits.all isHex
  else false

end TraceLean.Setting
