//! Orchestrating a differential run: generate, ask both sides, compare,
//! shrink, and record what was actually established.

use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use std::time::{Duration, Instant};

use super::config::Binding;
use super::gen::{self, Rng};
use super::protocol::{replies_agree, Case, Reply, Runner, RunnerError};
use super::schema::output_variant;

/// A case on which the two sides disagreed.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Divergence {
    pub case: u64,
    pub input: serde_json::Value,
    pub op: String,
    pub model: ReplySummary,
    pub implementation: ReplySummary,
    /// How many shrink steps reduced the original failing input.
    pub shrunk_steps: usize,
    /// Whose fault this is — a decision, never an assumption.
    pub triage: Triage,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReplySummary {
    pub output: Option<serde_json::Value>,
    pub error: Option<String>,
}

impl From<&Reply> for ReplySummary {
    fn from(r: &Reply) -> Self {
        Self { output: r.output.clone(), error: r.error.clone() }
    }
}

/// A divergence means *something* is wrong, and which thing is a judgement.
///
/// Auto-filing every divergence as an implementation bug would be wrong often
/// enough to destroy trust in the whole system: the model and the adapter are
/// just as capable of being wrong as the code.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "kebab-case")]
pub enum Triage {
    #[default]
    Untriaged,
    ImplBug,
    ModelBug,
    AdapterBug,
}

/// Which parts of the space were actually exercised.
///
/// Reported next to the case count because a case count alone is theatre: a
/// million cases that all took one branch establish nothing.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Coverage {
    pub input_constructors_seen: Vec<String>,
    pub input_constructors_total: usize,
    pub output_variants_seen: Vec<String>,
}

impl Coverage {
    pub fn covered(&self) -> usize {
        self.input_constructors_seen.len() + self.output_variants_seen.len()
    }

    pub fn total(&self) -> usize {
        // Output variants are discovered rather than declared, so the best
        // available denominator is "the input constructors we knew about plus
        // whatever outputs we saw".
        self.input_constructors_total + self.output_variants_seen.len()
    }

    /// The default floor: every declared input constructor exercised, and at
    /// least one output variant observed.
    pub fn meets_floor(&self, floor: &CoverageFloor) -> bool {
        match floor {
            CoverageFloor::None => true,
            CoverageFloor::AllInputConstructors => {
                self.input_constructors_seen.len() >= self.input_constructors_total
                    && !self.output_variants_seen.is_empty()
            }
            CoverageFloor::MinOutputVariants { n } => self.output_variants_seen.len() >= *n,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum CoverageFloor {
    None,
    AllInputConstructors,
    /// A struct variant, not a newtype one: `#[serde(tag = "kind")]` cannot
    /// represent a newtype variant at all, so the tuple form silently failed to
    /// serialize — and `.tracelean/drt.json` is the one place this type has to
    /// survive a round trip.
    MinOutputVariants { n: usize },
}

impl Default for CoverageFloor {
    fn default() -> Self {
        CoverageFloor::AllInputConstructors
    }
}

/// What one run established.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DrtResult {
    pub req_id: String,
    pub clause: Option<String>,
    pub seed: u64,
    pub cases_run: usize,
    pub divergences: Vec<Divergence>,
    pub coverage: Coverage,
    pub coverage_floor_met: bool,
    pub duration_ms: u64,
    /// Runner-level trouble that is not a divergence: timeouts, restarts.
    pub incidents: Vec<String>,
}

impl DrtResult {
    /// A run earns L3 only when nothing diverged *and* the run was diverse
    /// enough to have had a chance of finding something.
    pub fn earns_l3(&self) -> bool {
        self.divergences.is_empty() && self.coverage_floor_met && self.cases_run > 0
    }
}

pub struct RunOptions {
    pub seed: u64,
    pub cases: usize,
    pub timeout: Duration,
    pub max_restarts: u32,
    pub shrink_budget: usize,
    /// Replayed before generated cases: judge witnesses, past divergences.
    pub seeds_corpus: Vec<serde_json::Value>,
}

impl Default for RunOptions {
    fn default() -> Self {
        Self {
            seed: 1,
            cases: 200,
            timeout: Duration::from_secs(10),
            max_restarts: 3,
            shrink_budget: 200,
            seeds_corpus: Vec::new(),
        }
    }
}

/// Run a binding's model against its implementation.
///
/// The implementation is passed as a spawnable spec rather than read off the
/// binding, because a binding names a *function* and turning that into a
/// process is the caller's job -- `CallSpec::spec` does it. Making it a
/// parameter means there is no such thing as an unresolved run to guard
/// against, and it lets a test drive this machinery with a stub process.
pub fn run(
    binding: &Binding,
    implementation_spec: &crate::drt::protocol::RunnerSpec,
    options: &RunOptions,
) -> Result<DrtResult, String> {
    let started = Instant::now();

    let mut model = Runner::spawn(&binding.model)
        .map_err(|e| format!("model runner: {e}"))?;
    let mut implementation = Runner::spawn(implementation_spec)
        .map_err(|e| format!("implementation runner: {e}"))?;

    let mut rng = Rng::new(options.seed);
    let mut incidents = Vec::new();
    let mut divergences: Vec<Divergence> = Vec::new();
    let mut coverage = Coverage {
        input_constructors_total: binding.input.constructors().len(),
        ..Coverage::default()
    };
    let mut seen_inputs: BTreeSet<String> = BTreeSet::new();
    let mut seen_outputs: BTreeSet<String> = BTreeSet::new();
    let mut cases_run = 0usize;

    // The corpus goes first: a witness the judge produced, or an input that
    // diverged last time, is worth far more than a fresh random case.
    let corpus = options.seeds_corpus.clone();
    let total = options.cases.max(corpus.len());

    for i in 0..total {
        let input = match corpus.get(i) {
            Some(value) => value.clone(),
            None => gen::generate(&binding.input, &mut rng),
        };
        let case = Case { case: i as u64, op: binding.op.clone(), input: input.clone() };

        let model_reply = ask(&mut model, &case, options, &mut incidents, "model");
        let impl_reply = ask(&mut implementation, &case, options, &mut incidents, "implementation");
        cases_run += 1;

        if let Some(c) = binding.input.constructor_of(&input) {
            seen_inputs.insert(c);
        }

        let (model_reply, impl_reply) = match (model_reply, impl_reply) {
            (Some(m), Some(i)) => (m, i),
            // A runner that could not answer at all is an incident, already
            // recorded; there is nothing to compare.
            _ => continue,
        };

        if let Some(out) = &model_reply.output {
            seen_outputs.insert(output_variant(out));
        }

        if replies_agree(&model_reply, &impl_reply) {
            continue;
        }

        let (minimal, steps) = shrink_divergence(
            &mut model,
            &mut implementation,
            binding,
            &input,
            options,
            &mut incidents,
        );

        // Re-ask on the minimal input so the recorded replies match it.
        let final_case = Case { case: i as u64, op: binding.op.clone(), input: minimal.clone() };
        let m = ask(&mut model, &final_case, options, &mut incidents, "model")
            .unwrap_or_else(|| Reply::failed(final_case.case, "no reply"));
        let d = ask(&mut implementation, &final_case, options, &mut incidents, "implementation")
            .unwrap_or_else(|| Reply::failed(final_case.case, "no reply"));

        divergences.push(Divergence {
            case: i as u64,
            input: minimal,
            op: binding.op.clone(),
            model: (&m).into(),
            implementation: (&d).into(),
            shrunk_steps: steps,
            triage: Triage::Untriaged,
        });
    }

    coverage.input_constructors_seen = seen_inputs.into_iter().collect();
    coverage.output_variants_seen = seen_outputs.into_iter().collect();
    let coverage_floor_met = coverage.meets_floor(&binding.coverage_floor);

    Ok(DrtResult {
        req_id: binding.req_id.clone(),
        clause: binding.clause.clone(),
        seed: options.seed,
        cases_run,
        divergences,
        coverage,
        coverage_floor_met,
        duration_ms: started.elapsed().as_millis() as u64,
        incidents,
    })
}

/// Ask one side, restarting once on death or timeout.
///
/// A case that killed a runner still counts as run, and is reported as an
/// incident: pretending it did not happen would inflate the case count that an
/// evidence record is built on.
fn ask(
    runner: &mut Runner,
    case: &Case,
    options: &RunOptions,
    incidents: &mut Vec<String>,
    side: &str,
) -> Option<Reply> {
    match runner.ask(case, options.timeout) {
        Ok(reply) => Some(reply),
        Err(RunnerError::Protocol(message)) => {
            incidents.push(format!("case {}: {side} spoke nonsense: {message}", case.case));
            None
        }
        Err(error) => {
            incidents.push(format!("case {}: {side} {error}", case.case));
            match runner.restart(options.max_restarts) {
                Ok(()) => runner.ask(case, options.timeout).ok(),
                Err(e) => {
                    incidents.push(format!("{side} could not be restarted: {e}"));
                    None
                }
            }
        }
    }
}

/// Reduce a failing input while the divergence survives.
fn shrink_divergence(
    model: &mut Runner,
    implementation: &mut Runner,
    binding: &Binding,
    input: &serde_json::Value,
    options: &RunOptions,
    incidents: &mut Vec<String>,
) -> (serde_json::Value, usize) {
    let mut current = input.clone();
    let mut steps = 0usize;
    let mut budget = options.shrink_budget;

    'outer: while budget > 0 {
        for candidate in gen::shrink(&current) {
            if budget == 0 {
                break 'outer;
            }
            budget -= 1;

            let case = Case { case: u64::MAX, op: binding.op.clone(), input: candidate.clone() };
            let (Some(m), Some(d)) = (
                ask(model, &case, options, incidents, "model"),
                ask(implementation, &case, options, incidents, "implementation"),
            ) else {
                continue;
            };

            if !replies_agree(&m, &d) {
                current = candidate;
                steps += 1;
                continue 'outer; // smaller input still diverges; keep going
            }
        }
        break; // nothing smaller reproduced it
    }

    (current, steps)
}

/// Turn a result into the evidence record the checker reads.
///
/// The hashes come from the trace index rather than being recomputed here, so
/// that staleness is judged against exactly the inputs the checker knows about.
pub fn to_evidence(
    result: &DrtResult,
    input_hashes: BTreeMap<String, String>,
    lean_version: String,
    node: Option<String>,
) -> crate::trace::EvidenceRecord {
    crate::trace::EvidenceRecord {
        key: crate::trace::EvidenceKey {
            req_id: result.req_id.clone(),
            clause: result.clause.clone(),
            bond: crate::trace::Bond::ModelImpl,
        },
        level: if result.earns_l3() {
            crate::trace::Level::L3
        } else {
            crate::trace::Level::L1
        },
        input_hashes,
        detail: crate::trace::EvidenceDetail::Drt {
            seed: result.seed,
            cases: result.cases_run,
            divergences: result.divergences.len(),
            coverage_covered: result.coverage.covered(),
            coverage_total: result.coverage.total(),
            lean_version,
        },
        at: chrono::Utc::now().to_rfc3339(),
        node,
    }
}

/// Append confirmed divergence inputs to the binding's seed corpus, so a case
/// that once broke this binding is replayed on every future run.
pub fn append_seeds(root: &Path, req_id: &str, inputs: &[serde_json::Value]) -> Result<(), String> {
    if inputs.is_empty() {
        return Ok(());
    }
    let dir = root.join(".tracelean").join("drt").join("seeds");
    std::fs::create_dir_all(&dir).map_err(|e| format!("creating {}: {e}", dir.display()))?;
    let path = dir.join(format!("{}.jsonl", sanitize(req_id)));

    let mut existing: BTreeSet<String> = std::fs::read_to_string(&path)
        .map(|text| text.lines().map(|l| l.to_string()).collect())
        .unwrap_or_default();
    for input in inputs {
        if let Ok(line) = serde_json::to_string(input) {
            existing.insert(line);
        }
    }

    let mut body: Vec<String> = existing.into_iter().filter(|l| !l.trim().is_empty()).collect();
    body.sort();
    std::fs::write(&path, body.join("\n") + "\n")
        .map_err(|e| format!("writing {}: {e}", path.display()))
}

/// Load a binding's seed corpus.
pub fn load_seeds(root: &Path, req_id: &str) -> Vec<serde_json::Value> {
    let path = root
        .join(".tracelean")
        .join("drt")
        .join("seeds")
        .join(format!("{}.jsonl", sanitize(req_id)));
    std::fs::read_to_string(path)
        .map(|text| {
            text.lines()
                .filter(|l| !l.trim().is_empty())
                .filter_map(|l| serde_json::from_str(l).ok())
                .collect()
        })
        .unwrap_or_default()
}

fn sanitize(id: &str) -> String {
    id.chars()
        .map(|c| if c.is_ascii_alphanumeric() || c == '-' || c == '_' { c } else { '_' })
        .collect()
}

