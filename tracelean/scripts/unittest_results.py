#!/usr/bin/env python3
"""Run a unittest suite and write TraceLean's normalized test-result file.

unittest has no JSON output of its own, and the three fields TraceLean needs --
a name, an outcome, nothing else -- do not justify a dependency. `cargo test --
--format json` is understood directly; this is the same information for Python.

    unittest_results.py <test directory> <output.json> [pattern]
"""
import json
import sys
import unittest


def main():
    if len(sys.argv) < 3:
        print(__doc__, file=sys.stderr)
        return 2
    directory, output = sys.argv[1], sys.argv[2]
    pattern = sys.argv[3] if len(sys.argv) > 3 else "*_checks.py"

    suite = unittest.defaultTestLoader.discover(directory, pattern=pattern)

    # Collect the names *before* running. A TestSuite drops each test as it
    # finishes -- it replaces the entry with None to free memory -- so walking
    # it afterwards yields a list of Nones and loses every name.
    outcomes = {}

    def walk(tests):
        for test in tests:
            if isinstance(test, unittest.TestSuite):
                walk(test)
            elif test is not None:
                outcomes[test.id()] = "passed"

    walk(suite)

    # A test is passing unless it is named in one of the failure lists. Deriving
    # it this way rather than counting is what keeps "3 of 5 passing" honest
    # when a test errors during setup and never reports at all.
    result = unittest.TextTestRunner(stream=sys.stderr, verbosity=0).run(suite)
    for case, _ in list(result.failures) + list(result.errors):
        outcomes[case.id()] = "failed"
    for case, _ in list(getattr(result, "skipped", [])):
        outcomes[case.id()] = "skipped"

    with open(output, "w", encoding="utf-8") as handle:
        json.dump({"results": outcomes, "source": "unittest"}, handle, indent=2)
    print(f"wrote {len(outcomes)} result(s) to {output}", file=sys.stderr)
    return 0


if __name__ == "__main__":
    sys.exit(main())
