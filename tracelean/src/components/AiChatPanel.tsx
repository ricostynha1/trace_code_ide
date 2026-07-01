import { useState, useEffect, useRef } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";

interface ChatMessage {
  role: "system" | "user" | "assistant";
  content: string;
}

interface TokenUsage {
  input_tokens: number;
  output_tokens: number;
  thinking_tokens: number;
  cached_tokens: number;
}

interface AiResponse {
  content: string;
  usage: TokenUsage;
  raw_response: string | null;
  truncated: boolean;
}

interface SessionStats {
  total_requests: number;
  total_input_tokens: number;
  total_output_tokens: number;
  total_thinking_tokens: number;
  total_cached_tokens: number;
  total_cost_usd: number;
}

interface ModelConfig {
  provider: string;
  model_id: string;
  display_name: string;
  max_tokens: number;
  temperature: number;
  input_cost_per_m: number;
  output_cost_per_m: number;
  cached_input_cost_per_m: number;
}

interface AiSettings {
  active_provider: string;
  openrouter_api_key: string | null;
  bedrock_access_key: string | null;
  bedrock_secret_key: string | null;
  bedrock_region: string | null;
  selected_model: ModelConfig | null;
}

interface InteractionEntry {
  id: string;
  timestamp: string;
  agent: string;
  model_id: string;
  model_display_name: string;
  request_messages: ChatMessage[];
  response_content: string | null;
  error: string | null;
  usage: TokenUsage;
  cost: { total_usd: number; input_cost: number; output_cost: number; cached_savings: number };
  duration_ms: number;
  truncated: boolean;
}

interface Props {
  visible: boolean;
  onClose: () => void;
}

interface MockPendingRequest {
  id: string;
  messages: ChatMessage[];
  model: ModelConfig;
  timestamp: string;
}

type Tab = "chat" | "settings" | "log" | "stats";

export function AiChatPanel({ visible, onClose }: Props) {
  const [tab, setTab] = useState<Tab>("chat");
  const [messages, setMessages] = useState<ChatMessage[]>([]);
  const [input, setInput] = useState("");
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [stats, setStats] = useState<SessionStats | null>(null);
  const [settings, setSettings] = useState<AiSettings | null>(null);
  const [models, setModels] = useState<ModelConfig[]>([]);
  const [log, setLog] = useState<InteractionEntry[]>([]);
  const [inspectEntry, setInspectEntry] = useState<InteractionEntry | null>(null);
  const [mockPending, setMockPending] = useState<MockPendingRequest[]>([]);
  const [mockResponse, setMockResponse] = useState("");
  const [streamingContent, setStreamingContent] = useState<string>("");
  const [useStreaming, setUseStreaming] = useState(true);
  const messagesEndRef = useRef<HTMLDivElement>(null);
  const mockPollRef = useRef<ReturnType<typeof setInterval> | null>(null);

  useEffect(() => {
    if (visible) {
      loadSettings();
      loadStats();
    }
  }, [visible]);

  // Poll for mock pending requests when loading and mock provider active
  useEffect(() => {
    if (visible && loading && settings?.active_provider === "mock") {
      mockPollRef.current = setInterval(async () => {
        try {
          const pending = await invoke<MockPendingRequest[]>("get_mock_pending");
          setMockPending(pending);
        } catch {}
      }, 500);
    } else {
      if (mockPollRef.current) {
        clearInterval(mockPollRef.current);
        mockPollRef.current = null;
      }
      setMockPending([]);
    }
    return () => {
      if (mockPollRef.current) {
        clearInterval(mockPollRef.current);
        mockPollRef.current = null;
      }
    };
  }, [visible, loading, settings?.active_provider]);

  useEffect(() => {
    messagesEndRef.current?.scrollIntoView({ behavior: "smooth" });
  }, [messages]);

  const loadSettings = async () => {
    try {
      const s = await invoke<AiSettings>("get_ai_settings");
      setSettings(s);
    } catch {}
  };

  const loadStats = async () => {
    try {
      const s = await invoke<SessionStats>("get_ai_session_stats");
      setStats(s);
    } catch {}
  };

  const loadModels = async () => {
    try {
      const m = await invoke<ModelConfig[]>("get_ai_models");
      setModels(m);
    } catch (e) {
      setError(String(e));
    }
  };

  const loadLog = async () => {
    try {
      const l = await invoke<InteractionEntry[]>("get_ai_interaction_log", { limit: 100 });
      setLog(l);
    } catch {}
  };

  const sendMessage = async () => {
    if (!input.trim() || loading) return;

    const userMsg: ChatMessage = { role: "user", content: input.trim() };
    const newMessages = [...messages, userMsg];
    setMessages(newMessages);
    setInput("");
    setLoading(true);
    setError(null);
    setStreamingContent("");

    try {
      if (useStreaming) {
        // Listen for stream tokens
        const unlisten = await listen<{ stream_id: string; token: string; done: boolean; accumulated: string }>(
          "ai-stream-token",
          (event) => {
            if (!event.payload.done) {
              setStreamingContent(event.payload.accumulated);
            }
          }
        );

        const response = await invoke<AiResponse>("ai_chat_stream", { messages: newMessages });
        unlisten();
        const assistantMsg: ChatMessage = { role: "assistant", content: response.content };
        setMessages([...newMessages, assistantMsg]);
        setStreamingContent("");
      } else {
        const response = await invoke<AiResponse>("ai_chat", { messages: newMessages });
        const assistantMsg: ChatMessage = { role: "assistant", content: response.content };
        setMessages([...newMessages, assistantMsg]);
      }
      loadStats();
    } catch (e) {
      setError(String(e));
      setStreamingContent("");
    } finally {
      setLoading(false);
    }
  };

  const submitMockResponse = async (requestId: string) => {
    if (!mockResponse.trim()) return;
    try {
      await invoke("mock_submit_response", { requestId, response: mockResponse.trim() });
      setMockResponse("");
    } catch (e) {
      setError(String(e));
    }
  };

  const saveSettings = async (newSettings: AiSettings) => {
    try {
      await invoke("update_ai_settings", { newSettings });
      setSettings(newSettings);
    } catch (e) {
      setError(String(e));
    }
  };

  if (!visible) return null;

  return (
    <div className="ai-chat-panel">
      <div className="ai-panel-header">
        <div className="ai-tabs">
          <button className={tab === "chat" ? "active" : ""} onClick={() => setTab("chat")}>Chat</button>
          <button className={tab === "settings" ? "active" : ""} onClick={() => { setTab("settings"); loadModels(); }}>Settings</button>
          <button className={tab === "log" ? "active" : ""} onClick={() => { setTab("log"); loadLog(); }}>Log</button>
          <button className={tab === "stats" ? "active" : ""} onClick={() => { setTab("stats"); loadStats(); }}>Stats</button>
        </div>
        <button className="close-btn" onClick={onClose}>×</button>
      </div>

      {tab === "chat" && (
        <div className="ai-chat-content">
          <div className="ai-messages">
            {messages.map((m, i) => (
              <div key={i} className={`ai-msg ai-msg-${m.role}`}>
                <span className="ai-msg-role">{m.role}</span>
                <pre className="ai-msg-content">{m.content}</pre>
              </div>
            ))}
            {loading && mockPending.length === 0 && !streamingContent && <div className="ai-msg ai-msg-loading">Thinking...</div>}
            {loading && streamingContent && (
              <div className="ai-msg ai-msg-assistant">
                <span className="ai-msg-role">assistant</span>
                <pre className="ai-msg-content">{streamingContent}<span className="ai-cursor">▌</span></pre>
              </div>
            )}
            {loading && mockPending.length > 0 && (
              <div className="ai-msg ai-msg-loading">
                Mock mode — respond in the prompt window overlay.
              </div>
            )}
            {error && <div className="ai-msg ai-msg-error">{error}</div>}
            <div ref={messagesEndRef} />
          </div>
          <div className="ai-input-area">
            <textarea
              value={input}
              onChange={(e) => setInput(e.target.value)}
              onKeyDown={(e) => { if (e.key === "Enter" && !e.shiftKey) { e.preventDefault(); sendMessage(); } }}
              placeholder="Send a message..."
              disabled={loading}
            />
            <button onClick={sendMessage} disabled={loading || !input.trim()}>Send</button>
          </div>
        </div>
      )}

      {tab === "settings" && settings && (
        <div className="ai-settings-content">
          <h3>AI Provider Settings</h3>

          <label>Active Provider</label>
          <select
            value={settings.active_provider}
            onChange={(e) => saveSettings({ ...settings, active_provider: e.target.value })}
          >
            <option value="mock">Mock (Debug)</option>
            <option value="open_router">OpenRouter</option>
            <option value="bedrock">Amazon Bedrock</option>
          </select>

          <label>OpenRouter API Key</label>
          <input
            type="password"
            value={settings.openrouter_api_key || ""}
            onChange={(e) => saveSettings({ ...settings, openrouter_api_key: e.target.value || null })}
          />

          <label>Bedrock Access Key</label>
          <input
            type="password"
            value={settings.bedrock_access_key || ""}
            onChange={(e) => saveSettings({ ...settings, bedrock_access_key: e.target.value || null })}
          />

          <label>Bedrock Secret Key</label>
          <input
            type="password"
            value={settings.bedrock_secret_key || ""}
            onChange={(e) => saveSettings({ ...settings, bedrock_secret_key: e.target.value || null })}
          />

          <label>Bedrock Region</label>
          <input
            value={settings.bedrock_region || ""}
            onChange={(e) => saveSettings({ ...settings, bedrock_region: e.target.value || null })}
          />

          <h3>Model Selection</h3>

          <label>
            <input
              type="checkbox"
              checked={useStreaming}
              onChange={(e) => setUseStreaming(e.target.checked)}
            />
            {" "}Enable streaming responses
          </label>

          {models.length === 0 ? (
            <p className="ai-hint">Click "Settings" tab to fetch available models from providers.</p>
          ) : (
            <select
              value={settings.selected_model?.model_id || ""}
              onChange={(e) => {
                const m = models.find((x) => x.model_id === e.target.value);
                if (m) saveSettings({ ...settings, selected_model: m });
              }}
            >
              <option value="">-- Select model --</option>
              {models.map((m) => (
                <option key={m.model_id} value={m.model_id}>
                  {m.display_name} (${m.input_cost_per_m.toFixed(2)}/${m.output_cost_per_m.toFixed(2)} per 1M)
                </option>
              ))}
            </select>
          )}
        </div>
      )}

      {tab === "log" && (
        <div className="ai-log-content">
          <h3>Interaction Log</h3>
          {inspectEntry ? (
            <div className="ai-log-detail">
              <button onClick={() => setInspectEntry(null)}>← Back</button>
              <h4>{inspectEntry.agent} — {inspectEntry.model_display_name}</h4>
              <p className="ai-meta">{inspectEntry.timestamp} | {inspectEntry.duration_ms}ms | ${inspectEntry.cost.total_usd.toFixed(6)}</p>
              <h5>Request Messages</h5>
              {inspectEntry.request_messages.map((m, i) => (
                <div key={i} className="ai-log-msg">
                  <strong>{m.role}:</strong>
                  <pre>{m.content}</pre>
                </div>
              ))}
              <h5>Response</h5>
              <pre>{inspectEntry.response_content || inspectEntry.error || "(none)"}</pre>
              <h5>Token Usage</h5>
              <table>
                <tbody>
                  <tr><td>Input</td><td>{inspectEntry.usage.input_tokens}</td></tr>
                  <tr><td>Output</td><td>{inspectEntry.usage.output_tokens}</td></tr>
                  <tr><td>Thinking</td><td>{inspectEntry.usage.thinking_tokens}</td></tr>
                  <tr><td>Cached</td><td>{inspectEntry.usage.cached_tokens}</td></tr>
                </tbody>
              </table>
            </div>
          ) : (
            <div className="ai-log-list">
              {log.length === 0 && <p>No interactions yet.</p>}
              {log.map((entry) => (
                <div key={entry.id} className="ai-log-entry" onClick={() => setInspectEntry(entry)}>
                  <span className="ai-log-agent">{entry.agent}</span>
                  <span className="ai-log-model">{entry.model_display_name}</span>
                  <span className="ai-log-cost">${entry.cost.total_usd.toFixed(6)}</span>
                  <span className="ai-log-time">{new Date(entry.timestamp).toLocaleTimeString()}</span>
                </div>
              ))}
            </div>
          )}
        </div>
      )}

      {tab === "stats" && (
        <div className="ai-stats-content">
          <h3>Session Statistics</h3>
          {stats ? (
            <table className="ai-stats-table">
              <tbody>
                <tr><td>Requests</td><td>{stats.total_requests}</td></tr>
                <tr><td>Input tokens</td><td>{stats.total_input_tokens.toLocaleString()}</td></tr>
                <tr><td>Output tokens</td><td>{stats.total_output_tokens.toLocaleString()}</td></tr>
                <tr><td>Thinking tokens</td><td>{stats.total_thinking_tokens.toLocaleString()}</td></tr>
                <tr><td>Cached tokens</td><td>{stats.total_cached_tokens.toLocaleString()}</td></tr>
                <tr><td>Total cost</td><td>${stats.total_cost_usd.toFixed(6)}</td></tr>
              </tbody>
            </table>
          ) : (
            <p>Loading...</p>
          )}
        </div>
      )}
    </div>
  );
}
