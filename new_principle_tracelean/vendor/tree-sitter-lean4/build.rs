//! Builds the parser from `grammar.js`.
//!
//! Generating `parser.c` takes the tree-sitter CLI about ten minutes and some
//! 24 GB of memory for this grammar, so the generated parser is kept beside the
//! grammar, compressed, with the hash of the grammar it came from. It is used
//! when that hash matches; otherwise the parser is generated again, and
//! `src/parser.c.gz` and `src/grammar.hash` should be refreshed from the output
//! (see `README.md`).

use std::{
    fs,
    io::{Read, Write},
    path::{Path, PathBuf},
    process::Command,
};

/// FNV-1a over the grammar's bytes: enough to tell one grammar from another.
fn grammar_hash(text: &[u8]) -> String {
    let mut hash: u64 = 0xcbf29ce484222325;
    for byte in text {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x00000100000001B3);
    }
    format!("{hash:016x}")
}

fn generate(out: &Path) {
    fs::copy("grammar.js", out.join("grammar.js")).expect("copy grammar.js");
    fs::copy("tree-sitter.json", out.join("tree-sitter.json")).expect("copy tree-sitter.json");
    let status = Command::new("tree-sitter")
        .arg("generate")
        .current_dir(out)
        .status()
        .expect("the tree-sitter CLI is needed to generate the vendored Lean grammar");
    assert!(status.success(), "tree-sitter generate failed");
    println!(
        "cargo:warning=generated a new Lean parser in {}; store it as src/parser.c.gz with src/grammar.hash",
        out.display()
    );
}

fn main() {
    println!("cargo:rerun-if-changed=grammar.js");
    println!("cargo:rerun-if-changed=tree-sitter.json");
    println!("cargo:rerun-if-changed=src/scanner.c");
    println!("cargo:rerun-if-changed=src/parser.c.gz");
    println!("cargo:rerun-if-changed=src/grammar.hash");

    let out = PathBuf::from(std::env::var("OUT_DIR").unwrap()).join("generated");
    let src = out.join("src");
    fs::create_dir_all(&src).expect("create the output directory");
    fs::copy("src/scanner.c", src.join("scanner.c")).expect("copy scanner.c");

    println!("cargo:rerun-if-env-changed=TRACELEAN_GENERATE_LEAN_PARSER");
    let grammar = fs::read("grammar.js").expect("read grammar.js");
    let stored = fs::read_to_string("src/grammar.hash").unwrap_or_default();
    let current = stored.trim() == grammar_hash(&grammar);
    // Generating is asked for, never started by an editor's background check:
    // two at once do not fit in memory. A stored parser older than the grammar
    // is used with a warning, and `tests/lean_grammar.rs` fails until it is
    // refreshed.
    let asked = std::env::var_os("TRACELEAN_GENERATE_LEAN_PARSER").is_some();
    if !current && !asked {
        println!(
            "cargo:warning=src/parser.c.gz is older than grammar.js; build once with TRACELEAN_GENERATE_LEAN_PARSER=1 and store the result"
        );
    }
    if (current || !asked) && Path::new("src/parser.c.gz").exists() {
        let mut compressed = Vec::new();
        fs::File::open("src/parser.c.gz")
            .and_then(|mut f| f.read_to_end(&mut compressed))
            .expect("read parser.c.gz");
        let mut parser = Vec::new();
        flate2::read::GzDecoder::new(&compressed[..]).read_to_end(&mut parser).expect("decompress parser.c.gz");
        fs::File::create(src.join("parser.c")).and_then(|mut f| f.write_all(&parser)).expect("write parser.c");
        // The generated headers the parser includes.
        fs::create_dir_all(src.join("tree_sitter")).expect("create tree_sitter/");
        for header in ["parser.h", "alloc.h", "array.h"] {
            let from = Path::new("src/tree_sitter").join(header);
            if from.exists() {
                fs::copy(&from, src.join("tree_sitter").join(header)).expect("copy a header");
            }
        }
    } else {
        generate(&out);
    }

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
