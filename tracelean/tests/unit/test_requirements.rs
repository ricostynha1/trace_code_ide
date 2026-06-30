//! Unit tests for requirements module (MVP 3)

use tracelean_lib::requirements::*;
use std::path::PathBuf;
use tempfile::TempDir;

#[test]
fn status_from_str() {
    assert_eq!(ReqStatus::from_str("draft"), ReqStatus::Draft);
    assert_eq!(ReqStatus::from_str("approved"), ReqStatus::Approved);
    assert_eq!(ReqStatus::from_str("linked"), ReqStatus::Linked);
    assert_eq!(ReqStatus::from_str("unknown"), ReqStatus::Draft);
    assert_eq!(ReqStatus::from_str("APPROVED"), ReqStatus::Approved);
}

#[test]
fn status_transitions() {
    assert!(ReqStatus::Draft.can_transition_to(&ReqStatus::Approved));
    assert!(ReqStatus::Approved.can_transition_to(&ReqStatus::Linked));
    assert!(ReqStatus::Approved.can_transition_to(&ReqStatus::Draft));
    assert!(ReqStatus::Linked.can_transition_to(&ReqStatus::Approved));

    // Invalid transitions
    assert!(!ReqStatus::Draft.can_transition_to(&ReqStatus::Linked));
    assert!(!ReqStatus::Linked.can_transition_to(&ReqStatus::Draft));
    assert!(!ReqStatus::Draft.can_transition_to(&ReqStatus::Draft));
}

#[test]
fn parse_requirement_file() {
    let dir = TempDir::new().unwrap();
    let reqs_dir = dir.path().join("reqs");
    std::fs::create_dir_all(&reqs_dir).unwrap();

    let content = "# REQ-01: User Authentication\nStatus: approved\n\nUsers must log in.\n";
    let file = reqs_dir.join("REQ-01.md");
    std::fs::write(&file, content).unwrap();

    let req = parse_requirement(&file, dir.path()).unwrap();
    assert_eq!(req.id, "REQ-01");
    assert_eq!(req.title, "User Authentication");
    assert_eq!(req.status, ReqStatus::Approved);
    assert_eq!(req.file, PathBuf::from("reqs/REQ-01.md"));
    assert!(req.description.contains("Users must log in"));
}

#[test]
fn parse_requirement_no_status_defaults_draft() {
    let dir = TempDir::new().unwrap();
    let reqs_dir = dir.path().join("reqs");
    std::fs::create_dir_all(&reqs_dir).unwrap();

    let content = "# REQ-02: Something\n\nDescription here.\n";
    let file = reqs_dir.join("REQ-02.md");
    std::fs::write(&file, content).unwrap();

    let req = parse_requirement(&file, dir.path()).unwrap();
    assert_eq!(req.id, "REQ-02");
    assert_eq!(req.status, ReqStatus::Draft);
}

#[test]
fn list_requirements_empty_dir() {
    let dir = TempDir::new().unwrap();
    let reqs = list_requirements(dir.path());
    assert!(reqs.is_empty());
}

#[test]
fn list_requirements_finds_all() {
    let dir = TempDir::new().unwrap();
    let reqs_dir = dir.path().join("reqs");
    std::fs::create_dir_all(&reqs_dir).unwrap();

    std::fs::write(reqs_dir.join("REQ-01.md"), "# REQ-01: First\nStatus: draft\n").unwrap();
    std::fs::write(reqs_dir.join("REQ-02.md"), "# REQ-02: Second\nStatus: approved\n").unwrap();
    // Non-md file should be skipped
    std::fs::write(reqs_dir.join("notes.txt"), "just notes").unwrap();

    let reqs = list_requirements(dir.path());
    assert_eq!(reqs.len(), 2);
    assert_eq!(reqs[0].id, "REQ-01");
    assert_eq!(reqs[1].id, "REQ-02");
}

#[test]
fn update_status_in_content() {
    let content = "# REQ-01: Title\nStatus: draft\n\nSome text.\n";
    let updated = update_requirement_status(content, &ReqStatus::Approved);
    assert!(updated.contains("Status: approved"));
    assert!(!updated.contains("Status: draft"));
    assert!(updated.contains("Some text."));
}

#[test]
fn update_status_inserts_if_missing() {
    let content = "# REQ-01: Title\n\nSome text.\n";
    let updated = update_requirement_status(content, &ReqStatus::Draft);
    assert!(updated.contains("Status: draft"));
}

#[test]
fn generate_spec_template_has_req_info() {
    let req = RequirementInfo {
        id: "REQ-05".into(),
        title: "Data Export".into(),
        status: ReqStatus::Approved,
        file: PathBuf::from("reqs/REQ-05.md"),
        description: "Export data as CSV.".into(),
        has_spec: false,
    };
    let template = generate_spec_template(&req);
    assert!(template.contains("REQ-05"));
    assert!(template.contains("Data Export"));
    assert!(template.contains("structure"));
}

#[test]
fn has_spec_detection() {
    let dir = TempDir::new().unwrap();
    let reqs_dir = dir.path().join("reqs");
    let specs_dir = dir.path().join("specs");
    std::fs::create_dir_all(&reqs_dir).unwrap();
    std::fs::create_dir_all(&specs_dir).unwrap();

    std::fs::write(reqs_dir.join("REQ-01.md"), "# REQ-01: Auth\nStatus: linked\n").unwrap();
    std::fs::write(specs_dir.join("REQ-01.lean"), "-- spec").unwrap();

    let req = parse_requirement(&reqs_dir.join("REQ-01.md"), dir.path()).unwrap();
    assert!(req.has_spec);

    // No spec for REQ-02
    std::fs::write(reqs_dir.join("REQ-02.md"), "# REQ-02: Other\nStatus: draft\n").unwrap();
    let req2 = parse_requirement(&reqs_dir.join("REQ-02.md"), dir.path()).unwrap();
    assert!(!req2.has_spec);
}
