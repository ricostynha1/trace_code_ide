import { useState, useEffect, useRef } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { ChatSwitcher, ChatInstance } from "./ChatSwitcher";

interface ChatMessage {
  role: "system" | "user" | "assistant" | "tool";
  content: string;
  tool_call_id?: string;
  tool_calls?: ToolCallResponse[];
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
  coding_index?: number | null;
  coding_rank?: number | null;
  supports_caching: boolean;
  supports_tools: boolean;
}

interface AiSettings {
  active_provider: string;
  openrouter_api_key: string | null;
  bedrock_api_key: string | null;
  bedrock_region: string | null;
  selected_model: ModelConfig | null;
  spend_cap_usd: number;
}

interface ToolCallResponse {
  id: string;
  type: string;
  function: { name: string; arguments: string };
}

interface ToolSchemaInfo {
  type: string;
  function: {
    name: string;
    description: string;
    parameters: any;
  };
}

interface InteractionEntry {
  id: string;
  timestamp: string;
  agent: string;
  model_id: string;
  model_display_name: string;
  request_messages: ChatMessage[];
  response_content: string | null;
  response_tool_calls: ToolCallResponse[];
  error: string | null;
  usage: TokenUsage;
  cost: { total_usd: number; input_cost: number; output_cost: number; cached_savings: number };
  duration_ms: number;
  truncated: boolean;
  tools_provided?: number;
  tool_names?: string[];
  tool_schemas?: ToolSchemaInfo[];
}

interface Props {
  visible: boolean;
  onClose: () => void;
}

interface MockPendingRequest {
  id: string;
  raw_request_json: string;
  model: ModelConfig;
  timestamp: string;
}

type Tab = "chat" | "settings" | "log" | "stats";

/** Format token count compactly: 1234 -> '1.2k', 1234567 -> '1.2M' */
function compactNum(n: number): string {
  if (n >= 1_000_000) return (n / 1_000_000).toFixed(1) + "M";
  if (n >= 1_000) return (n / 1_000).toFixed(n >= 10_000 ? 0 : 1) + "k";
  return String(n);
}

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
  const [modelFetchLoading, setModelFetchLoading] = useState(false);
  const [modelFetchError, setModelFetchError] = useState<string | null>(null);
  const [envKeys, setEnvKeys] = useState<Record<string, string>>({});
  const [toolLoopPaused, setToolLoopPaused] = useState<string | null>(null);
  const [chatInstances, setChatInstances] = useState<ChatInstance[]>([{ id: crypto.randomUUID(), messages: [], createdAt: new Date(), label: "Chat 1" }]);
  const [activeChatId, setActiveChatId] = useState<string>(chatInstances[0].id);
  const [showSwitcher, setShowSwitcher] = useState(false);
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

  // Listen for intermediate chat messages (tool calls, tool responses) from backend
  useEffect(() => {
    const unlisten = listen<{ role: string; content: string }>("ai-chat-message", (event) => {
      const { role, content } = event.payload;
      // T3.2: Reset streaming content so tool calls appear as separate messages
      setStreamingContent("");
      setMessages((prev) => [...prev, { role: role as "system" | "user" | "assistant", content }]);
    });
    return () => { unlisten.then((fn) => fn()); };
  }, []);

  // Listen for tool loop pause (Bug 3: user-prompted pause instead of hard abort)
  useEffect(() => {
    const unlisten = listen<{ loops_completed: number; message: string }>("tool-loop-pause", (event) => {
      setToolLoopPaused(event.payload.message);
    });
    return () => { unlisten.then((fn) => fn()); };
  }, []);

  // T3.1: Listen for stats updates to refresh cost bar live (not only on user prompt)
  useEffect(() => {
    const unlisten = listen("ai-stats-updated", () => {
      loadStats();
    });
    return () => { unlisten.then((fn) => fn()); };
  }, []);

  const loadSettings = async () => {
    try {
      const s = await invoke<AiSettings>("get_ai_settings");
      setSettings(s);
      const keys = await invoke<Record<string, string>>("detect_env_keys");
      setEnvKeys(keys);
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

    // T3.8: Frontend spend cap check
    const cap = settings?.spend_cap_usd ?? 1;
    if (stats && stats.total_cost_usd >= cap) {
      setError(`Spend cap reached ($${stats.total_cost_usd.toFixed(4)} >= $${cap.toFixed(2)}). Increase in settings.`);
      return;
    }

    const userMsg: ChatMessage = { role: "user", content: input.trim() };
    const newMessages = [...messages, userMsg];
    setMessages(newMessages);
    setInput("");
    setLoading(true);
    setError(null);
    setStreamingContent("");

    // Filter out synthetic system messages (tool calls/responses) before sending to backend
    const apiMessages = newMessages.filter((m) => m.role !== "system");

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

        const response = await invoke<AiResponse>("ai_chat_stream", { messages: apiMessages });
        unlisten();
        const assistantMsg: ChatMessage = { role: "assistant", content: response.content };
        setMessages((prev) => [...prev, assistantMsg]);
        setStreamingContent("");
      } else {
        const response = await invoke<AiResponse>("ai_chat", { messages: apiMessages });
        const assistantMsg: ChatMessage = { role: "assistant", content: response.content };
        setMessages((prev) => [...prev, assistantMsg]);
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

  const resumeToolLoop = async (shouldContinue: boolean) => {
    setToolLoopPaused(null);
    try {
      await invoke("resume_tool_loop", { shouldContinue });
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

  const switchChat = (id: string) => {
    // Save current messages to active instance
    setChatInstances((prev) =>
      prev.map((inst) => inst.id === activeChatId ? { ...inst, messages } : inst)
    );
    // Load target instance
    const target = chatInstances.find((inst) => inst.id === id);
    if (target) {
      setMessages(target.messages as ChatMessage[]);
      setActiveChatId(id);
      setError(null);
      setStreamingContent("");
    }
  };

  const createNewChat = () => {
    // Save current messages first
    setChatInstances((prev) =>
      prev.map((inst) => inst.id === activeChatId ? { ...inst, messages } : inst)
    );
    const newId = crypto.randomUUID();
    const newInst: ChatInstance = { id: newId, messages: [], createdAt: new Date(), label: `Chat ${chatInstances.length + 1}` };
    setChatInstances((prev) => [...prev, newInst]);
    setActiveChatId(newId);
    setMessages([]);
    setError(null);
    setStreamingContent("");
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
          <button className="ai-new-session-btn" onClick={() => setShowSwitcher(true)} title="Switch chat session">+</button>
        </div>
        <button className="close-btn" onClick={onClose}>×</button>
      </div>

      {tab === "chat" && (
        <div className="ai-chat-content">
          <div className="ai-messages">
            {(() => {
              // Group consecutive system messages into batches
              const grouped: Array<{ type: "single"; msg: ChatMessage; idx: number } | { type: "batch"; msgs: ChatMessage[]; startIdx: number }> = [];
              let i = 0;
              while (i < messages.length) {
                if (messages[i].role === "system") {
                  const batch: ChatMessage[] = [];
                  const startIdx = i;
                  while (i < messages.length && messages[i].role === "system") {
                    batch.push(messages[i]);
                    i++;
                  }
                  if (batch.length > 1) {
                    grouped.push({ type: "batch", msgs: batch, startIdx });
                  } else {
                    grouped.push({ type: "single", msg: batch[0], idx: startIdx });
                  }
                } else {
                  grouped.push({ type: "single", msg: messages[i], idx: i });
                  i++;
                }
              }

              // Parse tool name from content like '→ tool_name(args)' or 'call tool_name'
              const parseToolName = (content: string): string => {
                const m1 = content.match(/^→\s+(\w+)/);
                if (m1) return m1[1];
                const m2 = content.match(/^call\s+(\w+)/i);
                if (m2) return m2[1];
                const m3 = content.match(/tool[_\s]call[:\s]+(\w+)/i);
                if (m3) return m3[1];
                return "tool_call";
              };

              return grouped.map((entry, _gi) => {
                if (entry.type === "batch") {
                  // Count tool names
                  const counts: Record<string, number> = {};
                  for (const msg of entry.msgs) {
                    const name = parseToolName(msg.content);
                    counts[name] = (counts[name] || 0) + 1;
                  }
                  const summary = Object.entries(counts)
                    .map(([name, count]) => `${name} ×${count}`)
                    .join(", ");
                  return (
                    <div key={`batch-${entry.startIdx}`} className="ai-msg ai-msg-system ai-msg-tool-batch">
                      <span className="ai-msg-tool-batch-label">batch</span>
                      <span className="ai-msg-tool-call">call {summary}</span>
                    </div>
                  );
                }
                const m = entry.msg;
                return (
                  <div key={entry.idx} className={`ai-msg ai-msg-${m.role}`}>
                    {m.role === "system" ? (
                      <span className="ai-msg-tool-call">{m.content}</span>
                    ) : (
                      <>
                        <span className="ai-msg-role">{m.role}</span>
                        <pre className="ai-msg-content">{m.content}</pre>
                      </>
                    )}
                  </div>
                );
              });
            })()}
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
            {toolLoopPaused && (
              <div className="ai-msg ai-msg-pause">
                <span>{toolLoopPaused}</span>
                <div className="ai-pause-buttons">
                  <button onClick={() => resumeToolLoop(true)}>Continue</button>
                  <button onClick={() => resumeToolLoop(false)}>Stop</button>
                </div>
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
          {stats && (
            <div className="ai-cost-bar">
              <span className="ai-cost-label">Session: ${stats.total_cost_usd.toFixed(4)}</span>
              <span className="ai-cost-tokens">
                I{compactNum(stats.total_input_tokens)} O{compactNum(stats.total_output_tokens)} T{compactNum(stats.total_thinking_tokens)}
              </span>
              {settings?.selected_model && (
                <span className="ai-cost-context">
                  {compactNum(stats.total_input_tokens)}/{compactNum(settings.selected_model.max_tokens)} ctx
                </span>
              )}
            </div>
          )}
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
            placeholder={envKeys.openrouter ? `Detected from ${envKeys.openrouter}` : ""}
          />

          <label>Bedrock Bearer Token</label>
          <input
            type="password"
            value={settings.bedrock_api_key || ""}
            onChange={(e) => saveSettings({ ...settings, bedrock_api_key: e.target.value || null })}
            placeholder={envKeys.bedrock ? `Detected from ${envKeys.bedrock}` : ""}
          />

          <label>Bedrock Region (default: eu-west-1)</label>
          <input
            value={settings.bedrock_region || ""}
            onChange={(e) => saveSettings({ ...settings, bedrock_region: e.target.value || null })}
            placeholder="eu-west-1"
          />

          <h3>Model Selection</h3>

          <button
            className="ai-fetch-models-btn"
            onClick={async () => {
              setModelFetchError(null);
              setModelFetchLoading(true);
              try {
                const m = await invoke<ModelConfig[]>("get_ai_models");
                setModels(m);
              } catch (e) {
                setModelFetchError(String(e));
              } finally {
                setModelFetchLoading(false);
              }
            }}
            disabled={modelFetchLoading}
          >
            {modelFetchLoading ? "Fetching models..." : "Fetch Available Models"}
          </button>

          <label>
            <input
              type="checkbox"
              checked={useStreaming}
              onChange={(e) => setUseStreaming(e.target.checked)}
            />
            {" "}Enable streaming responses
          </label>

          {models.length === 0 ? (
            <p className="ai-hint">Click "Fetch Available Models" to query providers.</p>
          ) : (
            <select
              value={settings.selected_model?.model_id || ""}
              onChange={(e) => {
                const m = models.find((x) => x.model_id === e.target.value);
                if (m) saveSettings({ ...settings, selected_model: m });
              }}
            >
              <option value="">-- Select model --</option>
              {models.map((m) => {
                const rank = m.coding_rank ? `#${m.coding_rank}` : "—";
                const badges = [
                  m.supports_caching ? "⚡cache" : "",
                  m.supports_tools ? "🔧tools" : "",
                ].filter(Boolean).join(" ");
                const price = `$${m.input_cost_per_m.toFixed(2)}/${m.output_cost_per_m.toFixed(2)}`;
                return (
                  <option key={m.model_id} value={m.model_id}>
                    [{rank}] {m.display_name} {badges} ({price}/1M)
                  </option>
                );
              })}
            </select>
          )}

          {modelFetchError && (
            <div className="ai-settings-error">
              <strong>Error:</strong> {modelFetchError}
            </div>
          )}

          <h3>Spend Cap</h3>
          <label>Max session spend (USD)</label>
          <input
            type="number"
            step="0.1"
            min="0"
            value={settings.spend_cap_usd ?? 1}
            onChange={(e) => saveSettings({ ...settings, spend_cap_usd: parseFloat(e.target.value) || 1 })}
          />
          <p className="ai-hint">Chat will refuse new requests when session cost exceeds this cap.</p>
        </div>
      )}

      {tab === "log" && (
        <div className="ai-log-content">
          <h3>Interaction Log</h3>
          {/* T3.3: Total cost on top */}
          {!inspectEntry && log.length > 0 && (
            <div className="ai-log-totals">
              <table className="ai-log-aggregate-table">
                <thead>
                  <tr><th>Metric</th><th>Total</th></tr>
                </thead>
                <tbody>
                  <tr><td>Requests</td><td>{log.length}</td></tr>
                  <tr><td>Input tokens</td><td>{log.reduce((s, e) => s + e.usage.input_tokens, 0).toLocaleString()}</td></tr>
                  <tr><td>Cached tokens</td><td>{log.reduce((s, e) => s + e.usage.cached_tokens, 0).toLocaleString()}</td></tr>
                  <tr><td>Output tokens</td><td>{log.reduce((s, e) => s + e.usage.output_tokens, 0).toLocaleString()}</td></tr>
                  <tr><td>Thinking tokens</td><td>{log.reduce((s, e) => s + e.usage.thinking_tokens, 0).toLocaleString()}</td></tr>
                  <tr><td>Total cost</td><td><strong>${log.reduce((s, e) => s + e.cost.total_usd, 0).toFixed(6)}</strong></td></tr>
                  <tr><td>Cache savings</td><td>${log.reduce((s, e) => s + e.cost.cached_savings, 0).toFixed(6)}</td></tr>
                </tbody>
              </table>
            </div>
          )}
          {inspectEntry ? (
            <div className="ai-log-detail">
              <button onClick={() => setInspectEntry(null)}>← Back</button>
              <h4>{inspectEntry.agent} — {inspectEntry.model_display_name}</h4>
              <p className="ai-meta">{inspectEntry.timestamp} | {inspectEntry.duration_ms}ms | ${inspectEntry.cost.total_usd.toFixed(6)}</p>

              {(inspectEntry.tool_names?.length ?? 0) > 0 && (
                <details className="ai-log-tools-provided">
                  <summary>Tools provided: {inspectEntry.tools_provided ?? 0} — [{inspectEntry.tool_names?.join(", ")}]</summary>
                  <div className="ai-log-tools-list">
                    {(inspectEntry.tool_schemas ?? []).map((schema, i) => (
                      <div key={i} className="ai-log-tool-schema">
                        <strong>{schema.function.name}</strong>
                        <span className="ai-tool-desc"> — {schema.function.description}</span>
                        <pre className="ai-tool-params">{JSON.stringify(schema.function.parameters, null, 2)}</pre>
                      </div>
                    ))}
                    {(!inspectEntry.tool_schemas || inspectEntry.tool_schemas.length === 0) && (
                      <ul>
                        {inspectEntry.tool_names?.map((name, i) => <li key={i}>{name}</li>)}
                      </ul>
                    )}
                  </div>
                </details>
              )}

              <h5>Request Messages</h5>
              {inspectEntry.request_messages.map((m, i) => (
                <div key={i} className="ai-log-msg">
                  {m.role.toUpperCase() === "SYSTEM" ? (
                    <details className="ai-log-system-prompt">
                      <summary><strong>SYSTEM PROMPT</strong> ({m.content.length} chars)</summary>
                      <pre>{m.content}</pre>
                    </details>
                  ) : (
                    <>
                      <strong>{m.role.toUpperCase()}:</strong>
                      {m.content && <pre>{m.content}</pre>}
                    </>
                  )}
                  {(m as any).tool_calls?.length > 0 && (
                    <div className="ai-log-tool-calls">
                      {(m as any).tool_calls.map((tc: any, j: number) => (
                        <pre key={j} className="ai-log-tool-call">→ {tc.function?.name}({tc.function?.arguments})</pre>
                      ))}
                    </div>
                  )}
                  {(m as any).tool_call_id && (
                    <span className="ai-log-tool-id">[tool_call_id: {(m as any).tool_call_id}]</span>
                  )}
                </div>
              ))}

              <h5>Response</h5>
              {inspectEntry.response_tool_calls?.length > 0 ? (
                <div className="ai-log-response-tools">
                  {inspectEntry.response_content && <pre>{inspectEntry.response_content}</pre>}
                  <div className="ai-log-tool-calls">
                    <strong>Tool calls:</strong>
                    {inspectEntry.response_tool_calls.map((tc, i) => (
                      <pre key={i} className="ai-log-tool-call">→ {tc.function.name}({tc.function.arguments})</pre>
                    ))}
                  </div>
                </div>
              ) : (
                <pre>{inspectEntry.response_content || inspectEntry.error || "(no response)"}</pre>
              )}

              {/* T3.7: Detailed cost breakdown per log */}
              <h5>Token Usage & Cost</h5>
              <table>
                <tbody>
                  <tr><td>Input (non-cached)</td><td>{inspectEntry.usage.input_tokens - inspectEntry.usage.cached_tokens}</td><td>${((inspectEntry.usage.input_tokens - inspectEntry.usage.cached_tokens) / 1_000_000 * (settings?.selected_model?.input_cost_per_m ?? 0)).toFixed(6)}</td></tr>
                  <tr><td>Input (cached)</td><td>{inspectEntry.usage.cached_tokens}</td><td>${(inspectEntry.usage.cached_tokens / 1_000_000 * (settings?.selected_model?.cached_input_cost_per_m ?? 0)).toFixed(6)}</td></tr>
                  <tr><td>Output</td><td>{inspectEntry.usage.output_tokens}</td><td>${(inspectEntry.usage.output_tokens / 1_000_000 * (settings?.selected_model?.output_cost_per_m ?? 0)).toFixed(6)}</td></tr>
                  <tr><td>Thinking</td><td>{inspectEntry.usage.thinking_tokens}</td><td>${(inspectEntry.usage.thinking_tokens / 1_000_000 * (settings?.selected_model?.output_cost_per_m ?? 0)).toFixed(6)}</td></tr>
                  <tr><td><strong>Total</strong></td><td></td><td><strong>${inspectEntry.cost.total_usd.toFixed(6)}</strong></td></tr>
                  <tr><td>Cache savings</td><td></td><td>${inspectEntry.cost.cached_savings.toFixed(6)}</td></tr>
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

      {showSwitcher && (
        <ChatSwitcher
          chatInstances={chatInstances.map((inst) => inst.id === activeChatId ? { ...inst, messages } : inst)}
          activeChatId={activeChatId}
          onSwitch={switchChat}
          onNewChat={createNewChat}
          onReorder={setChatInstances}
          onClose={() => setShowSwitcher(false)}
        />
      )}
    </div>
  );
}
