//! Traceability core: annotations, anchors, requirements, checker, lockfile.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use tracelean_lib::trace::{
    self, annotation, anchor, checker, evidence, hash, lockfile, requirement,
    AnchorKind, Bond, EvidenceDetail, EvidenceKey, EvidenceRecord, FindingKind, Level, Qualifier,
    Role,
};

fn fixture_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures/trace_project")
}

fn scan(name: &str, content: &str) -> annotation::ScanResult {
    let path = PathBuf::from(name);
    annotation::scan_file(&path, Path::new(""), content)
}

// --- annotation scanning -------------------------------------------------

#[test]
fn parses_rust_line_comment() {
    let r = scan("a.rs", "// @implements REQ-A.post\nfn f() {}\n");
    assert_eq!(r.annotations.len(), 1);
    assert_eq!(r.annotations[0].role, Role::Implements);
    assert_eq!(r.annotations[0].req_id, "REQ-A");
    assert_eq!(r.annotations[0].clause.as_deref(), Some("post"));
    assert!(r.precise);
}

#[test]
fn parses_python_hash_comment() {
    let r = scan("a.py", "# @tests REQ-B\ndef f():\n    pass\n");
    assert_eq!(r.annotations.len(), 1);
    assert_eq!(r.annotations[0].role, Role::Tests);
}

#[test]
fn parses_lean_dash_comment() {
    let r = scan("a.lean", "-- @models REQ-C.post\ndef f := 1\n");
    assert_eq!(r.annotations.len(), 1);
    assert_eq!(r.annotations[0].role, Role::Models);
}

#[test]
fn parses_cpp_block_comment() {
    let r = scan("a.cpp", "/* @implements REQ-D */\nint f() { return 0; }\n");
    assert_eq!(r.annotations.len(), 1);
}

#[test]
fn rust_doc_comment_is_counted_once() {
    // tree-sitter-rust nests doc_comment inside line_comment; a naive
    // `kind().contains("comment")` walk would yield this annotation 2-3 times.
    let r = scan("a.rs", "/// @implements REQ-A\nfn f() {}\n");
    assert_eq!(r.annotations.len(), 1, "doc comment counted more than once");
}

#[test]
fn unknown_role_with_id_is_reported() {
    let r = scan("a.rs", "// @implementz REQ-A\nfn f() {}\n");
    assert!(r.annotations.is_empty());
    assert_eq!(r.problems.len(), 1);
    assert_eq!(
        r.problems[0].kind,
        annotation::AnnotationProblemKind::UnknownRole
    );
}

#[test]
fn ordinary_doc_tags_are_not_flagged() {
    // `@param` and friends are documentation, not typos of a role.
    let r = scan("a.rs", "/// @param x the thing\n/// @returns nothing\nfn f() {}\n");
    assert!(r.problems.is_empty(), "doc tags must not be reported");
}

#[test]
fn attributes_are_parsed() {
    let r = scan(
        "a.rs",
        r#"// @exempt REQ-A.log reason="no logging in model" by=ana until=2027-01-01
fn f() {}
"#,
    );
    let ann = &r.annotations[0];
    match ann.qualifier.as_ref().expect("qualifier") {
        Qualifier::Exempt { reason, by, until } => {
            assert_eq!(reason.as_deref(), Some("no logging in model"));
            assert_eq!(by.as_deref(), Some("ana"));
            assert_eq!(until.as_deref(), Some("2027-01-01"));
        }
        other => panic!("expected exempt, got {other:?}"),
    }
}

#[test]
fn bare_partial_qualifies_previous_annotation() {
    let r = scan(
        "a.rs",
        "// @implements REQ-A.post\n// @partial reason=\"only the happy path\"\nfn f() {}\n",
    );
    assert_eq!(r.annotations.len(), 1);
    assert!(matches!(
        r.annotations[0].qualifier,
        Some(Qualifier::Partial { .. })
    ));
}

#[test]
fn orphan_qualifier_is_reported() {
    let r = scan("a.rs", "// @partial reason=\"x\"\nfn f() {}\n");
    assert_eq!(
        r.problems[0].kind,
        annotation::AnnotationProblemKind::OrphanQualifier
    );
}

#[test]
fn exclusive_flag_is_recorded() {
    let r = scan("a.rs", "// @implements REQ-A.post exclusive\nfn f() {}\n");
    assert!(r.annotations[0].attrs.contains_key("exclusive"));
}

#[test]
fn region_begin_end_bounds_the_body() {
    let src = "// @implements REQ-A begin\nlet x = 1;\nlet y = 2;\n// @end\nlet z = 3;\n";
    let r = scan("a.rs", src);
    assert_eq!(r.annotations.len(), 1);
    let (start, end) = r.annotations[0].region.expect("region");
    let body = &src[start..end];
    assert!(body.contains("let x"), "body was {body:?}");
    assert!(body.contains("let y"));
    assert!(!body.contains("let z"), "region must stop at @end");
}

#[test]
fn nested_regions_close_lifo() {
    let src = "// @implements REQ-A begin\n// @tests REQ-B begin\nx\n// @end\ny\n// @end\n";
    let r = scan("a.rs", src);
    assert_eq!(r.annotations.len(), 2);
    // The inner one (REQ-B) closes first and is therefore recorded first.
    assert_eq!(r.annotations[0].req_id, "REQ-B");
    assert_eq!(r.annotations[1].req_id, "REQ-A");
}

#[test]
fn unclosed_region_is_reported_but_kept() {
    let r = scan("a.rs", "// @implements REQ-A begin\nx\n");
    assert_eq!(r.annotations.len(), 1, "link must not be lost");
    assert_eq!(
        r.problems[0].kind,
        annotation::AnnotationProblemKind::UnclosedRegion
    );
}

#[test]
fn stray_end_is_reported() {
    let r = scan("a.rs", "// @end\n");
    assert_eq!(
        r.problems[0].kind,
        annotation::AnnotationProblemKind::StrayEnd
    );
}

#[test]
fn file_without_grammar_falls_back_to_line_scan() {
    let r = scan("a.unknownext", "# @implements REQ-A\nstuff\n");
    assert_eq!(r.annotations.len(), 1);
    assert!(!r.precise, "must be marked imprecise");
}

// --- anchors -------------------------------------------------------------

fn resolve(name: &str, content: &str) -> Vec<(annotation::RawAnnotation, trace::Anchor)> {
    let path = PathBuf::from(name);
    let scan = annotation::scan_file(&path, Path::new(""), content);
    anchor::resolve_all(&path, Path::new(""), content, &scan)
}

#[test]
fn anchors_to_the_following_function() {
    let got = resolve("a.rs", "// @implements REQ-A\nfn login() {}\n");
    assert_eq!(
        got[0].1.symbol_path(),
        Some("login"),
        "got {:?}",
        got[0].1.kind
    );
}

#[test]
fn anchors_to_a_method_inside_an_impl() {
    // The symbol table only extracts top-level items, so this case is why
    // anchors walk the tree themselves.
    let src = "struct S;\nimpl S {\n    // @implements REQ-A\n    fn login(&self) {}\n}\n";
    let got = resolve("a.rs", src);
    let path = got[0].1.symbol_path().expect("decl anchor");
    assert!(path.ends_with("login"), "path was {path}");
    assert!(path.contains("::"), "expected a nested path, got {path}");
}

#[test]
fn anchors_across_rust_attributes() {
    let src = "// @tests REQ-A\n#[test]\nfn t() {}\n";
    let got = resolve("a.rs", src);
    assert_eq!(got[0].1.symbol_path(), Some("t"));
}

#[test]
fn anchors_across_python_decorators() {
    let src = "import functools\n\n\n@functools.lru_cache\n# @implements REQ-A\ndef search(i, q):\n    return i\n";
    let got = resolve("a.py", src);
    assert_eq!(got[0].1.symbol_path(), Some("search"), "got {:?}", got[0].1.kind);
}

#[test]
fn anchors_to_file_when_nothing_follows() {
    let got = resolve("a.rs", "fn f() {}\n// @implements REQ-A\n");
    assert!(matches!(got[0].1.kind, AnchorKind::File));
}

#[test]
fn hash_is_stable_under_reformatting() {
    let a = resolve("a.rs", "// @implements REQ-A\nfn f() { let x = 1; }\n");
    let b = resolve("a.rs", "// @implements REQ-A\nfn f() {\n    let x   =   1;\n}\n");
    assert_eq!(a[0].1.body_hash, b[0].1.body_hash);
}

#[test]
fn hash_ignores_comment_edits() {
    // Editing the annotation itself must not invalidate its own evidence.
    let a = resolve("a.rs", "// @implements REQ-A\nfn f() { /* old note */ let x = 1; }\n");
    let b = resolve("a.rs", "// @implements REQ-A\nfn f() { /* a totally different note */ let x = 1; }\n");
    assert_eq!(a[0].1.body_hash, b[0].1.body_hash);
}

#[test]
fn hash_changes_on_a_real_edit() {
    let a = resolve("a.rs", "// @implements REQ-A\nfn f() { let x = 1; }\n");
    let b = resolve("a.rs", "// @implements REQ-A\nfn f() { let x = 2; }\n");
    assert_ne!(a[0].1.body_hash, b[0].1.body_hash);
}

#[test]
fn hash_preserves_whitespace_inside_string_literals() {
    let a = resolve("a.rs", "// @implements REQ-A\nfn f() { let s = \"a  b\"; }\n");
    let b = resolve("a.rs", "// @implements REQ-A\nfn f() { let s = \"a b\"; }\n");
    assert_ne!(a[0].1.body_hash, b[0].1.body_hash, "string content must be significant");
}

#[test]
fn link_hash_changes_when_retargeted() {
    let attrs = BTreeMap::new();
    let a = hash::hash_link("implements", "REQ-A", Some("post"), &attrs, "src/x.rs::f");
    let b = hash::hash_link("implements", "REQ-B", Some("post"), &attrs, "src/x.rs::f");
    assert_ne!(a, b);
}

// --- requirements --------------------------------------------------------

#[test]
fn parses_frontmatter_requirement() {
    let src = "---\nid: REQ-A\ntitle: Thing\nrefines: [REQ-P]\ndecomposition: complete\nclauses:\n  post: returns a token\n  err: errors are total\n---\nBody text.\n";
    match requirement::parse_markdown(Path::new("a.md"), Path::new(""), src) {
        requirement::ParseOutcome::Requirement(req, problems) => {
            assert!(problems.is_empty(), "{problems:?}");
            assert_eq!(req.id, "REQ-A");
            assert_eq!(req.refines, vec!["REQ-P"]);
            assert_eq!(req.decomposition, requirement::Decomposition::Complete);
            assert_eq!(req.clauses.len(), 2);
            assert_eq!(req.clauses.get("post").map(|s| s.as_str()), Some("returns a token"));
        }
        _ => panic!("expected a requirement"),
    }
}

#[test]
fn markdown_without_id_is_not_a_requirement() {
    let src = "# Just a heading\n\ntext\n";
    assert!(matches!(
        requirement::parse_markdown(Path::new("a.md"), Path::new(""), src),
        requirement::ParseOutcome::NotARequirement
    ));
}

#[test]
fn decomposition_defaults_to_open() {
    let src = "---\nid: REQ-A\n---\nBody.\n";
    match requirement::parse_markdown(Path::new("a.md"), Path::new(""), src) {
        requirement::ParseOutcome::Requirement(req, _) => {
            assert_eq!(req.decomposition, requirement::Decomposition::Open);
        }
        _ => panic!("expected a requirement"),
    }
}

#[test]
fn malformed_frontmatter_is_reported_not_panicked() {
    let src = "---\nid: REQ-A\nthis line has no colon\n---\nBody.\n";
    match requirement::parse_markdown(Path::new("a.md"), Path::new(""), src) {
        requirement::ParseOutcome::Requirement(req, problems) => {
            assert_eq!(req.id, "REQ-A");
            assert_eq!(problems.len(), 1);
        }
        _ => panic!("expected a requirement"),
    }
}

#[test]
fn requirement_hash_ignores_status_changes() {
    let a = "---\nid: REQ-A\nstatus: draft\nclauses:\n  post: x\n---\nBody.\n";
    let b = "---\nid: REQ-A\nstatus: approved\nclauses:\n  post: x\n---\nBody.\n";
    let ha = match requirement::parse_markdown(Path::new("a.md"), Path::new(""), a) {
        requirement::ParseOutcome::Requirement(r, _) => r.content_hash,
        _ => panic!(),
    };
    let hb = match requirement::parse_markdown(Path::new("a.md"), Path::new(""), b) {
        requirement::ParseOutcome::Requirement(r, _) => r.content_hash,
        _ => panic!(),
    };
    assert_eq!(ha, hb, "workflow status is not meaning");
}

// --- index + checker over the fixture project ----------------------------

fn fixture_index() -> trace::TraceIndex {
    trace::build(&fixture_root())
}

fn kinds(index: &trace::TraceIndex) -> Vec<FindingKind> {
    index.findings.iter().map(|f| f.kind).collect()
}

#[test]
fn fixture_finds_both_requirements_and_ignores_plain_markdown() {
    let index = fixture_index();
    assert!(index.requirements.contains_key("REQ-AUTH-03"));
    assert!(index.requirements.contains_key("REQ-AUTH"));
    assert_eq!(index.requirements.len(), 2, "notes.md must be ignored");
}

#[test]
fn fixture_links_model_impl_test_and_harness() {
    let index = fixture_index();
    assert!(index.has_role("REQ-AUTH-03", Some("post"), Role::Models));
    assert!(index.has_role("REQ-AUTH-03", Some("post"), Role::Implements));
    assert!(index.has_role("REQ-AUTH-03", Some("post"), Role::Tests));
    assert!(index.has_role("REQ-AUTH-03", Some("post"), Role::Drt));
}

#[test]
fn fixture_reports_dangling_requirement() {
    // search.py claims REQ-SEARCH-02, which no document declares.
    let index = fixture_index();
    assert!(index
        .findings
        .iter()
        .any(|f| f.kind == FindingKind::Dangling && f.req_id.as_deref() == Some("REQ-SEARCH-02")));
}

#[test]
fn exempt_clause_suppresses_coverage_findings() {
    let index = fixture_index();
    let log_findings: Vec<_> = index
        .findings
        .iter()
        .filter(|f| f.clause.as_deref() == Some("log") && f.kind == FindingKind::Unmodeled)
        .collect();
    assert!(log_findings.is_empty(), "exemption must suppress `unmodeled`");
}

#[test]
fn unbound_fires_when_no_harness_binds_a_clause() {
    // REQ-AUTH-03.err has a model and an implementation but no @drt.
    let index = fixture_index();
    assert!(index.findings.iter().any(|f| f.kind == FindingKind::Unbound
        && f.clause.as_deref() == Some("err")));
}

#[test]
fn untested_fires_for_an_implemented_clause_without_tests() {
    let index = fixture_index();
    assert!(index.findings.iter().any(|f| f.kind == FindingKind::Untested
        && f.clause.as_deref() == Some("err")));
}

#[test]
fn partial_without_reason_is_unsound() {
    let path = PathBuf::from("a.rs");
    let content = "// @implements REQ-A.post\n// @partial\nfn f() {}\n";
    let scan = annotation::scan_file(&path, Path::new(""), content);
    let resolved = anchor::resolve_all(&path, Path::new(""), content, &scan);
    let mut index = trace::TraceIndex::default();
    for (ann, anchor) in resolved {
        index.links.push(trace::Link {
            role: ann.role,
            req_id: ann.req_id.clone(),
            clause: ann.clause.clone(),
            qualifier: ann.qualifier.clone(),
            attrs: ann.attrs.clone(),
            link_hash: hash::hash_link(
                ann.role.as_str(),
                &ann.req_id,
                ann.clause.as_deref(),
                &ann.attrs,
                &anchor.ident(),
            ),
            anchor,
            line: ann.line,
        });
    }
    let findings = checker::check(&index, &checker::Policy::default());
    assert!(findings.iter().any(|f| f.kind == FindingKind::UnsoundQualifier));
}

#[test]
fn dangling_refines_and_cycles_are_detected() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("a.md"),
        "---\nid: REQ-A\nrefines: [REQ-B]\n---\nA\n",
    )
    .unwrap();
    let index = trace::build(dir.path());
    assert!(kinds(&index).contains(&FindingKind::DanglingRefines));

    std::fs::write(
        dir.path().join("b.md"),
        "---\nid: REQ-B\nrefines: [REQ-A]\n---\nB\n",
    )
    .unwrap();
    let index = trace::build(dir.path());
    assert!(kinds(&index).contains(&FindingKind::RefinesCycle));
}

#[test]
fn duplicate_ids_are_reported() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("a.md"), "---\nid: REQ-A\n---\nA\n").unwrap();
    std::fs::write(dir.path().join("b.md"), "---\nid: REQ-A\n---\nA again\n").unwrap();
    let index = trace::build(dir.path());
    assert!(kinds(&index).contains(&FindingKind::DuplicateId));
}

#[test]
fn expired_exemption_is_unsound() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("a.md"),
        "---\nid: REQ-A\nclauses:\n  log: logs things\n---\nA\n",
    )
    .unwrap();
    std::fs::write(
        dir.path().join("a.rs"),
        "// @exempt REQ-A.log reason=\"x\" by=ana until=2000-01-01\nfn f() {}\n",
    )
    .unwrap();
    let index = trace::build(dir.path());
    assert!(kinds(&index).contains(&FindingKind::UnsoundExemption));
}

// --- evidence and staleness ---------------------------------------------

fn drt_record(req: &str, clause: &str, hashes: BTreeMap<String, String>) -> EvidenceRecord {
    EvidenceRecord {
        key: EvidenceKey {
            req_id: req.into(),
            clause: Some(clause.into()),
            bond: Bond::ModelImpl,
        },
        level: Level::L3,
        input_hashes: hashes,
        detail: EvidenceDetail::Drt {
            seed: 7,
            cases: 10_000,
            divergences: 0,
            coverage_covered: 4,
            coverage_total: 4,
            lean_version: "4.29.1".into(),
        },
        at: "2026-09-11T00:00:00Z".into(),
        node: None,
    }
}

#[test]
fn evidence_stands_while_inputs_match() {
    let mut index = fixture_index();
    let current = index.current_hashes("REQ-AUTH-03", Some("post"));
    index.evidence.push(drt_record("REQ-AUTH-03", "post", current));
    let assurance = index.assurance("REQ-AUTH-03", Some("post"));
    assert_eq!(assurance.model_impl, Some(Level::L3));
    assert!(assurance.stale.is_empty());
}

#[test]
fn changing_the_model_alone_makes_drt_evidence_stale() {
    // A single anchor hash could not catch this: the implementation is
    // untouched, only the model moved.
    let mut index = fixture_index();
    let mut current = index.current_hashes("REQ-AUTH-03", Some("post"));
    current.insert("model".into(), "v1:deadbeef".into());
    index.evidence.push(drt_record("REQ-AUTH-03", "post", current));
    let assurance = index.assurance("REQ-AUTH-03", Some("post"));
    assert_eq!(assurance.model_impl, None);
    assert!(assurance.stale.contains(&Bond::ModelImpl));
}

#[test]
fn weakest_link_governs_assurance() {
    let mut a = evidence::Assurance::default();
    a.set(Bond::ModelProof, Level::L4);
    a.set(Bond::ModelImpl, Level::L1);
    assert_eq!(a.weakest(), Level::L1, "a proof about the model is not a proof about the code");
}

#[test]
fn divergence_in_evidence_becomes_a_finding() {
    let mut index = fixture_index();
    let current = index.current_hashes("REQ-AUTH-03", Some("post"));
    let mut record = drt_record("REQ-AUTH-03", "post", current);
    record.detail = EvidenceDetail::Drt {
        seed: 7,
        cases: 10,
        divergences: 3,
        coverage_covered: 1,
        coverage_total: 4,
        lean_version: "4.29.1".into(),
    };
    index.evidence.push(record);
    let findings = checker::check(&index, &checker::Policy::default());
    assert!(findings.iter().any(|f| f.kind == FindingKind::Divergence));
}

#[test]
fn confirmed_judge_drift_becomes_a_finding() {
    let mut index = fixture_index();
    let current = index.current_hashes("REQ-AUTH-03", Some("post"));
    index.evidence.push(EvidenceRecord {
        key: EvidenceKey {
            req_id: "REQ-AUTH-03".into(),
            clause: Some("post".into()),
            bond: Bond::RequirementModel,
        },
        level: Level::L1,
        input_hashes: current,
        detail: EvidenceDetail::Judge {
            verdict: "under_constrained".into(),
            prompt_version: "judge/v1".into(),
            model_id: "test".into(),
            witness_confirmed: Some(true),
            degraded: false,
            confidence: "high".into(),
            comparison_mode: "structural-json".into(),
            cost_usd: 0.0,
            source: "api".into(),
        },
        at: "2026-09-11T00:00:00Z".into(),
        node: None,
    });
    let findings = checker::check(&index, &checker::Policy::default());
    assert!(findings.iter().any(|f| f.kind == FindingKind::JudgeDrift));
}

#[test]
fn unconfirmed_judge_drift_is_not_a_finding() {
    // A verdict whose witness was falsified must never produce a finding.
    let mut index = fixture_index();
    let current = index.current_hashes("REQ-AUTH-03", Some("post"));
    index.evidence.push(EvidenceRecord {
        key: EvidenceKey {
            req_id: "REQ-AUTH-03".into(),
            clause: Some("post".into()),
            bond: Bond::RequirementModel,
        },
        level: Level::L1,
        input_hashes: current,
        detail: EvidenceDetail::Judge {
            verdict: "under_constrained".into(),
            prompt_version: "judge/v1".into(),
            model_id: "test".into(),
            witness_confirmed: Some(false),
            degraded: false,
            confidence: "low".into(),
            comparison_mode: "structural-json".into(),
            cost_usd: 0.0,
            source: "api".into(),
        },
        at: "2026-09-11T00:00:00Z".into(),
        node: None,
    });
    let findings = checker::check(&index, &checker::Policy::default());
    assert!(!findings.iter().any(|f| f.kind == FindingKind::JudgeDrift));
}

// --- policy --------------------------------------------------------------

#[test]
fn policy_require_rule_reports_missing_level() {
    let index = fixture_index();
    let policy = checker::Policy {
        severity: BTreeMap::new(),
        require: vec![checker::RequireRule {
            r#match: "REQ-AUTH*".into(),
            min_level: "L3".into(),
            bond: "model-impl".into(),
        }],
        block_on: vec![],
        high_value: vec![],
    };
    let findings = checker::check(&index, &policy);
    assert!(findings.iter().any(|f| f.kind == FindingKind::PolicyUnmet));
}

#[test]
fn policy_severity_override_applies() {
    let index = fixture_index();
    let mut severity = BTreeMap::new();
    severity.insert("untested".to_string(), checker::Severity::Error);
    let policy = checker::Policy { severity, require: vec![], block_on: vec![], high_value: vec![] };
    let findings = checker::check(&index, &policy);
    let untested = findings.iter().find(|f| f.kind == FindingKind::Untested).expect("untested");
    assert_eq!(untested.severity, checker::Severity::Error);
}

// --- lockfile ------------------------------------------------------------

#[test]
fn lockfile_serialization_is_deterministic() {
    let index = fixture_index();
    let a = lockfile::to_string(&lockfile::render(&index)).unwrap();
    let b = lockfile::to_string(&lockfile::render(&index)).unwrap();
    assert_eq!(a, b);

    // And a fresh scan of the same tree must produce the same bytes.
    let again = fixture_index();
    let c = lockfile::to_string(&lockfile::render(&again)).unwrap();
    assert_eq!(a, c, "rebuild from a clean scan must be byte-identical");
}

#[test]
fn lockfile_round_trips() {
    let index = fixture_index();
    let text = lockfile::to_string(&lockfile::render(&index)).unwrap();
    let parsed: lockfile::Lockfile = serde_json::from_str(&text).unwrap();
    assert_eq!(parsed.version, lockfile::VERSION);
    assert_eq!(parsed.requirements.len(), index.requirements.len());
}

#[test]
fn put_evidence_preserves_other_records_and_replaces_its_own() {
    let dir = tempfile::tempdir().unwrap();
    let hashes = BTreeMap::new();
    let first = drt_record("REQ-A", "post", hashes.clone());
    lockfile::put_evidence(dir.path(), first).unwrap();

    let mut other = drt_record("REQ-B", "post", hashes.clone());
    other.key.bond = Bond::ModelImpl;
    lockfile::put_evidence(dir.path(), other).unwrap();

    let mut updated = drt_record("REQ-A", "post", hashes);
    updated.at = "2026-09-12T00:00:00Z".into();
    lockfile::put_evidence(dir.path(), updated).unwrap();

    let lock = lockfile::load(dir.path()).unwrap();
    assert_eq!(lock.evidence.len(), 2, "one per (key, backend)");
    let a = lock.evidence.iter().find(|r| r.key.req_id == "REQ-A").unwrap();
    assert_eq!(a.at, "2026-09-12T00:00:00Z");
}

#[test]
fn scanner_never_drops_evidence() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("a.md"), "---\nid: REQ-A\n---\nA\n").unwrap();
    lockfile::put_evidence(dir.path(), drt_record("REQ-A", "post", BTreeMap::new())).unwrap();
    let index = trace::build(dir.path());
    assert_eq!(index.evidence.len(), 1, "evidence is authored elsewhere and must survive a scan");
}

#[test]
fn default_policy_file_is_written_on_first_scan() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("a.md"), "---\nid: REQ-A\n---\nA\n").unwrap();
    let _ = trace::build(dir.path());
    assert!(dir.path().join(".tracelean/trace_policy.json").exists());
}

// --- roll-up over the refinement DAG ------------------------------------

/// Build an index from inline files, so DAG shapes can be written directly.
fn index_from(files: &[(&str, &str)]) -> (tempfile::TempDir, trace::TraceIndex) {
    let dir = tempfile::tempdir().unwrap();
    for (name, content) in files {
        let path = dir.path().join(name);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).unwrap();
        }
        std::fs::write(path, content).unwrap();
    }
    let index = trace::build(dir.path());
    (dir, index)
}

#[test]
fn rollup_counts_a_shared_leaf_once() {
    // A diamond: TOP refines into L and R, both of which have LEAF as a child.
    let (_d, index) = index_from(&[
        ("top.md", "---\nid: TOP\ndecomposition: complete\n---\nTop.\n"),
        ("l.md", "---\nid: L\nrefines: [TOP]\ndecomposition: complete\n---\nLeft.\n"),
        ("r.md", "---\nid: R\nrefines: [TOP]\ndecomposition: complete\n---\nRight.\n"),
        (
            "leaf.md",
            "---\nid: LEAF\nrefines: [L, R]\ndecomposition: complete\nclauses:\n  post: does a thing\n---\nLeaf.\n",
        ),
    ]);
    let rollup = index.rollup("TOP");
    // TOP, L, R each contribute one bodyless clause; LEAF contributes one —
    // counted once despite being reachable by two paths.
    assert_eq!(rollup.reachable.len(), 4);
    assert_eq!(rollup.leaf_clauses, 4, "the shared leaf must not be double-counted");
}

#[test]
fn one_open_node_makes_the_whole_chain_a_lower_bound() {
    let (_d, index) = index_from(&[
        ("top.md", "---\nid: TOP\ndecomposition: complete\n---\nTop.\n"),
        ("kid.md", "---\nid: KID\nrefines: [TOP]\ndecomposition: open\n---\nKid.\n"),
    ]);
    assert!(
        index.rollup("TOP").coverage_is_lower_bound,
        "an open descendant means the denominator is unknown"
    );
}

#[test]
fn assurance_is_the_minimum_not_the_mean() {
    let (_d, mut index) = index_from(&[
        (
            "top.md",
            "---\nid: TOP\ndecomposition: complete\nclauses:\n  a: first\n  b: second\n---\nTop.\n",
        ),
        ("m.lean", "-- @models TOP.a\n-- @models TOP.b\ndef f := 1\n"),
    ]);
    // Give clause `a` strong evidence and leave `b` with none.
    let current = index.current_hashes("TOP", Some("a"));
    index.evidence.push(EvidenceRecord {
        key: EvidenceKey { req_id: "TOP".into(), clause: Some("a".into()), bond: Bond::ModelProof },
        level: Level::L4,
        input_hashes: current,
        detail: EvidenceDetail::Proof { theorem: "t".into(), lean_version: "4.29.1".into() },
        at: "2026-09-11T00:00:00Z".into(),
        node: None,
    });
    let rollup = index.rollup("TOP");
    assert_eq!(rollup.assurance, Level::L1, "the unchecked clause governs");
}

#[test]
fn exempt_leaves_the_denominator_and_partial_caps_the_numerator() {
    let (_d, index) = index_from(&[
        (
            "r.md",
            "---\nid: REQ-X\ndecomposition: complete\nclauses:\n  a: first\n  b: second\n  c: third\n---\nR.\n",
        ),
        (
            "code.rs",
            "// @exempt REQ-X.a reason=\"out of model scope\" by=ana\nfn one() {}\n\n// @implements REQ-X.b\n// @partial reason=\"happy path only\"\nfn two() {}\n\n// @implements REQ-X.c\nfn three() {}\n",
        ),
    ]);
    let rollup = index.rollup("REQ-X");
    assert_eq!(rollup.exempt_clauses, 1);
    assert_eq!(rollup.leaf_clauses, 2, "the exempt clause leaves the denominator");
    // b is claimed but partial (0.5), c is claimed but unverified (0.5).
    assert!((rollup.coverage - 0.5).abs() < 0.001, "coverage was {}", rollup.coverage);
}

#[test]
fn collapsed_parent_is_never_greener_than_expanded() {
    let (_d, index) = index_from(&[
        ("top.md", "---\nid: TOP\ndecomposition: complete\n---\nTop.\n"),
        (
            "kid.md",
            "---\nid: KID\nrefines: [TOP]\ndecomposition: complete\nclauses:\n  post: x\n---\nKid.\n",
        ),
        // Dangling annotation under the child produces an error finding.
        ("code.rs", "// @implements NOPE-1\nfn f() {}\n"),
    ]);
    let collapsed = index.tree(Some(0));
    let expanded = index.tree(None);
    let top_collapsed = collapsed.iter().find(|n| n.req_id == "TOP").unwrap();
    let top_expanded = expanded.iter().find(|n| n.req_id == "TOP").unwrap();

    assert!(top_collapsed.children.is_empty());
    assert_eq!(
        top_collapsed.rollup.coverage, top_expanded.rollup.coverage,
        "collapsing must not change the number"
    );
    assert_eq!(top_collapsed.rollup.assurance, top_expanded.rollup.assurance);
    assert_eq!(top_collapsed.rollup.leaf_clauses, top_expanded.rollup.leaf_clauses);
}

#[test]
fn a_requirement_with_two_parents_is_marked_duplicate_once() {
    let (_d, index) = index_from(&[
        ("a.md", "---\nid: A\n---\nA.\n"),
        ("b.md", "---\nid: B\n---\nB.\n"),
        ("c.md", "---\nid: C\nrefines: [A, B]\n---\nC.\n"),
    ]);
    let tree = index.tree(None);
    let mut occurrences = Vec::new();
    fn collect(node: &trace::TreeNode, out: &mut Vec<bool>) {
        if node.req_id == "C" {
            out.push(node.duplicate);
        }
        for child in &node.children {
            collect(child, out);
        }
    }
    for root in &tree {
        collect(root, &mut occurrences);
    }
    assert_eq!(occurrences.len(), 2, "C appears under both parents");
    assert_eq!(
        occurrences.iter().filter(|d| !**d).count(),
        1,
        "exactly one occurrence is the canonical one"
    );
}

#[test]
fn cycles_do_not_hang_rollup() {
    let (_d, index) = index_from(&[
        ("a.md", "---\nid: A\nrefines: [B]\n---\nA.\n"),
        ("b.md", "---\nid: B\nrefines: [A]\n---\nB.\n"),
    ]);
    let rollup = index.rollup("A");
    assert_eq!(rollup.reachable.len(), 2);
    // With every node parented, the tree falls back to listing all of them
    // rather than rendering nothing.
    assert_eq!(index.tree(None).len(), 2);
}

// --- coverage map --------------------------------------------------------

#[test]
fn the_coverage_map_includes_untraced_files_as_grey() {
    // The grey is the point: a map that showed only annotated code would
    // flatter the project.
    let (dir, index) = index_from(&[
        ("r.md", "---\nid: REQ-X\nclauses:\n  a: first\n---\nR.\n"),
        ("traced.rs", "// @implements REQ-X.a\nfn one() {\n    let x = 1;\n}\n"),
        ("untraced.rs", "fn two() {}\nfn three() {}\nfn four() {}\n"),
    ]);
    let map = trace::map::build(dir.path(), &index);

    let untraced: Vec<_> = map
        .entries
        .iter()
        .filter(|e| e.requirements.is_empty())
        .collect();
    assert!(
        untraced.iter().any(|e| e.file.ends_with("untraced.rs")),
        "an untraced file must still appear"
    );
    assert!(map.total_lines > map.traced_lines);
    assert!(map.traced_fraction() > 0.0 && map.traced_fraction() < 1.0);
}

#[test]
fn requirement_documents_are_not_part_of_the_map() {
    let (dir, index) = index_from(&[
        ("r.md", "---\nid: REQ-X\n---\nR.\n"),
        ("code.rs", "fn f() {}\n"),
    ]);
    let map = trace::map::build(dir.path(), &index);
    assert!(
        !map.entries.iter().any(|e| e.file.extension().and_then(|x| x.to_str()) == Some("md")),
        "requirements describe the project; they are not part of it"
    );
}

#[test]
fn a_partly_annotated_file_reports_its_remainder_as_untraced() {
    let (dir, index) = index_from(&[
        ("r.md", "---\nid: REQ-X\nclauses:\n  a: first\n---\nR.\n"),
        (
            "mixed.rs",
            "// @implements REQ-X.a\nfn traced() {\n    let x = 1;\n}\n\nfn untraced() {\n    let y = 2;\n}\n",
        ),
    ]);
    let map = trace::map::build(dir.path(), &index);
    let mixed: Vec<_> = map.entries.iter().filter(|e| e.file.ends_with("mixed.rs")).collect();
    assert!(mixed.iter().any(|e| !e.requirements.is_empty()));
    assert!(mixed.iter().any(|e| e.requirements.is_empty()), "the rest of the file is grey");
}


#[test]
fn several_annotations_on_one_declaration_are_one_rectangle_counted_once() {
    // The bug this pins: entries used to be emitted per *link*, so a function
    // carrying three `@implements` produced three rectangles over the same
    // lines and counted those lines three times toward coverage.
    let (dir, index) = index_from(&[
        (
            "r.md",
            "---\nid: REQ-X\nclauses:\n  a: 1\n  b: 2\n  c: 3\n---\nR.\n",
        ),
        (
            "code.rs",
            "// @implements REQ-X.a\n// @implements REQ-X.b\n// @implements REQ-X.c\nfn one() {\n    let x = 1;\n}\n",
        ),
    ]);
    let map = trace::map::build(dir.path(), &index);
    let traced: Vec<_> = map
        .entries
        .iter()
        .filter(|e| e.file.ends_with("code.rs") && !e.requirements.is_empty())
        .collect();
    assert_eq!(traced.len(), 1, "one anchor is one rectangle: {traced:?}");
    assert_eq!(traced[0].requirements, vec!["REQ-X".to_string()]);
    assert!(
        map.traced_lines <= map.total_lines,
        "covered lines can never exceed the file: {} of {}",
        map.traced_lines,
        map.total_lines
    );
}

#[test]
fn every_line_of_a_file_is_attributed_exactly_once() {
    // Areas have to be conservative for a treemap to mean anything: the
    // rectangles of a file must sum to the file, no more and no less.
    let (dir, index) = index_from(&[
        ("r.md", "---\nid: REQ-X\nclauses:\n  a: 1\n---\nR.\n"),
        (
            "code.rs",
            "// @implements REQ-X.a\nfn traced() {\n    let x = 1;\n}\n\nfn untraced() {\n    let y = 2;\n}\n",
        ),
    ]);
    let map = trace::map::build(dir.path(), &index);
    let file_lines: u32 = map
        .entries
        .iter()
        .filter(|e| e.file.ends_with("code.rs"))
        .map(|e| e.lines)
        .sum();
    let on_disk = std::fs::read_to_string(dir.path().join("code.rs"))
        .unwrap()
        .lines()
        .count() as u32;
    assert_eq!(file_lines, on_disk, "rectangles must tile the file exactly");
}

#[test]
fn an_inner_region_owns_its_lines_rather_than_the_declaration_around_it() {
    let (dir, index) = index_from(&[
        (
            "r.md",
            "---\nid: REQ-X\nclauses:\n  outer: 1\n  inner: 2\n---\nR.\n",
        ),
        (
            "code.rs",
            "// @implements REQ-X.outer\nfn wide() {\n    // @implements REQ-X.inner begin\n    let x = 1;\n    // @end\n    let y = 2;\n}\n",
        ),
    ]);
    let map = trace::map::build(dir.path(), &index);
    let entries: Vec<_> = map
        .entries
        .iter()
        .filter(|e| e.file.ends_with("code.rs") && !e.requirements.is_empty())
        .collect();
    assert!(
        entries.len() >= 2,
        "the region and its container are separate rectangles: {entries:?}"
    );
    let file_lines: u32 = map
        .entries
        .iter()
        .filter(|e| e.file.ends_with("code.rs"))
        .map(|e| e.lines)
        .sum();
    assert_eq!(
        file_lines,
        std::fs::read_to_string(dir.path().join("code.rs")).unwrap().lines().count() as u32,
        "nesting must not double-count"
    );
}

#[test]
fn a_rectangles_tint_is_the_weakest_of_the_requirements_that_claim_it() {
    // Locally the same rule as the roll-up: a shared anchor is worth its
    // weakest claim, never the best one.
    let (dir, index) = index_from(&[
        ("r.md", "---\nid: REQ-X\nclauses:\n  a: 1\n---\nR.\n"),
        ("code.rs", "// @implements REQ-X.a\nfn one() {}\n"),
    ]);
    let map = trace::map::build(dir.path(), &index);
    let traced = map
        .entries
        .iter()
        .find(|e| !e.requirements.is_empty())
        .expect("an annotated rectangle");
    assert!(
        traced.weakest.is_none(),
        "no evidence recorded yet, so there is no level to show — not L1 by default"
    );
}

#[test]
fn links_in_a_file_come_back_in_line_order() {
    let (_d, index) = index_from(&[
        ("r.md", "---\nid: REQ-X\nclauses:\n  a: 1\n  b: 2\n---\nR.\n"),
        (
            "code.rs",
            "// @implements REQ-X.b\nfn second() {}\n\n// @implements REQ-X.a\nfn first() {}\n",
        ),
    ]);
    let links = trace::map::links_in_file(&index, std::path::Path::new("code.rs"));
    assert_eq!(links.len(), 2);
    assert!(links[0].anchor.start_line < links[1].anchor.start_line);
}

// --- progress over git history ------------------------------------------

/// Build a throwaway git repository with two commits: one requirement with no
/// implementation, then the implementation added.
fn history_repo(name: &str) -> Option<PathBuf> {
    let dir = std::env::temp_dir().join(format!("tracelean-history-test-{}-{}", std::process::id(), name));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).ok()?;

    let git = |args: &[&str]| {
        std::process::Command::new("git")
            .arg("-C")
            .arg(&dir)
            .args(args)
            .output()
            .ok()
            .filter(|o| o.status.success())
    };
    git(&["init", "-q"])?;
    git(&["config", "user.email", "t@example.com"])?;
    git(&["config", "user.name", "test"])?;

    std::fs::create_dir_all(dir.join("docs")).ok()?;
    std::fs::write(
        dir.join("docs/r.md"),
        "---\nid: REQ-H\ntitle: History\ndecomposition: complete\nstatus: approved\nclauses:\n  post: it works\n---\nbody\n",
    )
    .ok()?;
    git(&["add", "-A"])?;
    git(&["commit", "-q", "-m", "add requirement"])?;

    std::fs::create_dir_all(dir.join("src")).ok()?;
    std::fs::write(dir.join("src/a.rs"), "// @implements REQ-H.post\nfn a() {}\n").ok()?;
    git(&["add", "-A"])?;
    git(&["commit", "-q", "-m", "implement it"])?;

    Some(dir)
}

#[test]
fn history_reports_a_point_per_commit_oldest_first() {
    let Some(dir) = history_repo("points") else {
        eprintln!("git unavailable — skipping");
        return;
    };
    let (points, problems) = trace::history(&dir, 10).expect("history");
    assert!(problems.is_empty(), "unexpected problems: {:?}", problems);
    assert_eq!(points.len(), 2);
    assert_eq!(points[0].subject, "add requirement");
    assert_eq!(points[1].subject, "implement it");
    assert!(
        points[1].coverage > points[0].coverage,
        "adding an implementation must raise coverage: {:?} → {:?}",
        points[0].coverage,
        points[1].coverage
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn history_caches_computed_commits() {
    let Some(dir) = history_repo("cache") else {
        eprintln!("git unavailable — skipping");
        return;
    };
    let (first, _) = trace::history(&dir, 10).expect("history");
    let cache = dir.join(".tracelean/history.json");
    assert!(cache.is_file(), "computed points must be cached by commit sha");

    // Corrupt the working tree: if the second call recomputed from the tree
    // rather than reusing the cache, the numbers would move.
    std::fs::remove_dir_all(dir.join("src")).unwrap();
    let (second, _) = trace::history(&dir, 10).expect("history");
    assert_eq!(first, second, "a commit's point must be computed exactly once");
    let _ = std::fs::remove_dir_all(&dir);
}

// --- the shipped example project -----------------------------------------

fn example_root() -> Option<PathBuf> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("..").join("..").join("example");
    root.join("product").is_dir().then_some(root)
}

/// The example project is the first thing anyone opens, and it is a claim about
/// how the system is meant to be used. If its annotations do not resolve, it
/// teaches the wrong syntax to every reader.
#[test]
fn the_example_project_resolves_cleanly() {
    let Some(root) = example_root() else {
        eprintln!("no example project — skipping");
        return;
    };
    let index = trace::build(&root);

    for id in ["REQ-CHECKOUT", "REQ-DISCOUNT", "REQ-SHIPPING", "REQ-RECEIPT"] {
        assert!(index.requirements.contains_key(id), "missing requirement {id}");
    }

    // Every role that is written as an annotation is exercised somewhere.
    // `@drt` is absent on purpose: a Python implementation is called directly
    // by TraceLean's shipped runner, so there is no adapter file to annotate
    // and the binding in `.tracelean/drt.json` is what binds.
    for role in [Role::Models, Role::Implements, Role::Tests, Role::Proves] {
        assert!(
            index.links.iter().any(|l| l.role == role),
            "the example must demonstrate @{:?}",
            role
        );
    }
    assert!(
        !index.drt_bindings.is_empty(),
        "the example must demonstrate a differential-testing binding"
    );

    // No annotation may point at a requirement or clause that does not exist —
    // that is the one failure that would make the example actively misleading.
    let dangling: Vec<_> = index
        .findings
        .iter()
        .filter(|f| matches!(f.kind, FindingKind::Dangling | FindingKind::DanglingRefines | FindingKind::DuplicateId))
        .collect();
    assert!(dangling.is_empty(), "example has dangling links: {:?}", dangling);
}

#[test]
fn the_example_demonstrates_partial_and_exempt() {
    let Some(root) = example_root() else { return };
    let index = trace::build(&root);
    assert!(
        index.links.iter().any(|l| l.is_partial()),
        "the example must show a @partial implementation"
    );
    assert!(
        index.links.iter().any(|l| l.is_exempt()),
        "the example must show an @exempt clause"
    );
}

/// The example is built around a clause that nobody implemented, so the panel
/// has something true to be unhappy about. If someone "fixes" it by adding an
/// annotation, this test says what was lost.
#[test]
fn the_example_keeps_one_clause_deliberately_unbacked() {
    let Some(root) = example_root() else { return };
    let index = trace::build(&root);
    let backed = index
        .links
        .iter()
        .any(|l| l.req_id == "REQ-DISCOUNT" && l.clause.as_deref() == Some("coupon"));
    assert!(
        !backed,
        "REQ-DISCOUNT.coupon is unimplemented on purpose: it is what makes the example's \
         coverage number honest rather than decorative"
    );
}

#[test]
fn a_lean_annotation_anchors_to_the_declaration_not_the_keyword() {
    // tree-sitter-lean4 parses `def f ...` as a named `definition` node whose
    // first child is an anonymous `def` token -- and gives the keyword token
    // inside `structure C where` the kind `"structure"`, the same string as the
    // declaration node around it. Matching on kind alone picked the keyword:
    // every Lean anchor in the project resolved to the symbol path "def" with a
    // one-line span, which broke the body hash (so a model could be edited
    // without going stale), the gutter, the coverage map, and differential
    // binding, which needs the declaration's name to read its signature.
    let Some(root) = example_root() else { return };
    let index = trace::build(&root);

    let lean: Vec<_> = index
        .links
        .iter()
        .filter(|l| l.anchor.file.extension().and_then(|e| e.to_str()) == Some("lean"))
        .collect();
    assert!(!lean.is_empty(), "the example project has Lean annotations");

    for link in &lean {
        let symbol = link.anchor.symbol_path().unwrap_or("");
        assert!(
            !["def", "theorem", "abbrev", "instance", "structure", "inductive"]
                .contains(&symbol),
            "{} {:?} anchored to the keyword `{symbol}` rather than a declaration",
            link.req_id,
            link.clause
        );
    }

    let models = lean
        .iter()
        .find(|l| l.req_id == "REQ-DISCOUNT" && l.role == trace::Role::Models)
        .expect("REQ-DISCOUNT is modelled");
    assert_eq!(models.anchor.symbol_path(), Some("discountCents"));
    assert!(
        models.anchor.end_line > models.anchor.start_line,
        "the anchor must span the whole declaration, not just its first line"
    );
}

// --- the project graph ----------------------------------------------------

use tracelean_lib::trace::graph::{self, Confidence, EdgeKind, NodeKind};

fn example_graph() -> Option<graph::ProjectGraph> {
    let root = example_root()?;
    let index = trace::build(&root);
    Some(graph::build(&root, &index, &index.findings, graph::DEFAULT_DECLARATION_CAP))
}

#[test]
fn untraced_code_is_in_the_graph_and_is_not_l1() {
    // The grey is the point. A graph of only the annotated parts flatters the
    // project exactly as a coverage map of only the covered lines would -- and
    // `None` is not `L1`: `L1` means somebody claimed this and nothing has
    // checked it, `None` means nobody claimed it at all.
    let Some(graph) = example_graph() else { return };

    let untraced = graph
        .nodes
        .iter()
        .find(|n| n.kind == NodeKind::Declaration && n.name == "is_remote")
        .expect("`is_remote` carries no annotation in the example");
    assert!(untraced.requirements.is_empty());
    assert_eq!(untraced.assurance, None);

    let traced = graph
        .nodes
        .iter()
        .find(|n| n.name == "discount_cents")
        .expect("`discount_cents` is annotated");
    assert!(!traced.requirements.is_empty());
    assert!(traced.assurance.is_some());
}

#[test]
fn a_node_is_tinted_by_the_weakest_of_its_requirements() {
    let Some(root) = example_root() else { return };
    let index = trace::build(&root);
    let graph = graph::build(&root, &index, &index.findings, graph::DEFAULT_DECLARATION_CAP);

    for node in graph.nodes.iter().filter(|n| !n.requirements.is_empty()) {
        let expected = node
            .requirements
            .iter()
            .map(|r| {
                let (req, clause) = match r.split_once('.') {
                    Some((req, clause)) => (req, Some(clause)),
                    None => (r.as_str(), None),
                };
                index.assurance(req, clause).weakest()
            })
            .min();
        assert_eq!(
            node.assurance, expected,
            "{} is tinted by something other than its weakest requirement",
            node.id
        );
    }
}

#[test]
fn containment_is_certain_and_a_name_match_is_not() {
    let Some(graph) = example_graph() else { return };

    assert!(graph
        .edges
        .iter()
        .filter(|e| e.kind == EdgeKind::Contains)
        .all(|e| e.confidence == Confidence::Certain));
    assert!(graph
        .edges
        .iter()
        .filter(|e| e.kind == EdgeKind::References)
        .all(|e| e.confidence == Confidence::Textual));

    // Every containment edge names two real nodes, so nothing is drawn into
    // empty space.
    let ids: std::collections::HashSet<&str> =
        graph.nodes.iter().map(|n| n.id.as_str()).collect();
    for edge in &graph.edges {
        assert!(ids.contains(edge.from.as_str()), "dangling edge from {}", edge.from);
        assert!(ids.contains(edge.to.as_str()), "dangling edge to {}", edge.to);
    }
}

#[test]
fn a_reference_edge_stays_inside_one_language() {
    // A Python test mentioning `price` is not a reference to the Lean `price`.
    // Drawing that edge made the two relations worth seeing -- test to
    // implementation, and model to implementation -- indistinguishable from a
    // coincidence of naming.
    let Some(graph) = example_graph() else { return };
    let by_id: std::collections::HashMap<&str, &graph::GraphNode> =
        graph.nodes.iter().map(|n| (n.id.as_str(), n)).collect();

    for edge in graph.edges.iter().filter(|e| e.kind == EdgeKind::References) {
        let from = by_id[edge.from.as_str()];
        let to = by_id[edge.to.as_str()];
        assert_eq!(
            from.file.extension(),
            to.file.extension(),
            "{} -> {} crosses languages",
            edge.from,
            edge.to
        );
    }

    // And the edges that should be there, are.
    assert!(graph.edges.iter().any(|e| e.from.ends_with("pricing.py::price")
        && e.to.ends_with("pricing.py::discount_cents")));
}

#[test]
fn a_directory_is_as_big_as_what_is_inside_it() {
    // Zero-sized directories would draw as zero-area tiles, which is the bug
    // the old coverage map had: a rectangle whose area means nothing.
    let Some(graph) = example_graph() else { return };
    for directory in graph.nodes.iter().filter(|n| n.kind == NodeKind::Directory) {
        let contained: u32 = graph
            .nodes
            .iter()
            .filter(|n| n.kind == NodeKind::File && n.file.starts_with(&directory.file))
            .map(|n| n.lines)
            .sum();
        assert_eq!(directory.lines, contained, "{}", directory.id);
        assert!(directory.lines > 0, "{} is empty", directory.id);
    }
}

#[test]
fn nothing_is_dropped_without_being_counted() {
    // A graph that quietly shows two thirds of a project is worse than one
    // that shows a third and says so.
    let Some(root) = example_root() else { return };
    let index = trace::build(&root);
    let capped = graph::build(&root, &index, &index.findings, 3);
    let full = graph::build(&root, &index, &index.findings, usize::MAX);

    let declarations = |g: &graph::ProjectGraph| {
        g.nodes.iter().filter(|n| n.kind == NodeKind::Declaration).count()
    };
    assert_eq!(declarations(&capped), 3);
    assert_eq!(
        declarations(&capped) + capped.omitted_declarations,
        declarations(&full)
    );
    assert_eq!(capped.node_cap, 3);

    // Files and directories survive the cap, so the shape of the project is
    // still there even when its detail is not.
    let files = |g: &graph::ProjectGraph| {
        g.nodes.iter().filter(|n| n.kind == NodeKind::File).count()
    };
    assert_eq!(files(&capped), files(&full));
}

#[test]
fn a_finding_lands_on_the_declaration_it_is_about() {
    let Some(root) = example_root() else { return };
    let index = trace::build(&root);
    let graph = graph::build(&root, &index, &index.findings, graph::DEFAULT_DECLARATION_CAP);

    for finding in &index.findings {
        let file = graph
            .nodes
            .iter()
            .find(|n| n.kind == NodeKind::File && n.file == finding.file);
        let Some(file) = file else { continue };
        assert!(
            file.findings.iter().any(|f| f.message == finding.message),
            "{} lost the finding {:?}",
            file.id,
            finding.message
        );
    }
}

// --- spec strength ---------------------------------------------------------

#[test]
fn strength_obligations_are_generated_from_the_proved_theorems() {
    let Some(root) = example_root() else { return };
    let index = trace::build(&root);
    let obligations = trace::strength::obligations(&root, &index);
    assert!(!obligations.is_empty(), "the example proves things, so it owes obligations");

    let discount = obligations
        .iter()
        .find(|o| o.declaration == "discountCents")
        .expect("discountCents is modelled and proved about");
    assert!(
        discount.source.contains("def Spec_discountCents (f : Nat → Nat)"),
        "the candidate's type comes from the model's own binders:\n{}",
        discount.source
    );
    assert!(
        discount.source.contains("f subtotal ≤ subtotal"),
        "the theorem's statement must be abstracted over the model:\n{}",
        discount.source
    );
    assert!(
        !discount.source.contains("discountCents subtotal ≤"),
        "the concrete model must not survive into the predicate:\n{}",
        discount.source
    );
    assert!(discount.source.contains("sorry"), "the proof is the human's part");
    assert!(discount.source.contains("@pins REQ-DISCOUNT.tiers"), "{}", discount.source);
}

#[test]
fn a_clause_nobody_has_claimed_reads_as_open_not_as_passing() {
    let Some(root) = example_root() else { return };
    let index = trace::build(&root);
    // `open` is the honest default: nobody has said whether the proved
    // properties determine this model, and that is different from saying they
    // do. REQ-RECEIPT has no strength obligation written for it.
    assert_eq!(
        trace::strength::declared(&index, "REQ-RECEIPT", Some("lines")),
        trace::strength::Strength::Open
    );
    // A written obligation is a claim, not a proof: until the toolchain has
    // been asked, the strongest the index alone will say is "attempted".
    assert_eq!(
        trace::strength::declared(&index, "REQ-SHIPPING", Some("free")),
        trace::strength::Strength::Attempted { unproved: Vec::new() }
    );
}

#[test]
fn a_declaration_head_splits_into_binders_and_type() {
    let (binders, ty) = trace::strength::split_head(
        "theorem free_above_threshold (afterDiscount : Nat) (h : afterDiscount ≥ 10000) :\n    shippingCents afterDiscount false = 0 := by",
    )
    .expect("a theorem head splits");
    assert_eq!(binders, "(afterDiscount : Nat) (h : afterDiscount ≥ 10000)");
    assert_eq!(ty, "shippingCents afterDiscount false = 0");
}

/// Write the example's strength obligations and print them. Not part of the
/// suite: it writes files. `cargo test -- --ignored materialize_strength`
#[test]
#[ignore]
fn materialize_strength() {
    let root = example_root().expect("example project");
    let index = trace::build(&root);
    let obligations = trace::strength::obligations(&root, &index);
    let text = trace::strength::render_file(&obligations);
    let path = root.join("formal/Strength.lean");
    std::fs::write(&path, &text).unwrap();
    println!("wrote {} ({} obligations)", path.display(), obligations.len());
    for o in &obligations {
        for p in &o.problems {
            println!("  problem [{}]: {p}", o.declaration);
        }
    }
}

/// Ask the toolchain whether the example's `@pins` obligations are finished.
/// Ignored: it needs Lean. `cargo test -- --ignored probe_strength`
#[test]
#[ignore]
fn probe_strength() {
    let root = example_root().expect("example project");
    let index = trace::build(&root);
    for ((req, clause), state) in trace::strength::check(&root, &index) {
        println!("{req}{} -> {}", clause.map(|c| format!(".{c}")).unwrap_or_default(), state.as_str());
    }
}

#[test]
fn a_sorry_warning_is_located_by_line() {
    let output = "Strength.lean:47:8: warning: declaration uses 'sorry'\n\
                  Strength.lean:83:8: warning: declaration uses 'sorry'\n";
    assert_eq!(trace::strength::sorry_lines(output), vec![47, 83]);
}

#[test]
fn a_proof_nobody_has_measured_is_reported_as_a_gap() {
    let Some(root) = example_root() else { return };
    let index = trace::build(&root);
    // REQ-DISCOUNT.tiers is proved and has an obligation written for it, so it
    // is not a gap. A clause with `@proves` and no obligation at all would be.
    let unpinned: Vec<&trace::Finding> = index
        .findings
        .iter()
        .filter(|f| f.kind == trace::FindingKind::UnpinnedProof)
        .collect();
    for finding in &unpinned {
        let clause = finding.clause.clone();
        assert_eq!(
            trace::strength::declared(
                &index,
                finding.req_id.as_deref().unwrap(),
                clause.as_deref()
            ),
            trace::strength::Strength::Open,
            "a gap is only reported where the question is actually unanswered"
        );
    }
    // And the reverse: nothing that has an obligation is reported as a gap.
    assert!(
        !unpinned
            .iter()
            .any(|f| f.req_id.as_deref() == Some("REQ-SHIPPING")),
        "REQ-SHIPPING has a `@pins` obligation, so it is not an unmeasured proof"
    );
}

/// Every `(requirement, clause)` the example binds to a differential test.
fn example_bindings(root: &std::path::Path) -> Vec<(String, Option<String>)> {
    tracelean_lib::drt::DrtConfig::load(root)
        .map(|c| c.bindings.iter().map(|b| (b.req_id.clone(), b.clause.clone())).collect())
        .unwrap_or_default()
}

#[test]
fn the_role_graph_puts_each_declaration_in_the_column_of_its_claim() {
    let Some(root) = example_root() else { return };
    let index = trace::build(&root);
    let findings = index.findings.clone();
    let coverage = trace::coverage::LineCoverage::load(&root);
    let bindings = example_bindings(&root);
    let tests = trace::test_results::TestResults::load(&root);
    let graph = trace::graph::role_graph(&index, &findings, &coverage, &tests, &bindings);

    let by_id = |needle: &str| {
        graph
            .nodes
            .iter()
            .find(|n| n.id.contains(needle))
            .unwrap_or_else(|| panic!("no node matching {needle}: {:?}", graph.nodes.iter().map(|n| &n.id).collect::<Vec<_>>()))
    };

    assert_eq!(by_id("REQ-CHECKOUT.total").column, trace::graph::Column::Requirement);
    assert_eq!(by_id("Checkout.lean::price").column, trace::graph::Column::Model);
    assert_eq!(by_id("pricing.py::price").column, trace::graph::Column::Implementation);

    // Every edge is an annotation somebody wrote, so every edge names a role.
    assert!(!graph.edges.is_empty());
    for edge in &graph.edges {
        assert!(
            ["models", "implements", "tests", "drt", "proves", "pins"]
                .contains(&edge.role.as_str()),
            "unknown role on an edge: {}",
            edge.role
        );
        assert!(graph.nodes.iter().any(|n| n.id == edge.from), "dangling from: {}", edge.from);
        assert!(graph.nodes.iter().any(|n| n.id == edge.to), "dangling to: {}", edge.to);
    }
}

#[test]
fn a_clause_with_no_annotation_is_counted_not_drawn() {
    let Some(root) = example_root() else { return };
    let index = trace::build(&root);
    let graph = trace::graph::role_graph(
        &index,
        &index.findings.clone(),
        &trace::coverage::LineCoverage::load(&root),
        &trace::test_results::TestResults::load(&root),
        &example_bindings(&root),
    );
    // REQ-DISCOUNT.coupon exists on purpose with nothing claiming it.
    assert!(
        graph.unlinked_clauses.iter().any(|c| c == "REQ-DISCOUNT.coupon"),
        "{:?}",
        graph.unlinked_clauses
    );
    assert!(
        !graph.nodes.iter().any(|n| n.id == "REQ-DISCOUNT.coupon"),
        "an orphan is a number, not a floating node in an empty column"
    );
}

// --- line coverage, attached to declarations -------------------------------

#[test]
fn coverage_is_read_per_declaration_not_per_repository() {
    use trace::coverage::{FileCoverage, LineCoverage};
    let mut cov = LineCoverage::default();
    cov.files.insert(
        std::path::PathBuf::from("engine/pricing.py"),
        FileCoverage {
            // 1-indexed, as every coverage tool reports.
            covered: [10u32, 11, 12].into_iter().collect(),
            uncovered: [20u32, 21].into_iter().collect(),
        },
    );
    // Anchors are 0-indexed and inclusive, so lines 10..=12 are span 9..=11.
    let hit = cov.for_span(std::path::Path::new("engine/pricing.py"), 9, 11).unwrap();
    assert_eq!((hit.covered, hit.executable), (3, 3));
    assert_eq!(hit.fraction(), 1.0);

    let miss = cov.for_span(std::path::Path::new("engine/pricing.py"), 19, 20).unwrap();
    assert_eq!((miss.covered, miss.executable), (0, 2));

    // A file nobody measured has no coverage, which is not the same as zero.
    assert!(cov.for_span(std::path::Path::new("engine/other.py"), 0, 5).is_none());
}

#[test]
fn a_line_with_no_executable_code_counts_as_neither() {
    use trace::coverage::{FileCoverage, LineCoverage};
    let mut cov = LineCoverage::default();
    cov.files.insert(
        std::path::PathBuf::from("a.py"),
        FileCoverage {
            covered: [3u32].into_iter().collect(),
            uncovered: Default::default(),
        },
    );
    // Twenty lines of docstring and blanks around one statement is 1/1, not
    // 1/20: a fraction over "lines in the span" would punish documentation.
    let span = cov.for_span(std::path::Path::new("a.py"), 0, 19).unwrap();
    assert_eq!((span.covered, span.executable), (1, 1));
}

#[test]
fn coverage_py_output_is_imported_as_written() {
    let json: serde_json::Value = serde_json::from_str(
        r#"{"files": {"engine/pricing.py": {"executed_lines": [1, 5], "missing_lines": [7]}}}"#,
    )
    .unwrap();
    let cov = trace::coverage::from_coverage_py(std::path::Path::new("/nowhere"), &json).unwrap();
    let file = cov.files.get(std::path::Path::new("engine/pricing.py")).unwrap();
    assert_eq!(file.covered.iter().copied().collect::<Vec<u32>>(), vec![1, 5]);
    assert_eq!(file.uncovered.iter().copied().collect::<Vec<u32>>(), vec![7]);
}

#[test]
fn llvm_cov_segments_decide_what_is_executable() {
    // `[line, column, count, has_count, is_region_entry, is_gap]`. A segment
    // with `has_count: false` marks the end of a region and says nothing about
    // whether the line runs, so it must land in neither set.
    let json: serde_json::Value = serde_json::from_str(
        r#"{"data":[{"files":[{"filename":"core/src/a.rs","segments":[
            [10,1,4,true,true,false],
            [12,1,0,true,true,false],
            [14,1,0,false,false,false]
        ]}]}]}"#,
    )
    .unwrap();
    let cov = trace::coverage::from_llvm_cov(std::path::Path::new("/nowhere"), &json).unwrap();
    let file = cov.files.get(std::path::Path::new("core/src/a.rs")).unwrap();
    assert_eq!(file.covered.iter().copied().collect::<Vec<u32>>(), vec![10]);
    assert_eq!(file.uncovered.iter().copied().collect::<Vec<u32>>(), vec![12]);
}

/// Import the example's Python coverage and show what each node carries.
/// Ignored: it writes `.tracelean/coverage.json`.
/// `cargo test -- --ignored import_example_coverage -- --nocapture`
#[test]
#[ignore]
fn import_example_coverage() {
    let root = example_root().expect("example project");
    let source = std::env::var("COVERAGE_JSON").expect("COVERAGE_JSON=<path to coverage json>");
    let cov = trace::coverage::import(&root, std::path::Path::new(&source)).expect("import");
    cov.save(&root).expect("save");
    println!("imported {} file(s)", cov.files.len());

    let index = trace::build(&root);
    let graph = trace::graph::role_graph(
        &index,
        &index.findings.clone(),
        &trace::coverage::LineCoverage::load(&root),
        &trace::test_results::TestResults::load(&root),
        &example_bindings(&root),
    );
    for node in &graph.nodes {
        let cover = node
            .coverage
            .map(|c| format!("{}/{} lines", c.covered, c.executable))
            .unwrap_or_else(|| "-".into());
        let tests = node
            .tests
            .map(|t| match t.passing {
                Some(p) => format!("{p}/{} passing", t.claimed),
                None => format!("{} test(s)", t.claimed),
            })
            .unwrap_or_else(|| "-".into());
        let harness = node
            .harness
            .as_ref()
            .map(|h| format!("{} cases, {} divergences", h.cases, h.divergences))
            .unwrap_or_else(|| "-".into());
        println!(
            "{:<12} {:<40} cov={:<14} {:<12} {:<10} strength={}",
            format!("{:?}", node.column),
            node.label,
            cover,
            tests,
            harness,
            node.strength.clone().unwrap_or_else(|| "-".into())
        );
    }
}

/// Import the example's coverage and test results, then print the graph.
/// Ignored: it writes into `.tracelean/`.
#[test]
#[ignore]
fn import_example_test_results() {
    let root = example_root().expect("example project");
    let source = std::env::var("RESULTS_JSON").expect("RESULTS_JSON=<path>");
    let results = trace::test_results::import(&root, std::path::Path::new(&source)).expect("import");
    results.save(&root).expect("save");
    println!("imported {} result(s)", results.results.len());
}

// --- test results ----------------------------------------------------------

#[test]
fn a_test_outcome_is_matched_by_the_declaration_name() {
    use trace::test_results::{Outcome, TestResults};
    let mut results = TestResults::default();
    results.results.insert(
        "pricing_checks.ShippingChecks.test_flat_below_the_threshold".into(),
        Outcome::Passed,
    );
    results
        .results
        .insert("unit::test_trace::test_flat_below_the_threshold".into(), Outcome::Failed);

    // Runners qualify names differently and an annotation anchors to a
    // declaration, so the last segment is the only part both sides agree on.
    // Two tests with one name and one failure must not read as passing.
    assert_eq!(
        results.outcome_for("test_flat_below_the_threshold"),
        Some(Outcome::Failed)
    );
    assert_eq!(results.outcome_for("test_nothing_named_this"), None);
}

#[test]
fn libtest_json_is_imported_and_started_events_carry_no_verdict() {
    use trace::test_results::{from_libtest_json, Outcome};
    let results = from_libtest_json(
        "{\"type\":\"suite\",\"event\":\"started\"}\n\
         {\"type\":\"test\",\"event\":\"started\",\"name\":\"a\"}\n\
         {\"type\":\"test\",\"event\":\"ok\",\"name\":\"a\"}\n\
         {\"type\":\"test\",\"event\":\"failed\",\"name\":\"b\"}\n\
         {\"type\":\"test\",\"event\":\"ignored\",\"name\":\"c\"}\n\
         not json at all\n",
    );
    assert_eq!(results.outcome_for("a"), Some(Outcome::Passed));
    assert_eq!(results.outcome_for("b"), Some(Outcome::Failed));
    // Ignored is recorded, not dropped: a claim backed only by tests that never
    // ran should not look green.
    assert_eq!(results.outcome_for("c"), Some(Outcome::Skipped));
    assert_eq!(results.results.len(), 3);
}

#[test]
fn the_role_graph_carries_the_properties_each_node_actually_has() {
    let Some(root) = example_root() else { return };
    let index = trace::build(&root);
    let graph = trace::graph::role_graph(
        &index,
        &index.findings.clone(),
        &trace::coverage::LineCoverage::load(&root),
        &trace::test_results::TestResults::load(&root),
        &example_bindings(&root),
    );

    // Spec strength is a property of the model, so it is answered on the model
    // node and not only on the requirement pointing at it.
    let model = graph
        .nodes
        .iter()
        .find(|n| n.column == trace::graph::Column::Model && n.label == "shippingCents")
        .expect("shippingCents is a model");
    assert!(model.strength.is_some(), "a model node must say whether it is pinned");
    assert!(model.coverage.is_none(), "a Lean model has no line coverage");

    // Tests belong to the implementation they exercise.
    let implementation = graph
        .nodes
        .iter()
        .find(|n| {
            n.column == trace::graph::Column::Implementation && n.label == "shipping_cents"
        })
        .expect("shipping_cents is an implementation");
    assert!(
        implementation.tests.map(|t| t.claimed).unwrap_or(0) > 0,
        "the implementation must carry its test count"
    );
    assert!(implementation.strength.is_none(), "strength is a model property");

    // The harness had no node at all before, which is why its state was
    // invisible in this picture.
    let harness = graph
        .nodes
        .iter()
        .find(|n| n.harness.is_some())
        .expect("a bound clause must have a harness node");
    let state = harness.harness.as_ref().unwrap();
    assert!(state.bound, "the example binds every modelled clause");
    assert_eq!(harness.column, trace::graph::Column::Evidence);
    assert!(
        graph.edges.iter().any(|e| e.to == harness.id && e.role == "drt"),
        "the harness node must hang off the clause it binds"
    );
}

/// Print every finding the example produces. Ignored: diagnostic only.
#[test]
#[ignore]
fn probe_example_findings() {
    let root = example_root().expect("example project");
    let index = trace::build(&root);
    for f in &index.findings {
        println!(
            "[{:?}] {:?} {}{} — {}",
            f.severity,
            f.kind,
            f.req_id.clone().unwrap_or_default(),
            f.clause.clone().map(|c| format!(".{c}")).unwrap_or_default(),
            f.message
        );
    }
}

/// `scripts/unittest_results.py` is the Python half of the test-result
/// importer. It is a script rather than a library, which is exactly why it
/// needs a test: nothing else would notice it breaking, and a silently broken
/// importer turns "3 of 3 passing" into "3 tests" with no error anywhere.
#[test]
fn the_unittest_result_script_reports_pass_fail_and_skip() {
    let script = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("scripts")
        .join("unittest_results.py");
    if !script.is_file() {
        eprintln!("no script at {} — skipping", script.display());
        return;
    }
    if std::process::Command::new("python3").arg("--version").output().is_err() {
        eprintln!("no python3 — skipping");
        return;
    }

    let dir = tempfile::TempDir::new().unwrap();
    std::fs::write(
        dir.path().join("sample_checks.py"),
        "import unittest\n\n\
         class Sample(unittest.TestCase):\n\
         \x20   def test_passes(self):\n\
         \x20       self.assertEqual(1, 1)\n\n\
         \x20   def test_fails(self):\n\
         \x20       self.assertEqual(1, 2)\n\n\
         \x20   @unittest.skip('deliberately')\n\
         \x20   def test_skipped(self):\n\
         \x20       pass\n",
    )
    .unwrap();

    let output_path = dir.path().join("results.json");
    let run = std::process::Command::new("python3")
        .arg(&script)
        .arg(dir.path())
        .arg(&output_path)
        .output()
        .expect("the script runs");
    assert!(
        output_path.is_file(),
        "the script wrote nothing. stdout: {} stderr: {}",
        String::from_utf8_lossy(&run.stdout),
        String::from_utf8_lossy(&run.stderr)
    );

    let results = trace::test_results::import(dir.path(), &output_path).expect("import");
    use trace::test_results::Outcome;
    assert_eq!(results.outcome_for("test_passes"), Some(Outcome::Passed));
    // The failing one is the point: a script that reported everything as
    // passing would be worse than none, and would look identical here.
    assert_eq!(results.outcome_for("test_fails"), Some(Outcome::Failed));
    assert_eq!(results.outcome_for("test_skipped"), Some(Outcome::Skipped));
    assert_eq!(results.results.len(), 3);
}
