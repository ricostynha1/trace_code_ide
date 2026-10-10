# Setup, in case of an error

In a TraceLean sandbox everything below is already there: `tracelean-trace` is
on `PATH`, `$TRACELEAN_SKILLS` is this directory, and the toolchains are the
host's (`~/.elan`, `~/.cargo`, `~/.rustup` are writable inside). **Install
nothing up front.** Read this page only when a command fails, and fix only what
the failure names. In a sandbox, prefer reporting a missing tool to installing
it: a host fix outlives your session, and a workaround of yours does not.

## Check in one go

```bash
command -v tracelean-trace lean lake cargo node; echo "skills: $TRACELEAN_SKILLS"
```

| Tool | Needed for | Check |
|---|---|---|
| `tracelean-trace` | everything | `tracelean-trace . \| tail -1` |
| `lean`, `lake` (elan) | building models, `--drt`, `--pins` | `lean --version` |
| `cargo`, `rustc` | `--drt` on Rust code, the project's tests | `cargo --version` |
| `llvm-tools` (rustup) | `--coverage`, the line rule (`TRACELEAN_DRT_LINES=1`) | `ls $(rustc --print sysroot)/lib/rustlib/*/bin/llvm-cov` |
| `node`, Chrome | only the TraceLean tree's own page tests | `node --version` |

## What a failure means

| You see | Cause | Fix |
|---|---|---|
| `tracelean-trace: command not found` | not on `PATH` | In the TraceLean tree: `cargo run -q -p tracelean-core --bin tracelean-trace -- .`. Elsewhere: report it; outside a sandbox, `cargo install --path <tracelean>/crates/core --bin tracelean-trace`. |
| `$TRACELEAN_SKILLS` empty | not started from a sandbox | the skills are `skills/` in the TraceLean source tree |
| ``no `lean` on PATH. Install elan`` | Lean missing | outside a sandbox, install elan; the toolchain comes from the nearest `lean-toolchain` (TraceLean's own: `formal/lean-toolchain`) |
| `the Lean runner did not build` | the model does not compile | `lake build` in the model's package (or `lean <file>`) and fix the first error |
| `unknown constant`/`unknown identifier` on a function that exists in newer Lean | toolchain mismatch | write for the pinned version (e.g. 4.12 has no `List.flatMap`: use `.bind`) |
| `the Rust runner did not compile` | the bound `fn` is not reachable or its types do not derive | read the error; a private `fn` or an argument type without `Deserialize` is the usual cause |
| ``no `llvm-cov` …: run `rustup component add llvm-tools` `` | coverage tools missing | that command (outside a sandbox), or report it |
| `mismatch … against …` from `--drt` | the Lean and Rust signatures do not line up | the listed problems name the argument; align the types, never the model's meaning |
| `Imprecise` on a file you wrote | the vendored grammar stopped reading it | `tracelean-trace . --unparsed <file>`, then write that declaration more plainly (see below) |
| `bwrap: …` | not a TraceLean error | the sandbox itself; report it |

## Lean the checker cannot read

The models are compiled by Lean but *read* by a vendored grammar that is older
than Lean. Where it stops, annotations after that point anchor to the whole
file and the claim is capped. Known to stop it; write these instead:

| Avoid | Write |
|---|---|
| char escapes: `'\x1b'` | `c.toNat == 27` |
| or-patterns: `\| .a \| .b => …` | one arm each |
| `«open»` as a field in a structure instance | rename the field |
| mixed binders: `fun (a : A) b => …` | annotate all or none |
| a multi-line `then` inside a match arm | a helper `def` |

A tuple of three or more crosses to Rust as nested pairs, which no Rust tuple
reads: give the model a `structure` instead.
