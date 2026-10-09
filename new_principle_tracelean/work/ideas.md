# Ideas

Things worth doing that are not requirements yet. Each says what is wrong now,
what would replace it, and what would have to be true for the change to be an
improvement rather than a rearrangement.

Nothing here is a commitment. A entry that becomes work becomes a requirement
with clauses first, like everything else.

## 0. Eleven names in the model meant two things each — **done**

**Was.** Found by trying to import every model module into one runner. Lake
compiles modules independently, so a fully-qualified name defined twice in
modules that never import each other builds cleanly and collides only when
something imports both. Nothing did, until the shared runner wanted to:

| Name | Was defined in |
|---|---|
| `TraceLean.Role` | `Annotation.lean`, `View.lean` |
| `TraceLean.Outcome` | `Command.lean`, `Keymap.lean` |
| `TraceLean.Record` | `Evidence.lean`, `Staleness.lean` |
| `TraceLean.Verdict` | `Judge.lean`, `Record.lean` |
| `TraceLean.Node` | `Rollup.lean`, `Tree.lean` |
| `TraceLean.Problem` | `Annotation.lean`, `Keymap.lean` |
| `TraceLean.Mark` | `Produce.lean`, `Refinement.lean` |
| `TraceLean.step` | `Keymap.lean`, `Refinement.lean` |
| `TraceLean.classify` | `Policy.lean`, `Transcript.lean` |
| `TraceLean.lookup` | `Requirement.lean`, `Staleness.lean` |
| `TraceLean.insertSorted` | `Requirement.lean`, `Strength.lean` |

Some were the same concept modelled twice (`Record` in `Evidence` and
`Staleness`); some were genuinely different things sharing a word (`Role` is a
span's in `View` and an annotation's in `Annotation`).

**Now.** Every module carries its own namespace under `TraceLean`, so the name a
reader sees is `TraceLean.View.Role`. A module opens the transitive closure of
what it imports, which is what keeps the bodies unchanged: only the declaration
sites moved. The JSON encoding did not move — `toJson`/`fromJson?` derive from
constructor and field names, not from the type's namespace path — so no
generated input or output changed shape.

Every `model.function` in `.tracelean/drt.json` moved with it, and so did every
`harness::lean_runner` call. The checker anchors the new names without help:
`formal/TraceLean/Drive.lean::TraceLean.Drive::drive` resolves, and no claim
became `Imprecise`.

**What it cost.** Every requirement↔model anchor changed, so every evidence
record keyed on one went stale and had to be re-earned. That is the mechanism
working, and it is the reason this was left alone until somebody decided the
models wanted namespacing.

**Still there.** Eleven names collided, and for some of them the right answer is
not two namespaces but one definition. `Record` in `Evidence` and `Staleness` is
one concept modelled twice; namespacing made it legal rather than making it
right. See §7.

## 1. One runner instead of a hundred and thirty-five — **done**

**Was.** Every differential test generated a package of its own and built it.
68 Lean calls, each running `lake build`; 67 Rust calls, each running
`cargo build --release` on a crate that differed from its neighbours only in
which function `main` calls. They cannot run at once — the Lean ones all write
the shared `formal/` package's output — so a global lock serialised them. A full
`--ignored` run took about 1364s across 55 suites, and the serialised builds
were the dominant term.

**Now.** One Lean package and one Rust crate, each carrying every op the binding
file declares, at a path all the suites share and which outlives the run.
`main_lean` and `main_rs` dispatch on the op; `write_if_changed` makes every
later process's build a no-op. The first suite to want a side pays for it, every
later suite finds it done, and a second run finds both done.

The entry lists are not invented: they are read from `.tracelean/drt.json`,
which already declares every binding's import, function and arguments. The Lean
package imports the transitive set of modules those bindings name, which §0 made
possible.

68 + 67 builds, none reused, became 2, each built once and cached across runs.
A full `--ignored` run of 97 tests is **295s cold** — from both shared
directories deleted — and **203s warm**, against about 1364s before.

**And then the no-op builds.** Sharing the runners was not enough on its own.
The suites are fifty-six separate processes and the in-process cache is
per-process, so every one of them took the build lock and ran a `lake build` and
a `cargo build` with nothing to do — serialised, because the lock is what makes
the shared directory safe. What the runner was generated from is now written
beside it, and a process whose inputs match that mark takes no lock at all. Warm
runs went from 234s to 203s and from 371% of sixteen cores to 487%.

**What is left is cargo.** `cargo test` runs test *binaries* one at a time, so
the parallelism a full run gets is bounded by the tests in one suite — a mean of
2.9 here. Running the same binaries concurrently instead takes **86s** at 1150%,
which is what `tools/differential-all.sh` does. Nothing in the project can lift
that bound; it is how cargo runs targets.

**Two things had to be true.** The generated Rust crate now declares an empty
`[workspace]` table, because a crate materialised inside a cargo workspace —
`target/`, most obviously — is otherwise refused for believing it is a member of
a workspace that never declared it. A generator whose output only builds in some
directories is wrong about its own output, so this is a fix rather than a
workaround.

And a call that is *not* a declared binding's own call still builds a package of
its own. That matters: the negative test in `differential_evidence.rs` binds the
wrong function deliberately, and a shared runner that quietly answered it with
the right one would turn a test of the harness into a test of nothing. Both
shared runners refuse a call whose spec does not match the file exactly.

**Also done.** `drt::config::Binding` now carries a `model: Option<ModelSpec>`.
The key had existed in the binding file since the beginning with no field to
land in, so the only reader was a private JSON parse in the test harness and
nothing in Rust checked the model half of a binding at all. Both halves now get
the same shape checks.

## 2. A performance register

**Now.** The suite's cost is known only by watching it. Both numbers in §1 were
measured by hand, once, and nothing would report either moving.

**Change.** A register in `.tracelean/` recording, per op, the seed, the case
count, and the wall-clock of the run that produced the evidence — written by the
same two-phase path that writes the L3 record, since that path already knows all
four. A report reads it and names what got slower.

**True if.** The numbers stay out of the evidence. A duration is not a claim
about a requirement: it cannot raise a level, and a record that carried one
would invite exactly the reading the ladder exists to prevent. The register
sits beside the evidence and is compared against itself over time, never
against a threshold that would make a slow machine look like a failed proof.
The stored value must therefore not enter the lockfile's bytes, or two machines
would produce two lockfiles for the same work.

## 3. Coverage floors that a machine proposes

**Now.** Each floor is a number somebody chose after watching a generator run.
20 of 86 bindings declare floors; the rest reach L1 because nobody has watched
them yet.

**Change.** A mode that runs a generator, reports how often each named situation
occurred, and prints the `floors` block it would write. A person still enters
it — the same rule as judging.

**True if.** The proposal is never applied automatically. A floor the generator
proposed from its own behaviour and then met is a tautology, and it would look
exactly like evidence.

## 4. Shrinking that reports what it removed

**Now.** `RunOptions.shrink_rounds` is 100 everywhere and the shrunk input is
what a divergence reports. Nothing says how much was removed, so a shrinker that
quietly stopped working would still produce a plausible-looking failure.

**Change.** Report the original input beside the shrunk one and the number of
rounds that made progress.

## 5. The judging queue as a report

**Now.** `tracelean-trace --judge REQ.clause` prints one prompt for one clause
named on the command line. Finding which clauses are waiting means reading the
roll-up and comparing by eye.

**Change.** A report listing every clause whose requirement↔model bond is at L1
with a model present — the clauses where a person's reading is the only thing
that would raise the level — ordered by how many implementations depend on them.

## 6. Proof obligations that name their difficulty

**Now.** `REQ-STRENGTH` generates an obligation per clause and one is proved.
The other 127 are `Open`, and nothing distinguishes an obligation that is one
`simp` from finished from one that needs a theory the project does not have.

**Change.** Record, per obligation, whether the generated scaffold's statement
elaborates at all. A statement that does not elaborate is a modelling problem,
not a proving problem, and the two want different people.

## 7. A report that finds two models of one concept

**Now.** §0 found eleven names meaning two things each, and it found them by
accident: somebody tried to import every module at once and the compiler
objected. Nothing looks for it. The opposite case is worse and entirely
invisible — two models of *the same* concept under two different names, which no
compiler will ever complain about. `Record` in `Evidence` and `Staleness` was
one, and it was only noticed because its name happened to collide.

This is the failure mode a coding agent is worst at. An agent asked to model a
new clause reads that clause, writes a function, and does not know the tree
already contains one. Every incentive it has points at adding: adding compiles,
adding passes, and the duplicate reads as thorough. Nothing in the method
currently says *stop, this exists*.

**Change.** A report that names candidate duplicates, and a rule that a new
model must answer it. Three signals, cheap to compute and none of them
conclusive alone:

1. **Same shape.** Two model functions whose argument and result types are
   structurally equal, modulo field names. Read from the declarations, not from
   the text.
2. **Same clauses.** Two functions whose `@models` sets overlap, or whose
   requirements refine a common parent. Two models under one clause is either a
   decomposition somebody meant or a duplicate nobody noticed, and the report
   does not have to know which — it has to ask.
3. **Same behaviour.** The one this project can actually settle. Two functions
   with the same shape already have a schema and a generator between them: run
   both over generated inputs and see whether they ever disagree. Two models
   that never disagree over a met coverage floor are one model, and the report
   can say so with the same evidence a binding produces.

Signal 3 is the point, and it is only available because the machinery for it
already exists. Duplication detection elsewhere is a similarity score over text;
here it can be a differential run between two things that both claim to be
specifications, judged by the same rule that judges everything else.

**For an agent.** The rule to add to `skills/04-models-and-bindings.md` is a
step before writing: run the report, and if it names a candidate, either reuse
it or say in the new model's doc comment why the two are different. The second
half matters as much as the first — "these look alike and here is why they are
not" is a sentence a reviewer can check, and its absence is what let eleven
names drift into meaning two things each.

**True if.** The report never deletes anything and never merges anything. A
machine that decided two models were one would be making the requirement↔model
judgement, which is the bond the method reserves for a person. It reports, and a
person or an agent answers in prose that lives next to the code.

**Cost.** Signal 3 is a differential run per candidate pair, which is the
expensive kind. Worth gating behind a flag and running when a model is added
rather than on every check.
