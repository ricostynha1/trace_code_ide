//! Generates the parser from `grammar.js` with the tree-sitter CLI, so the
//! 40 MB `parser.c` is never committed and the grammar is the only source.

use std::{fs, path::PathBuf, process::Command};

fn main() {
    println!("cargo:rerun-if-changed=grammar.js");
    println!("cargo:rerun-if-changed=tree-sitter.json");
    println!("cargo:rerun-if-changed=src/scanner.c");

    let out = PathBuf::from(std::env::var("OUT_DIR").unwrap()).join("generated");
    let src = out.join("src");
    fs::create_dir_all(&src).expect("create the output directory");
    fs::copy("grammar.js", out.join("grammar.js")).expect("copy grammar.js");
    fs::copy("tree-sitter.json", out.join("tree-sitter.json")).expect("copy tree-sitter.json");
    fs::copy("src/scanner.c", src.join("scanner.c")).expect("copy scanner.c");

    let status = Command::new("tree-sitter")
        .arg("generate")
        .current_dir(&out)
        .status()
        .expect("the tree-sitter CLI is needed to build the vendored Lean grammar");
    assert!(status.success(), "tree-sitter generate failed");

    cc::Build::new()
        .include(&src)
        .flag_if_supported("-Wno-unused-parameter")
        .flag_if_supported("-Wno-unused-but-set-variable")
        .flag_if_supported("-Wno-trigraphs")
        .opt_level(2)
        .file(src.join("parser.c"))
        .file(src.join("scanner.c"))
        .compile("parser");
}
