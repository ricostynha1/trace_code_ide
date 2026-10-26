---
id: REQ-MYTH
title: The modal keymap
refines: [ARCH-CORE-SHELL, ARCH-DETERMINISM]
status: approved
decomposition: complete
clauses:
  totality: The system shall give every key in every mode a defined outcome, and shall silently swallow no key.
  outcomes_closed: The system shall make each outcome exactly one of entering a mode, dispatching an action, leaving a mode, or passing the key to the surface.
  escape_pops_one: When Escape is pressed, the system shall move exactly one level toward the root, and when any other unbound key is pressed outside the root, the system shall return to the root.
  escape_terminates: The system shall reach the root mode from any mode by repeated Escape in finitely many steps, and shall report at load a mode other than the root with no parent or with a parent that does not exist.
  modes_defined: The system shall require every mode named as a transition target, a binding's target or a mode's parent to exist.
  actions_defined: The system shall require every action named by a binding to exist.
  actions_reachable: The system shall require every action to be reachable from the root mode by some key sequence.
  whichkey_is_a_query: The system shall compute the list of available keys from the keymap, and shall not maintain it separately.
  actions_are_commands: When an action changes state, the system shall have it return commands, and shall not let it mutate state directly.
  keymap_is_data: The system shall keep the keymap as data a user can edit, and if the keymap is invalid, then the system shall report it at load rather than on the key that triggers it.
---

# The modal keymap

Leader-key modes with a which-key bar keep an editor configurable without
making it unlearnable. "Sometimes a key does nothing" is `totality` failing,
"Escape left me somewhere unexpected" is `escape_pops_one`, and "that binding
stopped working after I edited the keymap" is `actions_defined` failing at the
wrong time. A mode machine is a total function from a mode and a key to one of
four outcomes, so once the laws hold those failures cannot happen. The bar is
computed from the data that dispatches the key, so it cannot describe a binding
that does not exist, and every binding returns commands
([REQ-CMD](../history/REQ-CMD.md)), so it can be undone.
