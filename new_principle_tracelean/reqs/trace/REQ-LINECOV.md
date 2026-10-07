---
id: REQ-LINECOV
title: Line coverage, test by test
refines: [REQ-EVID, ARCH-HONEST]
status: approved
decomposition: complete
clauses:
  per_test: Coverage shall be gathered one test at a time, so each executable line knows which tests ran it and how often.
  uncovered_shown: An executable line no test ran shall be marked beside it in the editor, and pointing at any measured line shall say which tests ran it and how often.
  stale_hidden: Coverage measured against other text of a file shall not be shown for it.
  clause_summary: A clause shall show how many executable lines of its implementing items some test ran, and by how many tests.
---

# Line coverage, test by test

`tracelean-trace . --coverage` builds a Rust project's tests with
`-C instrument-coverage`, runs each test on its own, and keeps per file the
lines LLVM marks executable with the tests that ran them
(`.tracelean/coverage.json`, keyed by the file's hash).

A requirement says *what* should be tested; coverage says what the tests
actually reach. A clause whose implementing function has a line no test runs
is a clause whose tests could all pass while that line is wrong.
