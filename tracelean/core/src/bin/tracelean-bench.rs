//! tracelean-bench — headless benchmark runner.
//!
//! Runs exercism tasks using the agent runtime without any UI.
//! Outputs JSON compatible with BenchmarkRun.
//!
//! Usage:
//!   cargo run --bin tracelean-bench -- --provider openrouter --model anthropic/claude-sonnet-4-20250514 hello-world
//!   cargo run --bin tracelean-bench -- --provider bedrock --model us.anthropic.claude-sonnet-4-20250514-v1:0 --all

use std::path::PathBuf;
use std::time::Instant;

use tracelean_core::agent::{AgentContext, run_agent_turn};
use tracelean_core::ai::benchmark::{BenchmarkConfig, run_benchmark};
use tracelean_core::ai::provider::{ChatMessage, MessageRole};
use tracelean_core::ai::tracking::{CostEstimate, TokenUsage};
use tracelean_core::ai::{ModelConfig, ProviderKind};
use tracelean_core::AiSettings;

// ─── CLI parsing ─────────────────────────────────────────────────────────────

struct CliArgs {
    provider: Option<String>,
    model: Option<String>,
    region: Option<String>,
    cost_cap: f64,
    task_names: Vec<String>,
    list_models: bool,
    verbose: bool,
}

fn parse_args() -> CliArgs {
    let args: Vec<String> = std::env::args().skip(1).collect();

    if args.is_empty() || args.iter().any(|a| a == "--help" || a == "-h") {
        print_usage();
        std::process::exit(if args.is_empty() { 1 } else { 0 });
    }

    let mut provider = None;
    let mut model = None;
    let mut region = None;
    let mut cost_cap = 1.0_f64;
    let mut task_names = Vec::new();
    let mut list_models = false;
    let mut verbose = false;
    let mut i = 0;

    while i < args.len() {
        match args[i].as_str() {
            "--provider" | "-p" => {
                i += 1;
                provider = args.get(i).cloned();
            }
            "--model" | "-m" => {
                i += 1;
                model = args.get(i).cloned();
            }
            "--region" | "-r" => {
                i += 1;
                region = args.get(i).cloned();
            }
            "--cap" | "-c" => {
                i += 1;
                if let Some(v) = args.get(i).and_then(|s| s.parse().ok()) {
                    cost_cap = v;
                }
            }
            "--all" => {
                task_names = default_exercism_tasks();
            }
            "--list-models" | "-l" => {
                list_models = true;
            }
            "--verbose" | "-v" => {
                verbose = true;
            }
            arg if arg.starts_with('-') => {
                eprintln!("Unknown flag: {}", arg);
                print_usage();
                std::process::exit(1);
            }
            task => {
                task_names.push(task.to_string());
            }
        }
        i += 1;
    }

    // Env var fallbacks
    if provider.is_none() {
        provider = std::env::var("PROVIDER").ok();
    }
    if model.is_none() {
        model = std::env::var("MODEL_ID").ok();
    }
    if region.is_none() {
        region = std::env::var("AWS_REGION").ok();
    }
    if let Ok(cap) = std::env::var("SPEND_CAP_USD") {
        if let Ok(v) = cap.parse::<f64>() {
            cost_cap = v;
        }
    }

    CliArgs { provider, model, region, cost_cap, task_names, list_models, verbose }
}

fn print_usage() {
    eprintln!("tracelean-bench — headless agent benchmark runner");
    eprintln!("");
    eprintln!("USAGE:");
    eprintln!("  tracelean-bench [OPTIONS] <task>... | --all");
    eprintln!("");
    eprintln!("OPTIONS:");
    eprintln!("  -p, --provider <NAME>   Provider: openrouter | bedrock | mock");
    eprintln!("  -m, --model <ID>        Model ID (e.g. anthropic/claude-sonnet-4-20250514)");
    eprintln!("  -r, --region <REGION>   AWS region for Bedrock (default: eu-west-1)");
    eprintln!("  -c, --cap <USD>         Spend cap in USD (default: 1.0)");
    eprintln!("  -l, --list-models       List available models for the provider and exit");
    eprintln!("  -v, --verbose           Show prompts, responses, and tool calls");
    eprintln!("      --all               Run all default exercism tasks");
    eprintln!("  -h, --help              Show this help");
    eprintln!("");
    eprintln!("ENVIRONMENT (fallbacks):");
    eprintln!("  OPENROUTER_API_KEY      OpenRouter API key");
    eprintln!("  AWS_BEARER_TOKEN_BEDROCK  Bedrock bearer token");
    eprintln!("  AWS_REGION              Bedrock region (default: eu-west-1)");
    eprintln!("  PROVIDER                Same as --provider");
    eprintln!("  MODEL_ID                Same as --model");
    eprintln!("  SPEND_CAP_USD           Same as --cap");
    eprintln!("");
    eprintln!("EXAMPLES:");
    eprintln!("  tracelean-bench -p openrouter -m anthropic/claude-sonnet-4-20250514 hello-world");
    eprintln!("  tracelean-bench -p bedrock -m us.anthropic.claude-sonnet-4-20250514-v1:0 --all");
    eprintln!("  tracelean-bench --cap 0.50 --all > results.json");
}

// ─── Main ────────────────────────────────────────────────────────────────────

#[tokio::main]
async fn main() {
    let cli = parse_args();

    if cli.list_models {
        list_available_models(cli.provider.as_deref(), cli.region.as_deref()).await;
        return;
    }

    if cli.task_names.is_empty() {
        eprintln!("Error: no tasks specified. Use task names or --all.");
        std::process::exit(1);
    }

    let config = BenchmarkConfig {
        cost_cap_usd: cli.cost_cap,
        task_names: cli.task_names.clone(),
    };

    // Resolve provider + model for display
    let provider_name = cli.provider.as_deref().unwrap_or("(auto-detect from env)");
    let model_name = cli.model.as_deref().unwrap_or("anthropic/claude-sonnet-4-20250514");
    let region_name = cli.region.as_deref().unwrap_or("eu-west-1");
    eprintln!("[bench] provider={}, model={}, region={}", provider_name, model_name, region_name);
    eprintln!("[bench] running {} tasks, cost cap ${:.2}", cli.task_names.len(), cli.cost_cap);

    let cli_provider = cli.provider.clone();
    let cli_model = cli.model.clone();
    let cli_region = cli.region.clone();
    let cli_cap = cli.cost_cap;
    let cli_verbose = cli.verbose;

    let result = run_benchmark(config, |task_name| {
        // block_in_place allows blocking inside multi-threaded tokio runtime
        tokio::task::block_in_place(|| {
            tokio::runtime::Handle::current().block_on(execute_exercism_task(
                task_name,
                cli_provider.as_deref(),
                cli_model.as_deref(),
                cli_region.as_deref(),
                cli_cap,
                cli_verbose,
            ))
        })
    });

    // Output JSON result
    match serde_json::to_string_pretty(&result) {
        Ok(json) => println!("{}", json),
        Err(e) => {
            eprintln!("[bench] failed to serialize results: {}", e);
            std::process::exit(1);
        }
    }

    eprintln!(
        "[bench] done: {}/{} passed, ${:.4} spent, {}ms",
        result.pass_count,
        result.pass_count + result.fail_count,
        result.session_stats.total_cost_usd,
        result.total_time_ms,
    );
}

// ─── Task execution ──────────────────────────────────────────────────────────

async fn execute_exercism_task(
    task_name: &str,
    cli_provider: Option<&str>,
    cli_model: Option<&str>,
    cli_region: Option<&str>,
    cost_cap: f64,
    verbose: bool,
) -> (bool, u32, u32, u32, TokenUsage, CostEstimate, Option<String>) {
    let project_root = PathBuf::from(format!("/tmp/tracelean-bench/{}", task_name));

    // Clean and recreate the working directory so each run starts fresh
    let _ = tokio::fs::remove_dir_all(&project_root).await;
    tokio::fs::create_dir_all(&project_root).await.expect("failed to create working dir");

    // Copy all task files (including .docs/, .meta/, tests) from source into working dir
    let source_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../exercism_tasks")
        .join(task_name);
    if source_dir.exists() {
        copy_dir_recursive(&source_dir, &project_root)
            .await
            .unwrap_or_else(|e| {
                eprintln!("[bench] WARNING: failed to copy task files for '{}': {}", task_name, e);
            });
        eprintln!("[bench] copied task files: {} -> {}", source_dir.display(), project_root.display());
    } else {
        eprintln!("[bench] WARNING: source task dir not found: {}", source_dir.display());
    }

    let mut ctx = AgentContext::from_env(project_root);
    ctx.verbose = verbose;

    // Configure settings from CLI + env
    {
        let mut settings = ctx.settings.lock().unwrap();
        configure_settings(&mut settings, cli_provider, cli_model, cli_region, cost_cap);
    }

    let prompt = format!(
        "Solve the exercism '{}' task in Python.\n\
         1. Read the instructions from the .docs/ directory\n\
         2. Implement the solution\n\
         3. Run `pytest` to verify it passes\n\
         If tests fail, fix and re-run until they pass.",
        task_name
    );

    let messages = vec![ChatMessage {
        role: MessageRole::User,
        content: prompt,
        tool_call_id: None,
        tool_calls: Vec::new(),
    }];

    let start = Instant::now();
    let result = run_agent_turn(&ctx, messages).await;
    let _duration_ms = start.elapsed().as_millis() as u64;

    match result {
        Ok(turn) => {
            let stats = ctx.stats.lock().unwrap();
            let usage = TokenUsage {
                input_tokens: stats.total_input_tokens as u32,
                output_tokens: stats.total_output_tokens as u32,
                thinking_tokens: stats.total_thinking_tokens as u32,
                cached_tokens: stats.total_cached_tokens as u32,
            };
            let cost = CostEstimate {
                total_usd: stats.total_cost_usd,
                input_cost: 0.0,
                output_cost: 0.0,
                cached_savings: 0.0,
            };

            let passed = turn.response.content.to_lowercase().contains("pass")
                || turn.tool_calls_executed.iter().any(|tc| {
                    tc.tool_name == "run_command" && tc.success
                });

            (
                passed,
                turn.tool_calls_executed.len() as u32,
                turn.iterations,
                0,
                usage,
                cost,
                None,
            )
        }
        Err(e) => {
            let usage = TokenUsage::default();
            let cost = CostEstimate {
                total_usd: 0.0,
                input_cost: 0.0,
                output_cost: 0.0,
                cached_savings: 0.0,
            };
            (false, 0, 0, 0, usage, cost, Some(e.to_string()))
        }
    }
}

// ─── Settings ────────────────────────────────────────────────────────────────

/// Configure AiSettings from CLI flags + environment variables.
/// CLI flags take priority over env vars.
fn configure_settings(
    settings: &mut AiSettings,
    cli_provider: Option<&str>,
    cli_model: Option<&str>,
    cli_region: Option<&str>,
    cost_cap: f64,
) {
    // Resolve region (CLI > env > default)
    let region = cli_region
        .map(|s| s.to_string())
        .or_else(|| std::env::var("AWS_REGION").ok())
        .unwrap_or_else(|| "eu-west-1".to_string());

    // Resolve provider (CLI > env > auto-detect)
    let provider_str = cli_provider
        .map(|s| s.to_string())
        .or_else(|| std::env::var("PROVIDER").ok());

    match provider_str.as_deref() {
        Some("openrouter" | "openai") => {
            settings.active_provider = ProviderKind::OpenRouter;
            settings.openrouter_api_key = std::env::var("OPENROUTER_API_KEY").ok();
        }
        Some("bedrock" | "aws") => {
            settings.active_provider = ProviderKind::Bedrock;
            settings.bedrock_api_key = std::env::var("AWS_BEARER_TOKEN_BEDROCK").ok();
            settings.bedrock_region = Some(region.clone());
        }
        Some("mock") => {
            settings.active_provider = ProviderKind::Mock;
        }
        _ => {
            // Auto-detect from available env vars
            if std::env::var("OPENROUTER_API_KEY").is_ok() {
                settings.active_provider = ProviderKind::OpenRouter;
                settings.openrouter_api_key = std::env::var("OPENROUTER_API_KEY").ok();
            } else if std::env::var("AWS_BEARER_TOKEN_BEDROCK").is_ok() {
                settings.active_provider = ProviderKind::Bedrock;
                settings.bedrock_api_key = std::env::var("AWS_BEARER_TOKEN_BEDROCK").ok();
                settings.bedrock_region = Some(region.clone());
            } else {
                eprintln!("[bench] WARNING: no API key found, falling back to mock provider");
                settings.active_provider = ProviderKind::Mock;
            }
        }
    }

    // Resolve model (CLI > env > default)
    let model_id = cli_model
        .map(|s| s.to_string())
        .or_else(|| std::env::var("MODEL_ID").ok())
        .unwrap_or_else(|| "anthropic/claude-sonnet-4-20250514".to_string());

    settings.selected_model = Some(ModelConfig {
        provider: settings.active_provider.clone(),
        model_id,
        display_name: "Bench Model".to_string(),
        max_tokens: 16384,
        temperature: 0.0,
        input_cost_per_m: 3.0,
        output_cost_per_m: 15.0,
        cached_input_cost_per_m: 0.3,
        extra_params: None,
        coding_index: None,
        coding_rank: None,
        supports_caching: false,
        supports_tools: false,
    });

    settings.spend_cap_usd = cost_cap;
}

/// Default exercism tasks for full benchmark.
fn default_exercism_tasks() -> Vec<String> {
    vec![
        "hello-world",
        "two-fer",
        "resistor-color",
        "rna-transcription",
        "reverse-string",
        "pangram",
        "isogram",
        "scrabble-score",
        "luhn",
        "clock",
    ]
    .into_iter()
    .map(String::from)
    .collect()
}

// ─── List models ─────────────────────────────────────────────────────────────

async fn list_available_models(cli_provider: Option<&str>, cli_region: Option<&str>) {
    use tracelean_core::ai::provider::AiProvider;

    let region = cli_region
        .map(|s| s.to_string())
        .or_else(|| std::env::var("AWS_REGION").ok());

    let provider_str = cli_provider
        .map(|s| s.to_string())
        .or_else(|| std::env::var("PROVIDER").ok());

    match provider_str.as_deref() {
        Some("bedrock" | "aws") => {
            let token = match std::env::var("AWS_BEARER_TOKEN_BEDROCK") {
                Ok(t) => t,
                Err(_) => {
                    eprintln!("Error: AWS_BEARER_TOKEN_BEDROCK not set");
                    std::process::exit(1);
                }
            };
            let provider = tracelean_core::ai::bedrock::BedrockProvider::new(token, region);
            match provider.list_models().await {
                Ok(models) => {
                    eprintln!("Available Bedrock models ({}):", models.len());
                    for m in &models {
                        println!("{:<55} ${:.2}/M in, ${:.2}/M out", m.model_id, m.input_cost_per_m, m.output_cost_per_m);
                    }
                }
                Err(e) => eprintln!("Error listing models: {}", e.message),
            }
        }
        Some("openrouter" | "openai") => {
            let key = match std::env::var("OPENROUTER_API_KEY") {
                Ok(k) => k,
                Err(_) => {
                    eprintln!("Error: OPENROUTER_API_KEY not set");
                    std::process::exit(1);
                }
            };
            let provider = tracelean_core::ai::openrouter::OpenRouterProvider::new(key);
            match provider.list_models().await {
                Ok(models) => {
                    eprintln!("Available OpenRouter models ({}):", models.len());
                    for m in &models {
                        println!("{:<55} ${:.2}/M in, ${:.2}/M out", m.model_id, m.input_cost_per_m, m.output_cost_per_m);
                    }
                }
                Err(e) => eprintln!("Error listing models: {}", e.message),
            }
        }
        Some("mock") => {
            eprintln!("Mock provider has 1 model: mock-debug");
        }
        _ => {
            eprintln!("Specify --provider (bedrock or openrouter) to list models.");
            eprintln!("  tracelean-bench -p bedrock --list-models");
            eprintln!("  tracelean-bench -p openrouter --list-models");
        }
    }
}

// ─── Filesystem helpers ──────────────────────────────────────────────────────

/// Recursively copy a directory tree from `src` to `dst`.
/// Preserves directory structure, copies all files including hidden ones (.docs/, .meta/).
async fn copy_dir_recursive(src: &std::path::Path, dst: &std::path::Path) -> std::io::Result<()> {
    use tokio::fs;

    let mut stack = vec![(src.to_path_buf(), dst.to_path_buf())];

    while let Some((src_path, dst_path)) = stack.pop() {
        fs::create_dir_all(&dst_path).await?;
        let mut entries = fs::read_dir(&src_path).await?;

        while let Some(entry) = entries.next_entry().await? {
            let file_type = entry.file_type().await?;
            let src_child = entry.path();
            let dst_child = dst_path.join(entry.file_name());

            if file_type.is_dir() {
                stack.push((src_child, dst_child));
            } else {
                fs::copy(&src_child, &dst_child).await?;
            }
        }
    }

    Ok(())
}
