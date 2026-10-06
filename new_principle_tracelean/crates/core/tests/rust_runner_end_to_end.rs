//! The stage-0 unblocker, proved end to end: generate a runner against this
//! crate, compile it, and hold a conversation with it over the protocol.
//!
//! The function under test is `drt::signature::parameters` — the differential
//! tester exercising a part of itself, which is the smallest honest version of
//! what this whole port is for.
//!
use std::collections::BTreeMap;
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use tracelean_core::drt::rust_runner::{materialize, package_dir, resolve};
use tracelean_core::drt::{Binding, CallSpec};

fn project_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap().parent().unwrap().to_path_buf()
}

/// @tests REQ-DRT-RUST.generated
/// @tests REQ-DRT-RUST.path_dependency
/// @tests REQ-DRT-RUST.types_inferred
#[test]
#[ignore = "compiles a generated crate; run with --ignored"]
fn generated_runner_answers_cases() {
    let root = project_root();
    let binding = Binding {
        req_id: "REQ-DRT-RUST".into(),
        clause: Some("params_from_source".into()),
        op: None,
        also_checks: Vec::new(),
        also_implemented_by: Vec::new(),
        floors: Vec::new(),
        model: None,
        implementation: CallSpec {
            language: "rust".into(),
            entry: "crates/core/src/drt/signature.rs::parameters".into(),
            params: BTreeMap::new(),
        },
    };

    let entry = resolve(&root, &binding).expect("binding resolves");
    assert_eq!(entry.op, "REQ-DRT-RUST.params_from_source");

    // Build inside a scratch root so the project's own .tracelean is untouched.
    let scratch = std::env::temp_dir().join("tracelean-drt-e2e");
    let _ = std::fs::remove_dir_all(&scratch);
    let mut deps = BTreeMap::new();
    deps.insert(
        "tracelean-core".to_string(),
        root.join("crates").join("core").display().to_string(),
    );
    materialize(&scratch, &[entry], &deps).expect("generated");

    let dir = package_dir(&scratch);
    let built = Command::new("cargo")
        .args(["build", "--release", "--quiet"])
        .current_dir(&dir)
        .output()
        .expect("cargo runs");
    assert!(
        built.status.success(),
        "the generated crate did not compile:\n{}",
        String::from_utf8_lossy(&built.stderr)
    );

    let mut child = Command::new(dir.join("target").join("release").join("tracelean-drt-runner"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .expect("runner starts");
    let mut stdin = child.stdin.take().unwrap();
    let mut stdout = BufReader::new(child.stdout.take().unwrap());

    let ask = |stdin: &mut std::process::ChildStdin,
               stdout: &mut BufReader<std::process::ChildStdout>,
               case: u64,
               source: &str,
               symbol: &str| {
        let line = serde_json::json!({
            "case": case,
            "op": "REQ-DRT-RUST.params_from_source",
            "input": {"source": source, "symbol": symbol},
        });
        writeln!(stdin, "{line}").unwrap();
        stdin.flush().unwrap();
        let mut reply = String::new();
        stdout.read_line(&mut reply).unwrap();
        serde_json::from_str::<serde_json::Value>(&reply).expect("a reply")
    };

    let r = ask(&mut stdin, &mut stdout, 1, "fn f(a: u8, b: Vec<u8>) {}", "f");
    assert_eq!(r["case"], 1);
    assert_eq!(r["output"], serde_json::json!(["a", "b"]));

    // `None` and `Some([])` must stay distinguishable across the wire: one is
    // "no such function", the other "a function taking nothing".
    let r = ask(&mut stdin, &mut stdout, 2, "fn f() {}", "nope");
    assert_eq!(r["output"], serde_json::Value::Null);
    let r = ask(&mut stdin, &mut stdout, 3, "fn f() {}", "f");
    assert_eq!(r["output"], serde_json::json!([]));

    drop(stdin);
    let _ = child.wait();
    let _ = std::fs::remove_dir_all(&scratch);
}
