# tree-sitter-lean4, as TraceLean reads Lean

A tree-sitter grammar that reads Lean 4 at the level of its commands: which
declarations a file makes, where each begins and ends, under which name and
namespace, and where its comments and string literals are. A command's body is
a run of tokens up to the next command, so a body parses whatever notation,
tactics or macros it uses. Why, and what is still unreadable, is in
[ADR-0008](../../docs/decisions/ADR-0008-lean-grammar-limits.md).

- `grammar.js`: the commands, the tokens a body is made of, and syntax
  quotations `` `(…) ``, whose parentheses are matched.
- `src/scanner.c`: where a body ends (the first word of the next line decides),
  and nested block comments.
- `src/parser.c.gz`, `src/grammar.hash`: the generated parser and the FNV-1a of
  the `grammar.js` it came from.

## Changing the grammar

```bash
cp grammar.js tree-sitter.json "$work"/ && mkdir -p "$work/src" && cp src/scanner.c "$work/src/"
(cd "$work" && tree-sitter generate)          # under a second
gzip -9 -c "$work/src/parser.c" > src/parser.c.gz
```

then write the new hash to `src/grammar.hash` (the FNV-1a that `build.rs`
computes; `crates/core/tests/lean_grammar.rs` fails while they disagree). A
build with `TRACELEAN_GENERATE_LEAN_PARSER=1` generates the parser itself and
names where it put it.

`tree-sitter parse File.lean` in `$work` shows the tree of one file. The
measure of a change is Lean's own sources:

```bash
cargo test -p tracelean-core --test lean_grammar -- --include-ignored
```

parses every file of the toolchain named by `formal/lean-toolchain` and checks
that each line starting a named declaration (outside comments, strings and
quotations) is one the grammar found, and that it found no other.

The highlighting queries are `assets/languages/lean4/highlights.scm`.

## License

MIT.
