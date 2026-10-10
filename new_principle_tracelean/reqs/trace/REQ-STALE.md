---
id: REQ-STALE
title: Staleness and invalidation
refines: [ARCH-HONEST, ARCH-CORE-SHELL, ARCH-DETERMINISM]
status: approved
decomposition: complete
clauses:
  inputs_identified: An evidence record shall identify every input it depended on.
  change_invalidates: A record shall be invalid once any input it depended on has changed.
  retarget_invalidates: Evidence keyed to a link shall not be inherited by a link that has been retargeted.
  scanner_never_writes: The scanner shall mark a record stale and shall never rewrite, regenerate or delete one.
  stale_is_visible: A stale record shall be reported as stale rather than omitted.
  no_silent_revalidation: A record shall not become valid again except by the backend that owns it producing a new one.
  agreed_not_rerun: A differential run whose record is still valid shall not be run again unless asked; one whose record went stale shall be.
  measured_not_retaken: A coverage measurement taken against Rust sources that hash as they do now shall not be taken again unless asked; one taken against other sources, or none, shall be.
  requirement_reopens_all: Every record about a clause — judgement, differential test, proof — shall carry the hash of that clause's text and narrowings (of the body, for a requirement without clauses), so rewording or narrowing the clause re-opens all of them and rewording another clause re-opens none.
  listed_for_a_script: Everything that must be approved, tested or proved again shall be listable by one command whose exit status says whether anything is.
---

# Staleness and invalidation

The soundness core. Everything else decides what a claim is worth; this decides
when it stops being worth it, and getting it wrong is worse than having no tool,
because evidence is displayed for code that has since changed.

`change_invalidates` is absolute: no tolerance, no "minor edit" exemption, no
heuristic about whether a change was meaningful. The hash moved, so the claim is
no longer about what was checked.

`retarget_invalidates` closes the subtler hole — evidence keyed only to a clause
would be silently inherited when the annotation moves to a different function.

`scanner_never_writes` is ownership with teeth: a scanner that could regenerate a
record could regenerate one that was never earned.
