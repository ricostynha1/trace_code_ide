//! Integration tests for the Bedrock provider (bedrock-mantle endpoint).
//! These tests require:
//!   1. Feature flag: --features live_bedrock
//!   2. Env var: AWS_BEARER_TOKEN_BEDROCK
//! If either is missing, tests are skipped.
//!
//! Run with: cargo test -p tracelean-tests --test integration_bedrock --features live_bedrock
//!
//! Uses cheap models (ministral-3b) to minimize cost. Qwen test uses qwen3-32b.
//! The Claude tests below hit the Bedrock Runtime **Converse** endpoint (see
//! `ai::bedrock::is_claude_model` / `complete_anthropic`) — this account has
//! zero access to bare `anthropic.claude-sonnet-5` via any path, but does have
//! access to Haiku 4.5 through Converse with a cross-region inference-profile
//! prefix, in `eu-west-1` (this account's US regions are broken/quota-zero —
//! see the `project_bedrock_account_quirks` note). If the AWS account hasn't
//! been granted Bedrock model access for a given Anthropic model, Bedrock
//! returns a `permission_error` *before* any inference runs (no cost
//! incurred). Those tests accept that as a "wiring is correct, account isn't
//! provisioned" result rather than failing the whole suite over an
//! account-entitlement gap that isn't a code defect.

#![cfg(feature = "live_bedrock")]

use tracelean_core::ai::provider::*;
use tracelean_core::ai::bedrock::BedrockProvider;

/// Cheapest model on bedrock-mantle for testing.
const TEST_MODEL: &str = "mistral.ministral-3-3b-instruct";
/// Region where we confirmed non-Claude models are available.
const TEST_REGION: &str = "us-east-1";
/// Cheapest Claude model confirmed to have account access via Converse, using
/// the cross-region inference-profile prefix this model requires on Bedrock.
const CLAUDE_MODEL: &str = "eu.anthropic.claude-haiku-4-5-20251001-v1:0";
/// Region confirmed to have quota for Claude models on this account (the US
/// regions are quota-zero/broken for this account).
const CLAUDE_REGION: &str = "eu-west-1";

/// Get bearer token or skip test.
fn get_token() -> Option<String> {
    std::env::var("AWS_BEARER_TOKEN_BEDROCK").ok().filter(|s| !s.is_empty())
}

fn make_provider(token: &str) -> BedrockProvider {
    BedrockProvider::new(token.to_string(), Some(TEST_REGION.to_string()))
}

/// Provider pointed at the region where this account actually has Claude
/// model quota (see `CLAUDE_REGION`).
fn make_claude_provider(token: &str) -> BedrockProvider {
    BedrockProvider::new(token.to_string(), Some(CLAUDE_REGION.to_string()))
}

fn test_model_config() -> ModelConfig {
    ModelConfig {
        provider: ProviderKind::Bedrock,
        model_id: TEST_MODEL.to_string(),
        display_name: TEST_MODEL.to_string(),
        max_tokens: 50,
        temperature: 0.0,
        input_cost_per_m: 0.0,
        output_cost_per_m: 0.0,
        cached_input_cost_per_m: 0.0,
        supports_caching: false,
        supports_tools: false,
        ..Default::default()
    }
}

fn claude_model_config() -> ModelConfig {
    ModelConfig {
        provider: ProviderKind::Bedrock,
        model_id: CLAUDE_MODEL.to_string(),
        display_name: CLAUDE_MODEL.to_string(),
        max_tokens: 200,
        temperature: 0.0,
        ..Default::default()
    }
}

fn msg(role: MessageRole, content: &str) -> ChatMessage {
    ChatMessage { role, content: content.into(), tool_call_id: None, tool_calls: Vec::new() }
}

/// True if this looks like Bedrock's "model not available for this account /
/// contact AWS Sales" account-entitlement error rather than a code defect.
fn is_account_entitlement_error(err: &AiError) -> bool {
    let m = err.message.to_lowercase();
    m.contains("not available for this account") || m.contains("permission_error")
}

// ─────────────────────────────────────────────────────────────────────────────
// Basic completion (non-Claude, OpenAI-compatible path — unchanged behavior)
// ─────────────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn test_bedrock_basic_completion() {
    let token = match get_token() {
        Some(t) => t,
        None => {
            eprintln!("SKIPPED: AWS_BEARER_TOKEN_BEDROCK not set");
            return;
        }
    };

    let provider = make_provider(&token);
    let request = AiRequest {
        model: test_model_config(),
        messages: vec![msg(MessageRole::User, "Reply with exactly the word 'hello'")],
        stop: None,
        tools: None,
        dynamic_tools: None,
        cache_breakpoints: Vec::new(),
    };

    let resp = provider.complete(&request).await;
    assert!(resp.is_ok(), "Expected success, got: {:?}", resp.err());

    let resp = resp.unwrap();
    assert!(!resp.content.is_empty(), "Expected non-empty content");
    assert!(resp.usage.input_tokens > 0, "Expected input tokens > 0");
    assert!(resp.usage.output_tokens > 0, "Expected output tokens > 0");
    assert!(!resp.truncated);
}

// ─────────────────────────────────────────────────────────────────────────────
// Tool calling (non-Claude, OpenAI-compatible path)
// ─────────────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn test_bedrock_tool_calling() {
    let token = match get_token() {
        Some(t) => t,
        None => {
            eprintln!("SKIPPED: AWS_BEARER_TOKEN_BEDROCK not set");
            return;
        }
    };

    let provider = make_provider(&token);
    let tools = vec![ToolSchema {
        tool_type: "function".to_string(),
        function: ToolFunction {
            name: "get_weather".to_string(),
            description: "Get current weather for a city".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "city": {"type": "string", "description": "City name"}
                },
                "required": ["city"]
            }),
        },
    }];

    let request = AiRequest {
        model: test_model_config(),
        messages: vec![msg(MessageRole::User, "What's the weather in Paris? Use the tool.")],
        stop: None,
        tools: Some(tools),
        dynamic_tools: None,
        cache_breakpoints: Vec::new(),
    };

    let resp = provider.complete(&request).await;
    assert!(resp.is_ok(), "Expected success, got: {:?}", resp.err());

    let resp = resp.unwrap();
    assert!(!resp.tool_calls.is_empty(), "Expected tool_calls, got text: {}", resp.content);

    let call = &resp.tool_calls[0];
    assert_eq!(call.function.name, "get_weather");
    assert_eq!(call.call_type, "function");
    let args: serde_json::Value = serde_json::from_str(&call.function.arguments)
        .expect("tool call arguments should be valid JSON");
    assert!(args.get("city").is_some(), "Expected 'city' in args: {}", call.function.arguments);
}

// ─────────────────────────────────────────────────────────────────────────────
// Tool calling with qwen model (the actual target model)
// ─────────────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn test_bedrock_qwen_tool_calling() {
    let token = match get_token() {
        Some(t) => t,
        None => {
            eprintln!("SKIPPED: AWS_BEARER_TOKEN_BEDROCK not set");
            return;
        }
    };

    let provider = make_provider(&token);
    let mut model = test_model_config();
    model.model_id = "qwen.qwen3-32b".to_string();
    model.max_tokens = 100;

    let tools = vec![ToolSchema {
        tool_type: "function".to_string(),
        function: ToolFunction {
            name: "calculator".to_string(),
            description: "Evaluate a math expression".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "expression": {"type": "string", "description": "Math expression"}
                },
                "required": ["expression"]
            }),
        },
    }];

    let request = AiRequest {
        model,
        messages: vec![msg(MessageRole::User, "What is 7 * 13? Use the calculator tool.")],
        stop: None,
        tools: Some(tools),
        dynamic_tools: None,
        cache_breakpoints: Vec::new(),
    };

    let resp = provider.complete(&request).await;
    assert!(resp.is_ok(), "Expected success, got: {:?}", resp.err());

    let resp = resp.unwrap();
    assert!(!resp.tool_calls.is_empty(), "Expected tool_calls from qwen, got: {}", resp.content);
    assert_eq!(resp.tool_calls[0].function.name, "calculator");
}

// ─────────────────────────────────────────────────────────────────────────────
// List models
// ─────────────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn test_bedrock_list_models() {
    let token = match get_token() {
        Some(t) => t,
        None => {
            eprintln!("SKIPPED: AWS_BEARER_TOKEN_BEDROCK not set");
            return;
        }
    };

    let provider = make_provider(&token);
    let models = provider.list_models().await;
    assert!(models.is_ok(), "Expected model list, got: {:?}", models.err());

    let models = models.unwrap();
    assert!(!models.is_empty(), "Expected at least one model");

    let has_qwen = models.iter().any(|m| m.model_id == "qwen.qwen3-32b");
    assert!(has_qwen, "Expected qwen.qwen3-32b in model list, got: {:?}",
        models.iter().map(|m| &m.model_id).collect::<Vec<_>>());

    // The whole point of this fix: Claude models must be discoverable so the
    // app's model picker can offer them (see ai::bedrock::is_claude_model).
    let has_claude_sonnet_5 = models.iter().any(|m| m.model_id == CLAUDE_MODEL);
    assert!(has_claude_sonnet_5, "Expected {} in model list, got: {:?}",
        CLAUDE_MODEL, models.iter().map(|m| &m.model_id).collect::<Vec<_>>());
}

// ─────────────────────────────────────────────────────────────────────────────
// Auth failure test (OpenAI-compatible path)
// ─────────────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn test_bedrock_bad_token_returns_auth_error() {
    let provider = BedrockProvider::new("invalid-token-xxx".to_string(), Some(TEST_REGION.to_string()));
    let request = AiRequest {
        model: test_model_config(),
        messages: vec![msg(MessageRole::User, "hi")],
        stop: None,
        tools: None,
        dynamic_tools: None,
        cache_breakpoints: Vec::new(),
    };

    let resp = provider.complete(&request).await;
    assert!(resp.is_err());
    let err = resp.unwrap_err();
    assert_eq!(err.kind, AiErrorKind::Authentication);
}

// ─────────────────────────────────────────────────────────────────────────────
// Claude — Bedrock Runtime Converse API path
// ─────────────────────────────────────────────────────────────────────────────

/// A bad token against the Converse endpoint must surface as a clean
/// `AiErrorKind::Authentication` carrying Bedrock's own error message — not a
/// parse failure — proving request/response shapes are right independent of
/// whether this AWS account has Claude model access.
#[tokio::test]
async fn test_bedrock_claude_bad_token_returns_auth_error() {
    let provider = BedrockProvider::new("invalid-token-xxx".to_string(), Some(CLAUDE_REGION.to_string()));
    let request = AiRequest {
        model: claude_model_config(),
        messages: vec![msg(MessageRole::User, "hi")],
        stop: None,
        tools: None,
        dynamic_tools: None,
        cache_breakpoints: Vec::new(),
    };

    let resp = provider.complete(&request).await;
    let err = resp.expect_err("bad token must fail");
    assert_eq!(err.kind, AiErrorKind::Authentication);
    assert!(!err.message.is_empty());
}

#[tokio::test]
async fn test_bedrock_claude_basic_completion() {
    let token = match get_token() {
        Some(t) => t,
        None => {
            eprintln!("SKIPPED: AWS_BEARER_TOKEN_BEDROCK not set");
            return;
        }
    };

    let provider = make_claude_provider(&token);
    let request = AiRequest {
        model: claude_model_config(),
        messages: vec![msg(MessageRole::User, "Reply with exactly the word 'hello'")],
        stop: None,
        tools: None,
        dynamic_tools: None,
        cache_breakpoints: Vec::new(),
    };

    match provider.complete(&request).await {
        Ok(resp) => {
            assert!(!resp.content.is_empty(), "Expected non-empty content");
            assert!(resp.usage.input_tokens > 0, "Expected input tokens > 0");
            assert!(resp.usage.output_tokens > 0, "Expected output tokens > 0");
        }
        Err(err) if is_account_entitlement_error(&err) => {
            eprintln!(
                "SKIPPED: AWS account lacks Bedrock model access for {} ({}). \
                 Grant access in AWS Console > Bedrock > Model access to fully verify.",
                CLAUDE_MODEL, err.message
            );
        }
        Err(err) => panic!("Expected success or account-entitlement error, got: {:?}", err),
    }
}

#[tokio::test]
async fn test_bedrock_claude_tool_calling() {
    let token = match get_token() {
        Some(t) => t,
        None => {
            eprintln!("SKIPPED: AWS_BEARER_TOKEN_BEDROCK not set");
            return;
        }
    };

    let provider = make_claude_provider(&token);
    let tools = vec![ToolSchema {
        tool_type: "function".to_string(),
        function: ToolFunction {
            name: "get_weather".to_string(),
            description: "Get current weather for a city".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "city": {"type": "string", "description": "City name"}
                },
                "required": ["city"]
            }),
        },
    }];

    let request = AiRequest {
        model: claude_model_config(),
        messages: vec![msg(MessageRole::User, "What's the weather in Paris? Use the tool.")],
        stop: None,
        tools: Some(tools),
        dynamic_tools: None,
        cache_breakpoints: Vec::new(),
    };

    match provider.complete(&request).await {
        Ok(resp) => {
            assert!(!resp.tool_calls.is_empty(), "Expected tool_calls, got text: {}", resp.content);
            let call = &resp.tool_calls[0];
            assert_eq!(call.function.name, "get_weather");
            assert_eq!(call.call_type, "function");
            let args: serde_json::Value = serde_json::from_str(&call.function.arguments)
                .expect("tool call arguments should be valid JSON");
            assert!(args.get("city").is_some(), "Expected 'city' in args: {}", call.function.arguments);
        }
        Err(err) if is_account_entitlement_error(&err) => {
            eprintln!(
                "SKIPPED: AWS account lacks Bedrock model access for {} ({}).",
                CLAUDE_MODEL, err.message
            );
        }
        Err(err) => panic!("Expected success or account-entitlement error, got: {:?}", err),
    }
}

/// Two-turn conversation with a cache breakpoint after the (large-ish) system
/// prompt: the second call should read from cache. Directly verifies the bug
/// this change fixes — `cache_breakpoints` reaching the wire as
/// `cache_control` and the response's `cache_read_input_tokens` flowing into
/// `TokenUsage.cached_tokens`.
#[tokio::test]
async fn test_bedrock_claude_prompt_caching_hits_on_second_call() {
    let token = match get_token() {
        Some(t) => t,
        None => {
            eprintln!("SKIPPED: AWS_BEARER_TOKEN_BEDROCK not set");
            return;
        }
    };

    let provider = make_claude_provider(&token);
    // Haiku 4.5's minimum cacheable prefix is 4096 tokens; pad the system
    // prompt well past that (~6750 tokens) so caching can actually engage
    // instead of silently no-op'ing on a too-short prefix.
    let system_prompt = "You are a careful assistant. ".repeat(900);

    let mut model = claude_model_config();
    model.max_tokens = 30;

    let make_request = |cache_breakpoints: Vec<usize>| AiRequest {
        model: model.clone(),
        messages: vec![
            msg(MessageRole::System, &system_prompt),
            msg(MessageRole::User, "Reply with exactly the word 'hello'"),
        ],
        stop: None,
        tools: None,
        dynamic_tools: None,
        cache_breakpoints,
    };

    let first = provider.complete(&make_request(vec![0])).await;
    let first = match first {
        Ok(r) => r,
        Err(err) if is_account_entitlement_error(&err) => {
            eprintln!(
                "SKIPPED: AWS account lacks Bedrock model access for {} ({}).",
                CLAUDE_MODEL, err.message
            );
            return;
        }
        Err(err) => panic!("first call failed: {:?}", err),
    };
    assert!(!first.content.is_empty());
    eprintln!("[cache test] first call usage:  {:?}", first.usage);

    let second = provider.complete(&make_request(vec![0])).await
        .expect("second call should succeed if the first did");
    eprintln!("[cache test] second call usage: {:?}", second.usage);
    assert!(
        second.usage.cached_tokens > 0,
        "Expected cache_read_input_tokens > 0 on the second call with an identical, cache-marked system prompt — got usage: {:?}",
        second.usage
    );
}
