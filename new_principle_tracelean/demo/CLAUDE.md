# This project is traced with TraceLean

Requirements are in `reqs/`; code, tests and Lean models (`specs/`) name the
clause they serve with `@implements`, `@tests`, `@models`, `@proves`.

Before changing anything:

1. Read the TraceLean skills: `$TRACELEAN_SKILLS/README.md` first (set in the
   sandbox; otherwise `skills/` in the TraceLean source tree).
2. Gather what the change touches:
   ```bash
   tracelean-trace . --context REQ-THERMO.to_celsius        # one clause
   tracelean-trace . --context REQ-THERMO --parts all       # the whole requirement
   ```
3. When done, run `tracelean-trace .` and report what it says.
