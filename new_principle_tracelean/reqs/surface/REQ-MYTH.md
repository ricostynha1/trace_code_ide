---
id: REQ-MYTH
title: The modal keymap
refines: [ARCH-CORE-SHELL, ARCH-DETERMINISM]
status: approved
decomposition: complete
clauses:
  totality: Every key in every mode shall have a defined outcome, and no key shall be silently swallowed.
  outcomes_closed: An outcome shall be exactly one of entering a mode, dispatching an action, leaving a mode, or passing the key to the surface.
  escape_pops_one: Escape shall move exactly one level toward the root; any other unbound key outside the root shall return to the root.
  escape_terminates: Repeated Escape shall reach the root mode in finitely many steps, so a mode other than the root without a parent, or with a parent that does not exist, shall be reported at load.
  modes_defined: Every mode named as a transition target, a binding's target or a mode's parent, shall exist.
  actions_defined: Every action named by a binding shall exist.
  actions_reachable: Every action shall be reachable from the root mode by some key sequence.
  whichkey_is_a_query: The list of available keys shall be computed from the keymap and shall not be maintained separately.
  actions_are_commands: An action that changes state shall return commands and shall not mutate state directly.
  keymap_is_data: The keymap shall be data that a user can edit, and an invalid keymap shall be reported at load rather than on the key that triggers it.
---

# The modal keymap

Leader-key modes with a which-key bar are how an editor becomes configurable
without becoming unlearnable: bindings live in data the user can edit, and the
interface explains itself at the moment of use.

This is specified early and precisely because the existing implementation works
poorly — and a mode machine working poorly is not a set of bugs, it is the absence
of these laws. "Sometimes a key does nothing" is `totality` failing. "Escape left
me somewhere unexpected" is `escape_pops_one` failing. "That binding stopped
working after I edited the keymap" is `actions_defined` failing at the wrong time.

A mode machine is a total function from mode and key to one of four outcomes:
among the smallest models here and the most valuable, because once the laws hold
the failures are unavailable rather than rarer.

`whichkey_is_a_query` keeps the help honest — the bar is computed from the data
that dispatches the key, so it cannot describe a binding that does not exist.
`actions_are_commands` connects this to [REQ-CMD](../history/REQ-CMD.md): every
binding is undoable without the keymap knowing undo exists.
