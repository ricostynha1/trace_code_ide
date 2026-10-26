---
id: REQ-STALE
title: Staleness and invalidation
refines: [ARCH-HONEST, ARCH-CORE-SHELL, ARCH-DETERMINISM]
status: approved
decomposition: complete
clauses:
  inputs_identified: The system shall have every evidence record identify every input it depended on.
  change_invalidates: When any input a record depended on has changed, the system shall treat the record as invalid.
  retarget_invalidates: When a link has been retargeted, the system shall not let it inherit evidence keyed to it.
  scanner_never_writes: The system shall have the scanner mark a record stale and never rewrite, regenerate or delete one.
  stale_is_visible: The system shall report a stale record as stale rather than omit it.
  no_silent_revalidation: The system shall make a record valid again only when the backend that owns it produces a new one.
  agreed_not_rerun: While a differential run's record is still valid, the system shall not run it again unless asked, and when the record has gone stale, the system shall run it.
  measured_not_retaken: While a coverage measurement was taken against Rust sources that hash as they do now, the system shall not take it again unless asked, and where it was taken against other sources, or never, the system shall take it.
  requirement_reopens_all: The system shall have every record about a clause, whether judgement, differential test or proof, carry the hash of that clause's text and narrowings (of the body, for a requirement without clauses), so that rewording or narrowing the clause re-opens all of them and rewording another clause re-opens none.
  listed_for_a_script: The system shall list everything that must be approved, tested or proved again by one command whose exit status says whether anything is.
---

# Staleness and invalidation

Getting staleness wrong is worse than having no tool, because evidence is
displayed for code that has since changed. Invalidation is absolute: no
tolerance, no minor-edit exemption, no heuristic about whether a change was
meaningful; the hash moved, so the claim is no longer about what was checked.
Evidence keyed only to a clause would be silently inherited when the annotation
moves to a different function, and a scanner that could regenerate a record
could regenerate one that was never earned.
