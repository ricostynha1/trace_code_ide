//! Unit test suite for TraceLean backend.
//! Mirrors src-tauri/src/ structure: unit/test_{source_file}.rs

mod unit {
    mod test_commands;
    mod test_state;
    mod test_undo_tree;
    mod test_persistence;
    mod test_trace_graph;
    mod test_requirements;
    mod test_highlight_queries;
}
