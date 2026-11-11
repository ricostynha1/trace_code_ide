# REQ-MODULES: The code's modules are shown with what they serve

- status: approved
- decomposition: open
- refines: REQ-FIGURE, REQ-SHOW

A directory of Rust is the unit: the graph is the one the code itself draws
with `crate::` and crate-name paths, and each module carries the requirements
its files claim, read from the index. A module claiming nothing is code that
nothing asked for, or nothing says so.

The graph is Rust only, and a table, not a drawn node-link figure; that gap,
and the lack of a rule for which module may use which, is why `decomposition`
is open.

## Clauses

- **one_per_directory**: The system shall make one module of each directory that holds Rust source, counting its files, and shall leave out every file under a directory whose name starts with a dot.
- **edges_from_paths**: The system shall give a module an edge to every other module that its code names through a `crate::` path or through the path of another crate of the tree, shall read nothing from comments, and shall give no module an edge to itself.
- **serves_from_claims**: The system shall list on each module, sorted and once each, the requirements that the Rust files in its directory claim.
- **unclaimed_flagged**: The system shall name the modules that claim no requirement.
- **shown_as_text**: The system shall show the modules as a Markdown table of module, files, what it uses and what it serves, with the modules that claim no requirement named beneath it, and shall say so when the tree has no Rust source.
- **drawn_in_overview**: The window shall draw the modules in the overview as a row each — its files, what it uses, what uses it and what it serves — the modules that use others first, and shall say of a module serving no requirement that it claims none.
