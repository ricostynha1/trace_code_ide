import { useState, useEffect, useRef } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";

interface ChatMessage {
  role: "system" | "user" | "assistant";
  content: string;
}

interface MockPendingRequest {
  id: string;
  messages: ChatMessage[];
  model: { display_name: string };
  timestamp: string;
}

/**
 * Full-screen modal overlay for mock AI mode.
 * Shows the complete prompt context and lets the user type/paste a response.
 * Appears automatically when mock provider receives a request.
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

  // Copy all messages as text for pasting into external AI
  const copyPrompt = () => {
    if (!pending) return;
    const text = pending.messages
      .map((m) => `[${m.role.toUpperCase()}]\n${m.content}`)
      .join("\n\n---\n\n");
    navigator.clipboard.writeText(text);
  };

  if (!pending) return null;

  return (
    <div className="mock-prompt-overlay">
      <div className="mock-prompt-window">
        <div className="mock-prompt-header">
          <h2>Mock AI — Awaiting Response</h2>
          <span className="mock-prompt-meta">
            Model: {pending.model.display_name} | {new Date(pending.timestamp).toLocaleTimeString()}
          </span>
          <button className="mock-copy-btn" onClick={copyPrompt} title="Copy prompt to clipboard">
            📋 Copy Prompt
          </button>
        </div>

        <div className="mock-prompt-messages">
          {pending.messages.map((m, i) => (
            <div key={i} className={`mock-msg mock-msg-${m.role}`}>
              <div className="mock-msg-role">{m.role}</div>
              <pre className="mock-msg-content">{m.content}</pre>
            </div>
          ))}
        </div>

        <div className="mock-prompt-response">
          <label>Your response (paste AI output or type manually):</label>
          <textarea
            ref={textareaRef}
            value={response}
            onChange={(e) => setResponse(e.target.value)}
            onKeyDown={handleKeyDown}
            placeholder="Paste the AI response here... (Ctrl+Enter to submit)"
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
