mod harness;

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use tracelean_core::drt::proofs;
use tracelean_core::trace::index::build;
use tracelean_core::trace::proofcheck::{self, Refusal, Verdict};
use tracelean_core::trace::record::Detail;
use tracelean_core::trace::store;

/// @tests REQ-EVID.mark_per_bond
/// @tests REQ-EVID.record_reproducible
/// @tests REQ-STALE.inputs_identified
/// @tests REQ-STRENGTH.kernel_decides
#[test]
#[ignore = "builds the Lean models"]
fn every_theorem_lean_accepts_earns_its_clause_a_record() {
    let root = harness::project_root();
    let index = build(&root);
    let report = proofs::check(&root, &index).expect("the models build and Lean runs");
    proofs::save(&root, &report).expect("the verdicts are kept");
    let (written, removed) = proofs::earn_all(&root, &index, &report, &proofs::toolchain());
    assert!(written > 50, "only {written} proof records written");
    println!("{written} proof records written, {removed} withdrawn");
    for entry in report.proofs.iter().filter(|p| !p.verdict.is_accepted()) {
        println!("  refused {}::{} {:?}", entry.req_id, entry.theorem, entry.verdict);
    }
    assert!(
        store::read_all(&root).iter().any(|r| matches!(r.detail, Detail::Proof { .. })),
        "the store holds no proof record after writing {written}"
    );
}

const PLANTED: &str = r#"namespace Fix

/-- @models REQ-F.a -/
def double (n : Nat) : Nat := n + n

/-- @proves REQ-F.a -/
theorem double_is_twice (n : Nat) : double n = 2 * n := by
  unfold double
  omega

/-- @proves REQ-F.a -/
theorem double_respects_equality (n m : Nat) (h : n = m) : double n = double m := by
  subst h
  rfl

/-- @proves REQ-F.a -/
theorem double_adds_nothing (n : Nat) : double n + 0 = double n := by
  unfold double
  simp

/-- @proves REQ-F.a -/
theorem twice_without_the_model (n : Nat) : n + n = 2 * n := by
  omega

/-- @proves REQ-F.a -/
theorem double_of_two : double 2 = 4 := rfl

def quad (n : Nat) : Nat := double (double n)

/-- @proves REQ-F.a -/
theorem quad_through_double (n : Nat) : quad n = 4 * n ∧ double n = 2 * n := by
  unfold quad double
  omega

/-- @proves REQ-F.a -/
theorem double_by_the_compiler (n : Nat) : double n = 2 * n ∧ double 3 = 6 :=
  ⟨by unfold double; omega, by native_decide⟩

/-- @proves REQ-F.a -/
theorem double_unfinished (n : Nat) : double n = n * 2 := by
  sorry

/-- @pins REQ-F.a -/
theorem double_pinned : ∀ g : Nat → Nat, (∀ n, g n = 2 * n) → g = double := by
  intro g h
  funext n
  rw [h n]
  unfold double
  omega

/-- @models REQ-F.b -/
structure Box where
  v : Nat

/-- @proves REQ-F.b -/
theorem boxes_with_one_value_are_one (a b : Box) (h : a.v = b.v) : a = b := by
  cases a
  cases b
  simp_all

/-- @assumes REQ-F.c -/
axiom oracle : Nat → Nat

/-- @assumes REQ-F.c -/
axiom oracle_is_identity : ∀ n, oracle n = n

/-- @models REQ-F.c -/
noncomputable def ask (n : Nat) : Nat := oracle n

/-- @proves REQ-F.c -/
theorem asking_answers_the_question (n : Nat) : ask n = n := oracle_is_identity n

axiom wishful : ∀ n : Nat, n = n + 0

/-- @models REQ-F.d -/
def same (n : Nat) : Nat := n

/-- @proves REQ-F.d -/
theorem same_by_wishing (n : Nat) : same n = n + 0 := wishful n

/-- @pins REQ-F.d -/
theorem same_pinned_too_weakly : ∀ g : Nat → Nat, g = same → g = same := fun _ h => h

end Fix
"#;

fn fixture() -> PathBuf {
    let dir = harness::scratch("proof-fixture");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("formal/Fix")).unwrap();
    let toolchain = harness::project_root().join("formal/lean-toolchain");
    std::fs::copy(&toolchain, dir.join("formal/lean-toolchain")).unwrap();
    std::fs::write(
        dir.join("formal/lakefile.lean"),
        "import Lake\nopen Lake DSL\n\npackage «fix» where\n\n@[default_target]\nlean_lib «Fix» where\n  globs := #[.submodules `Fix]\n",
    )
    .unwrap();
    std::fs::write(dir.join("formal/Fix/Planted.lean"), PLANTED).unwrap();
    dir
}

fn verdicts(root: &Path) -> (BTreeMap<String, Verdict>, BTreeMap<String, Verdict>) {
    let index = build(root);
    let report = proofs::check(root, &index).expect("the fixture builds and Lean runs");
    let short = |name: &str| name.rsplit("::").next().unwrap_or(name).to_string();
    let theorems = report.proofs.iter().map(|p| (short(&p.theorem), p.verdict.clone())).collect();
    let pins = report.models.iter().flat_map(|m| m.pins.iter()).map(|p| (short(&p.theorem), p.verdict.clone())).collect();
    (theorems, pins)
}

fn refused(refusal: Refusal) -> Verdict {
    Verdict::Refused { refusal }
}

/// @tests REQ-STRENGTH.proof_states_the_model
/// @tests REQ-STRENGTH.proof_is_general
/// @tests REQ-STRENGTH.proof_is_direct
/// @tests REQ-STRENGTH.proof_not_vacuous
/// @tests REQ-STRENGTH.model_is_a_function
/// @tests REQ-STRENGTH.axioms_declared
/// @tests REQ-STRENGTH.assumptions_shown
/// @tests REQ-STRENGTH.pinned_by_theorem
/// @tests REQ-STRENGTH.kernel_decides
#[test]
#[ignore = "builds a Lean package"]
fn each_way_of_proving_nothing_is_refused_by_name() {
    let root = fixture();
    let (theorems, pins) = verdicts(&root);
    let expect = |name: &str, verdict: Verdict| {
        assert_eq!(theorems.get(name), Some(&verdict), "{name}: {theorems:#?}");
    };
    expect("double_is_twice", Verdict::Accepted { assumed: vec![] });
    expect("double_respects_equality", refused(Refusal::Vacuous));
    expect("double_adds_nothing", refused(Refusal::Vacuous));
    expect("twice_without_the_model", refused(Refusal::Silent));
    expect("double_of_two", refused(Refusal::Example));
    expect("quad_through_double", refused(Refusal::Indirect("Fix.quad".into())));
    expect("double_by_the_compiler", refused(Refusal::Axiom("Lean.ofReduceBool".into())));
    expect("double_unfinished", refused(Refusal::Sorry));
    expect("boxes_with_one_value_are_one", refused(Refusal::Type));
    expect(
        "asking_answers_the_question",
        Verdict::Accepted { assumed: vec!["Fix.oracle".into(), "Fix.oracle_is_identity".into()] },
    );
    expect("same_by_wishing", refused(Refusal::Axiom("Fix.wishful".into())));
    assert_eq!(pins.get("double_pinned"), Some(&Verdict::Accepted { assumed: vec![] }), "{pins:#?}");
    assert_eq!(pins.get("same_pinned_too_weakly"), Some(&refused(Refusal::Statement)), "{pins:#?}");
    let _ = std::fs::remove_dir_all(&root);
}

/// @tests REQ-STRENGTH.not_proved_by_us
#[test]
fn the_obligation_is_stated_and_never_attempted() {
    let plan = proofcheck::ModelPlan {
        model: "Fix.double".into(),
        file: "formal/Fix/Planted.lean".into(),
        theorems: vec![proofcheck::Claimed { theorem: "Fix::double_is_twice".into(), ..Default::default() }],
        ..Default::default()
    };
    let source = proofcheck::obligation_source(
        vec!["Fix.Planted".into()],
        vec![plan],
        vec![("Fix.double".into(), "Fix::double_is_twice".into())],
    );
    let commands: Vec<&str> = source.lines().filter(|l| l.starts_with("#tracelean_obligation")).collect();
    assert_eq!(commands.len(), 1, "{source}");
    for word in ["theorem ", "example ", " by ", ":= by"] {
        assert!(!source.contains(word), "the check states `{word}`:\n{source}");
    }
    let command = proofcheck::PRELUDE
        .split("elab \"#tracelean_obligation ")
        .nth(1)
        .expect("the prelude defines the obligation command");
    for tactic in ["runTactic", "evalTactic", "tactic|", "TraceLeanCheck.closes", "addDecl"] {
        assert!(!command.contains(tactic), "the obligation command runs `{tactic}`");
    }
}

/// @tests REQ-STRENGTH.refusal_reported
#[test]
fn a_refused_theorem_is_a_finding_naming_it_its_clause_and_why() {
    use tracelean_core::trace::checker::{refused as findings_of, Kind, Policy, Severity};
    let entry = |theorem: &str, verdict: Verdict| proofcheck::ProofEntry {
        theorem: theorem.into(),
        model: "Fix.double".into(),
        req_id: "REQ-F".into(),
        clause: Some("a".into()),
        file: "formal/Fix/Planted.lean".into(),
        line: 7,
        verdict,
    };
    let report = proofcheck::ProofReport {
        rules: proofcheck::rules_hash(),
        proofs: vec![
            entry("Fix::double_is_twice", Verdict::Accepted { assumed: vec![] }),
            entry("Fix::twice_without_the_model", refused(Refusal::Silent)),
            entry("Fix::quad_through_double", refused(Refusal::Indirect("Fix.quad".into()))),
        ],
        models: vec![],
    };
    let found = findings_of(&report, &Policy::default());
    assert_eq!(found.len(), 2, "{found:#?}");
    for (finding, (name, why)) in found.iter().zip([
        ("twice_without_the_model", Refusal::Silent.message()),
        ("quad_through_double", Refusal::Indirect("Fix.quad".into()).message()),
    ]) {
        assert_eq!(finding.kind, Kind::ProofRefused);
        assert_eq!(finding.severity, Severity::Warn);
        assert!(!finding.blocking);
        assert_eq!((finding.req_id.as_deref(), finding.clause.as_deref()), (Some("REQ-F"), Some("a")));
        assert_eq!((finding.file.as_str(), finding.line), ("formal/Fix/Planted.lean", 7));
        assert!(finding.message.contains(name) && finding.message.contains("REQ-F.a") && finding.message.contains(&why), "{}", finding.message);
    }
}
