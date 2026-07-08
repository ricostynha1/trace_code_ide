import { useState, useEffect, useRef } from "react";
import { invoke } from "@tauri-apps/api/core";

interface MockPendingRequest {
  id: string;
  raw_request_json: string;
  model: { display_name: string };
  timestamp: string;
}

/**
 * Full-screen modal overlay for mock AI mode.
 * Shows the exact JSON request that would be sent to the LLM API
 * (messages + tools + model params). User pastes back the full JSON response.
 */
export function MockPromptWindow() {
  const [pending, setPending] = useState<MockPendingRequest | null>(null);
  const [response, setResponse] = useState("");
  const [submitting, setSubmitting] = useState(false);
  const textareaRef = useRef<HTMLTextAreaElement>(null);

  // Poll for mock pending requests
  useEffect(() => {
    let active = true;
    const poll = async () => {
      while (active) {
        try {
          const requests = await invoke<MockPendingRequest[]>("get_mock_pending");
          if (requests.length > 0 && active) {
            setPending(requests[0]);
          } else if (active) {
            setPending(null);
          }
        } catch {}
        await new Promise((r) => setTimeout(r, 500));
      }
    };
    poll();
    return () => { active = false; };
  }, []);

  // Focus textarea when prompt appears
  useEffect(() => {
    if (pending && textareaRef.current) {
      textareaRef.current.focus();
    }
  }, [pending]);

  const handleSubmit = async () => {
    if (!pending || !response.trim()) return;
    setSubmitting(true);
    try {
      await invoke("mock_submit_response", {
        requestId: pending.id,
        response: response.trim(),
      });
      setResponse("");
      setPending(null);
    } catch (e) {
      console.error("Failed to submit mock response:", e);
    } finally {
      setSubmitting(false);
    }
  };

  const handleKeyDown = (e: React.KeyboardEvent) => {
    if (e.key === "Enter" && e.ctrlKey) {
      e.preventDefault();
      handleSubmit();
    }
  };

  // Copy the full raw request JSON for pasting into external AI
  const copyRawRequest = () => {
    if (!pending) return;
    navigator.clipboard.writeText(pending.raw_request_json);
  };

  // Insert a template response with tool_calls for convenience
  const insertToolCallTemplate = () => {
    const template = JSON.stringify({
      choices: [{
        message: {
          content: "",
          tool_calls: [{
            id: "call_1",
            type: "function",
            function: {
              name: "tool_name",
              arguments: "{}"
            }
          }]
        },
        finish_reason: "tool_calls"
      }]
    }, null, 2);
    setResponse(template);
  };

  // Insert a simple text response template
  const insertTextTemplate = () => {
    const template = JSON.stringify({
      choices: [{
        message: {
          content: "Your response here...",
          tool_calls: null
        },
        finish_reason: "stop"
      }]
    }, null, 2);
    setResponse(template);
  };

  if (!pending) return null;

  return (
    <div className="mock-prompt-overlay">
      <div className="mock-prompt-window">
        <div className="mock-prompt-header">
          <h2>Mock AI — Full API Request</h2>
          <span className="mock-prompt-meta">
            Model: {pending.model.display_name} | {new Date(pending.timestamp).toLocaleTimeString()}
          </span>
          <button className="mock-copy-btn" onClick={copyRawRequest} title="Copy full API request JSON">
            📋 Copy Request JSON
          </button>
          <button className="mock-copy-btn" onClick={insertTextTemplate} title="Insert text response template">
            📝 Text Template
          </button>
          <button className="mock-copy-btn" onClick={insertToolCallTemplate} title="Insert tool_call response template">
            🔧 Tool Call Template
          </button>
        </div>

        <div className="mock-prompt-messages">
          <pre className="mock-msg-content" style={{ whiteSpace: "pre-wrap", fontSize: "12px", maxHeight: "60vh", overflow: "auto" }}>
            {pending.raw_request_json}
          </pre>
        </div>

        <div className="mock-prompt-response">
          <label>Paste API response JSON (or plain text for simple responses):</label>
          <textarea
            ref={textareaRef}
            value={response}
            onChange={(e) => setResponse(e.target.value)}
            onKeyDown={handleKeyDown}
            placeholder={'Paste full JSON response here...\n\nFormat: {"choices": [{"message": {"content": "...", "tool_calls": [...]}}]}\n\nOr just plain text for simple responses.\n\n(Ctrl+Enter to submit)'}
            disabled={submitting}
          />
          <div className="mock-prompt-actions">
            <button
              onClick={handleSubmit}
              disabled={submitting || !response.trim()}
              className="mock-submit-btn"
            >
              {submitting ? "Submitting..." : "Submit Response (Ctrl+Enter)"}
            </button>
          </div>
        </div>
      </div>
    </div>
  );
}
