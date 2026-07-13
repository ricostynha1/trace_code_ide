//! Integration tests for the Bedrock provider (bedrock-mantle endpoint).
//! These tests require:
//!   1. Feature flag: --features live_bedrock
//!   2. Env var: AWS_BEARER_TOKEN_BEDROCK
//! If either is missing, tests are skipped.
//!
//! Run with: cargo test -p tracelean-tests --test integration_bedrock --features live_bedrock
//!
//! Uses cheap models (ministral-3b) to minimize cost. Qwen test uses qwen3-32b.

#![cfg(feature = "live_bedrock")]

use tracelean_core::ai::provider::*;
use tracelean_core::ai::bedrock::BedrockProvider;

/// Cheapest model on bedrock-mantle for testing.
const TEST_MODEL: &str = "mistral.ministral-3-3b-instruct";
/// Region where we confirmed models are available.
const TEST_REGION: &str = "us-east-1";

/// Get bearer token or skip test.
fn get_token() -> Option<String> {
    std::env::var("AWS_BEARER_TOKEN_BEDROCK").ok().filter(|s| !s.is_empty())
}

fn make_provider(token: &str) -> BedrockProvider {
    BedrockProvider::new(token.to_string(), Some(TEST_REGION.to_string()))
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
        extra_params: None,
        coding_index: None,
        coding_rank: None,
        supports_caching: false,
        supports_tools: false,
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Basic completion
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
        messages: vec![ChatMessage {
            role: MessageRole::User,
            content: "Reply with exactly the word 'hello'".to_string(),
            tool_call_id: None,
            tool_calls: vec![],
        }],
        stop: None,
        tools: None,
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
// Tool calling
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
        messages: vec![ChatMessage {
            role: MessageRole::User,
            content: "What's the weather in Paris? Use the tool.".to_string(),
            tool_call_id: None,
            tool_calls: vec![],
        }],
        stop: None,
        tools: Some(tools),
    };

    let resp = provider.complete(&request).await;
    assert!(resp.is_ok(), "Expected success, got: {:?}", resp.err());

    let resp = resp.unwrap();
    assert!(!resp.tool_calls.is_empty(), "Expected tool_calls, got text: {}", resp.content);

    let call = &resp.tool_calls[0];
    assert_eq!(call.function.name, "get_weather");
    assert_eq!(call.call_type, "function");
    // Arguments should parse as JSON with "city" field
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
        messages: vec![ChatMessage {
            role: MessageRole::User,
            content: "What is 7 * 13? Use the calculator tool.".to_string(),
            tool_call_id: None,
            tool_calls: vec![],
        }],
        stop: None,
        tools: Some(tools),
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

    // Should contain qwen.qwen3-32b
    let has_qwen = models.iter().any(|m| m.model_id == "qwen.qwen3-32b");
    assert!(has_qwen, "Expected qwen.qwen3-32b in model list, got: {:?}",
        models.iter().map(|m| &m.model_id).collect::<Vec<_>>());
}

// ─────────────────────────────────────────────────────────────────────────────
// Model ID conversion (unit test — no network needed)
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_to_mantle_model_id_conversion() {
    // Runtime-style → mantle-style
    assert_eq!(
        BedrockProvider::to_mantle_model_id("eu.amazon.nova-lite-v1:0"),
        "amazon.nova-lite"
    );
    assert_eq!(
        BedrockProvider::to_mantle_model_id("qwen.qwen3-32b-v1:0"),
        "qwen.qwen3-32b"
    );
    assert_eq!(
        BedrockProvider::to_mantle_model_id("us.anthropic.claude-sonnet-4-6"),
        "anthropic.claude-sonnet-4-6"
    );
    // Already mantle-style — should pass through unchanged
    assert_eq!(
        BedrockProvider::to_mantle_model_id("qwen.qwen3-32b"),
        "qwen.qwen3-32b"
    );
    assert_eq!(
        BedrockProvider::to_mantle_model_id("mistral.ministral-3-3b-instruct"),
        "mistral.ministral-3-3b-instruct"
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// Auth failure test
// ─────────────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn test_bedrock_bad_token_returns_auth_error() {
    let provider = BedrockProvider::new("invalid-token-xxx".to_string(), Some(TEST_REGION.to_string()));
    let request = AiRequest {
        model: test_model_config(),
        messages: vec![ChatMessage {
            role: MessageRole::User,
            content: "hi".to_string(),
            tool_call_id: None,
            tool_calls: vec![],
        }],
        stop: None,
        tools: None,
    };

    let resp = provider.complete(&request).await;
    assert!(resp.is_err());
    let err = resp.unwrap_err();
    assert_eq!(err.kind, AiErrorKind::Authentication);
}
