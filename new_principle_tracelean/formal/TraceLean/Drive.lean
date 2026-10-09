import TraceLean.Keymap

/-!
# Driving the editor

Models `REQ-DRIVE`. Every arrow of the surface was modelled and the editor was
still unusable, because nothing joined them: a person presses keys one after
another, and the thing that had never been stated is what that sequence does.

A session is a value. A keymap, a mode to start in and a list of keys go in; one
step per key comes out, each carrying the mode that key left the machine in and
the bar offered there. Both are computed from `Keymap.lean` — `nextMode` and
`whichKey` — so this adds no second account of what a key does. What it adds is
the sequence, which is the part a frontend gets wrong.
-/

namespace TraceLean.Drive

open TraceLean.Keymap

open Lean (ToJson FromJson)

/-- What one key press did: the mode it left the machine in, and the bar there.

The key is carried so a step says which press it was without the reader
counting, which matters when a divergence is reported. -/
structure Step where
  key : String
  mode : String
  menu : List (String × String) := []
  deriving Repr, DecidableEq, Inhabited, ToJson, FromJson

/--
A session: one step per key, in order.

`walk_follows_the_machine` is the fold — each key is applied to the mode the
previous one left, never to the mode it started in. `menu_is_the_bar_there` is
the pairing — the bar reported with a key is the bar of the mode that key
reached, not of the mode it was pressed in, because that is what a person sees
after pressing it.

`session_is_a_value` is the signature: data in, data out, nothing read.

@models REQ-DRIVE.session_is_a_value
@models REQ-DRIVE.walk_follows_the_machine
@models REQ-DRIVE.menu_is_the_bar_there
-/
def drive (keymap : Keymap) (mode : String) : List String → List Step
  | [] => []
  | key :: rest =>
    let landed := nextMode keymap mode key
    { key := key, mode := landed, menu := whichKey keymap landed }
      :: drive keymap landed rest

/-- The modes a session passed through, in order. A projection rather than a
second walk: a reader that computed the modes itself could disagree with the
session it is reading. -/
def modesVisited (steps : List Step) : List String :=
  steps.map (·.mode)

/-- Pressing nothing does nothing.

The base case stated on purpose: a session of no keys is no steps, not one step
describing where the editor sat. What a person sees before pressing anything is
not the result of a key press.

@proves REQ-DRIVE.session_is_a_value -/
theorem an_empty_session_has_no_steps (keymap : Keymap) (mode : String) :
    drive keymap mode [] = [] := rfl

/-- A session answers once per key.

Nothing is swallowed and nothing is invented: this is `REQ-MYTH.totality` read
along a sequence rather than at one key, and it is what makes the step at index
`i` the answer for the key at index `i`.

@proves REQ-DRIVE.session_is_a_value -/
theorem a_session_answers_once_per_key (keymap : Keymap) (mode : String)
    (keys : List String) : (drive keymap mode keys).length = keys.length := by
  induction keys generalizing mode
  case nil => rfl
  case cons key rest ih => simp [drive, ih]

/-- The first key lands where the mode machine says it lands.

@proves REQ-DRIVE.walk_follows_the_machine -/
theorem the_first_step_is_one_step_of_the_machine
    (keymap : Keymap) (mode key : String) (rest : List String) :
    ((drive keymap mode (key :: rest)).head?).map (·.mode)
      = some (nextMode keymap mode key) := rfl

/-- The bar a step reports is the bar of the mode that step reached.

Stated for every step rather than for the first: a session that showed the right
menu once and the previous mode's menu thereafter is exactly the drift this
requirement exists to catch, and it agrees with the first-step version.

Written as a decidable check over the list rather than with a quantifier, so
that the statement is a value the grammar can anchor a claim inside (ADR-0008).

@proves REQ-DRIVE.menu_is_the_bar_there -/
theorem every_step_shows_the_bar_of_the_mode_it_reached (keymap : Keymap) (mode : String)
    (keys : List String) :
    (drive keymap mode keys).all (fun s => s.menu == whichKey keymap s.mode) = true := by
  induction keys generalizing mode
  case nil => rfl
  case cons key rest ih => simp [drive, ih]

/-- A session's modes are as long as its keys, so a walk can be compared to a
prediction key by key.

@proves REQ-DRIVE.walk_follows_the_machine -/
theorem a_walk_is_as_long_as_the_session (keymap : Keymap) (mode : String)
    (keys : List String) : (modesVisited (drive keymap mode keys)).length = keys.length := by
  simp [modesVisited, a_session_answers_once_per_key]

end TraceLean.Drive
