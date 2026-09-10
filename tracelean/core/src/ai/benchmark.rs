//! Aider Exercism benchmark integration for telemetry regression testing.

use serde::{Deserialize, Serialize};
use std::time::Instant;

use super::tracking::{CostEstimate, SessionStats, TokenUsage};

/// Max cost cap in USD before aborting a benchmark run.
const DEFAULT_COST_CAP_USD: f64 = 1.0;

/// Result of a single benchmark task.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BenchmarkTask {
    pub name: String,
    pub passed: bool,
    pub tool_calls: u32,
    pub model_iterations: u32,
    pub execution_time_ms: u64,
    pub token_usage: TokenUsage,
    pub cost: CostEstimate,
    pub parsing_failures: u32,
    pub error: Option<String>,
}

/// Aggregate results of a full benchmark run.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BenchmarkRun {
    pub tasks: Vec<BenchmarkTask>,
    pub session_stats: SessionStats,
    pub total_time_ms: u64,
    pub pass_count: u32,
    pub fail_count: u32,
    pub aborted: bool,
    pub abort_reason: Option<String>,
    pub cost_cap_usd: f64,
}

impl BenchmarkRun {
    pub fn pass_rate(&self) -> f64 {
        let total = self.pass_count + self.fail_count;
        if total == 0 {
            return 0.0;
        }
        self.pass_count as f64 / total as f64
    }
}

/// Configuration for a benchmark run.
#[derive(Debug, Clone)]
pub struct BenchmarkConfig {
    pub cost_cap_usd: f64,
    pub task_names: Vec<String>,
}

impl Default for BenchmarkConfig {
    fn default() -> Self {
        Self {
            cost_cap_usd: DEFAULT_COST_CAP_USD,
            task_names: Vec::new(),
        }
    }
}

/// Execute a benchmark run with cost cap enforcement.
///
/// `execute_task` is a closure that runs a single exercism task by name
/// and returns `(passed, tool_calls, model_iterations, parsing_failures, token_usage, cost, error)`.
pub fn run_benchmark<F>(config: BenchmarkConfig, mut execute_task: F) -> BenchmarkRun
where
    F: FnMut(&str) -> (bool, u32, u32, u32, TokenUsage, CostEstimate, Option<String>),
{
    let run_start = Instant::now();
    let mut session_stats = SessionStats::default();
    let mut tasks = Vec::new();
    let mut aborted = false;
    let mut abort_reason = None;

    for task_name in &config.task_names {
        // Cost cap check before each task
        if session_stats.total_cost_usd >= config.cost_cap_usd {
            aborted = true;
            abort_reason = Some(format!(
                "Cost cap ${:.2} exceeded (spent ${:.4})",
                config.cost_cap_usd, session_stats.total_cost_usd
            ));
            break;
        }

        let task_start = Instant::now();
        let (passed, tool_calls, model_iterations, parsing_failures, usage, cost, error) =
            execute_task(task_name);
        let elapsed = task_start.elapsed();

        session_stats.record(&usage, &cost);

        tasks.push(BenchmarkTask {
            name: task_name.clone(),
            passed,
            tool_calls,
            model_iterations,
            execution_time_ms: elapsed.as_millis() as u64,
            token_usage: usage,
            cost,
            parsing_failures,
            error,
        });

        // Post-task cost cap check
        if session_stats.total_cost_usd >= config.cost_cap_usd {
            aborted = true;
            abort_reason = Some(format!(
                "Cost cap ${:.2} exceeded after task '{}' (spent ${:.4})",
                config.cost_cap_usd, task_name, session_stats.total_cost_usd
            ));
            break;
        }
    }

    let pass_count = tasks.iter().filter(|t| t.passed).count() as u32;
    let fail_count = tasks.iter().filter(|t| !t.passed).count() as u32;

    BenchmarkRun {
        tasks,
        session_stats,
        total_time_ms: run_start.elapsed().as_millis() as u64,
        pass_count,
        fail_count,
        aborted,
        abort_reason,
        cost_cap_usd: config.cost_cap_usd,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cost_cap_abort() {
        let config = BenchmarkConfig {
            cost_cap_usd: 0.01,
            task_names: vec!["task1".into(), "task2".into(), "task3".into()],
        };

        let mut call_count = 0;
        let result = run_benchmark(config, |_name| {
            call_count += 1;
            let usage = TokenUsage {
                input_tokens: 10000,
                output_tokens: 5000,
                thinking_tokens: 0,
                cached_tokens: 0,
                cache_write_tokens: 0,
            };
            // ~$0.015 per call at typical pricing
            let cost = CostEstimate {
                total_usd: 0.006,
                input_cost: 0.003,
                output_cost: 0.003,
                cached_savings: 0.0,
                write_cost: 0.0,
            };
            (true, 3, 1, 0, usage, cost, None)
        });

        assert!(result.aborted);
        assert!(result.tasks.len() < 3);
    }

    #[test]
    fn test_pass_rate() {
        let config = BenchmarkConfig {
            cost_cap_usd: 100.0,
            task_names: vec!["a".into(), "b".into(), "c".into(), "d".into()],
        };

        let mut i = 0;
        let result = run_benchmark(config, |_| {
            i += 1;
            let usage = TokenUsage::default();
            let cost = CostEstimate {
                total_usd: 0.0001,
                input_cost: 0.0,
                output_cost: 0.0001,
                cached_savings: 0.0,
                write_cost: 0.0,
            };
            (i % 2 == 0, 1, 1, 0, usage, cost, None)
        });

        assert_eq!(result.pass_count, 2);
        assert_eq!(result.fail_count, 2);
        assert!((result.pass_rate() - 0.5).abs() < f64::EPSILON);
    }
}
