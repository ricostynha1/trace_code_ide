import TraceLean.Keymap

namespace TraceLean.Drive

open TraceLean.Keymap

open Lean (ToJson FromJson)

structure Step where
  key : String
  mode : String
  menu : List (String × String) := []
  deriving Repr, DecidableEq, Inhabited, ToJson, FromJson

/-- @models REQ-DRIVE.session_is_a_value
@models REQ-DRIVE.walk_follows_the_machine
@models REQ-DRIVE.menu_is_the_bar_there -/
def drive (keymap : Keymap) (mode : String) : List String → List Step
  | [] => []
  | key :: rest =>
    let landed := nextMode keymap mode key
    { key := key, mode := landed, menu := whichKey keymap landed }
      :: drive keymap landed rest

def modesVisited (steps : List Step) : List String :=
  steps.map (·.mode)

/-- @proves REQ-DRIVE.session_is_a_value -/
theorem an_empty_session_has_no_steps (keymap : Keymap) (mode : String) :
    drive keymap mode [] = [] := rfl

/-- @proves REQ-DRIVE.session_is_a_value -/
theorem a_session_answers_once_per_key (keymap : Keymap) (mode : String)
    (keys : List String) : (drive keymap mode keys).length = keys.length := by
  induction keys generalizing mode
  case nil => rfl
  case cons key rest ih => simp [drive, ih]

/-- @proves REQ-DRIVE.walk_follows_the_machine -/
theorem the_first_step_is_one_step_of_the_machine
    (keymap : Keymap) (mode key : String) (rest : List String) :
    ((drive keymap mode (key :: rest)).head?).map (·.mode)
      = some (nextMode keymap mode key) := rfl

/-- @proves REQ-DRIVE.menu_is_the_bar_there -/
theorem every_step_shows_the_bar_of_the_mode_it_reached (keymap : Keymap) (mode : String)
    (keys : List String) :
    (drive keymap mode keys).all (fun s => s.menu == whichKey keymap s.mode) = true := by
  induction keys generalizing mode
  case nil => rfl
  case cons key rest ih => simp [drive, ih]

/-- @proves REQ-DRIVE.walk_follows_the_machine -/
theorem a_walk_is_as_long_as_the_session (keymap : Keymap) (mode : String)
    (keys : List String) : (modesVisited (drive keymap mode keys)).length = keys.length := by
  simp [modesVisited, a_session_answers_once_per_key]

end TraceLean.Drive
