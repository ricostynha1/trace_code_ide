---
id: REQ-LINECOV
title: Line coverage, test by test
refines: [REQ-EVID, ARCH-HONEST]
status: approved
decomposition: complete
clauses:
  per_test: The system shall gather coverage one test at a time, so that each executable line knows which tests ran it and how often.
  uncovered_shown: The system shall mark in the editor, beside it, an executable line no test ran, and when any measured line is pointed at, shall say which tests ran it and how often.
  stale_hidden: If coverage was measured against other text of a file, then the system shall not show it for that file.
  clause_summary: The system shall show for a clause how many executable lines of its implementing items some test ran, and by how many tests.
  requirement_summary: The system shall show for a requirement the same over every item implementing any of its clauses, each line counted once, with the share of lines run.
  ignored_included: Where the ignored tests are asked for, the system shall run them as well, so that a line only they reach counts as run.
  children_counted: When a test runs a program the project builds, the system shall count the lines that program runs as run by that test.
  lines_listed: When such a count is opened, the system shall list, item by item, the tests that ran its lines with how often, and every executable line no test ran as a link to that line.
---

# Line coverage, test by test

`tracelean-trace . --coverage` builds a Rust project's tests with
`-C instrument-coverage`, runs each test on its own, and keeps per file the
lines LLVM marks executable with the tests that ran them
(`.tracelean/coverage.json`, keyed by the file's hash). A clause whose
implementing function has a line no test runs is a clause whose tests could all
pass while that line is wrong.

The differential and Lean suites are marked `#[ignore]` because each one builds
a Lean package or a crate. `--coverage --ignored` runs them too, one at a time
like the rest; without it a line only they reach reads as never run.

A test that drives the terminal frontend or the command line starts the built
program as a child. Each process writes its own profile (`%p` in
`LLVM_PROFILE_FILE`), the profiles of one test are merged, and the export reads
every instrumented program the build produced, so the lines the child ran are
the test's too.
