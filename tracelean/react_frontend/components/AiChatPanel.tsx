import { useState, useEffect, useRef } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { ChatSwitcher, ChatInstance } from "./ChatSwitcher";

interface ChatMessage {
  role: "system" | "user" | "assistant" | "tool";
  content: string;
  tool_call_id?: string;
  tool_calls?: ToolCallResponse[];
  /** Present when this transcript entry is a tool-call chip, not a chat message. */
  chip?: ToolChipData;
}

/** Backend ToolCallStatus: externally tagged, snake_case. */
type ToolCallStatusPayload = "running" | "completed" | { failed: { error: string } };

interface ToolCallEventPayload {
  tool_name: string;
  status: ToolCallStatusPayload;
  duration_ms: number | null;
  depth: number;
  reason?: string | null;
  call_id?: string | null;
  args_preview?: string | null;
  result_preview?: string | null;
}

interface ToolChipData {
  callId: string | null;
  toolName: string;
  status: "running" | "completed" | "failed";
  durationMs?: number;
  argsPreview?: string;
  resultPreview?: string;
  error?: string;
}

/** Tauri event payloads may arrive as objects or as JSON strings (older
 * emitters double-encoded). Accept both. */
function normalizePayload<T>(payload: unknown): T | null {
  if (typeof payload === "string") {
    try {
      return JSON.parse(payload) as T;
    } catch {
      return null;
    }
  }
  return payload as T;
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

/** P7: context-utilization snapshot from get_chat_session_info. */
interface ChatSessionInfo {
  estimated_context_tokens: number;
  context_window: number;
  context_window_known: boolean;
  turns: number;
  compaction_count: number;
  model_view_len: number;
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
  summary_model: ModelConfig | null;
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
  was_compacted?: boolean;
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
  const [expandedChips, setExpandedChips] = useState<Record<string, boolean>>({});
  const [sessionInfo, setSessionInfo] = useState<ChatSessionInfo | null>(null);
  const [confirmReset, setConfirmReset] = useState(false);
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

  // Listen for intermediate chat messages (assistant text between tool loops,
  // system error notices) from backend
  useEffect(() => {
    const unlisten = listen("ai-chat-message", (event) => {
      const payload = normalizePayload<{ role: string; content: string }>(event.payload);
      if (!payload || typeof payload.content !== "string") return;
      // T3.2: Reset streaming content so intermediate messages appear separately
      setStreamingContent("");
      setMessages((prev) => [...prev, { role: payload.role as "system" | "user" | "assistant", content: payload.content }]);
    });
    return () => { unlisten.then((fn) => fn()); };
  }, []);

  // Listen for tool-call lifecycle events: Running appends a chip, Completed/
  // Failed updates the same chip in place (matched by call_id).
  useEffect(() => {
    const unlisten = listen("tool-call", (event) => {
      const payload = normalizePayload<ToolCallEventPayload>(event.payload);
      if (!payload || typeof payload.tool_name !== "string") return;

      const status: ToolChipData["status"] =
        payload.status === "running" ? "running"
        : payload.status === "completed" ? "completed"
        : "failed";
      const chip: ToolChipData = {
        callId: payload.call_id ?? null,
        toolName: payload.tool_name,
        status,
        durationMs: payload.duration_ms ?? undefined,
        argsPreview: payload.args_preview ?? undefined,
        resultPreview: payload.result_preview ?? undefined,
        error: typeof payload.status === "object" ? payload.status.failed.error : undefined,
      };

      setStreamingContent("");
      setMessages((prev) => {
        // Update the matching running chip in place if we have a call_id
        if (chip.callId) {
          for (let i = prev.length - 1; i >= 0; i--) {
            const existing = prev[i].chip;
            if (existing && existing.callId === chip.callId) {
              if (existing.status !== "running") return prev; // already final
              const next = [...prev];
              next[i] = { ...prev[i], chip: { ...existing, ...chip } };
              return next;
            }
          }
        }
        return [...prev, { role: "tool", content: "", chip }];
      });
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
      loadSessionInfo();
    });
    return () => { unlisten.then((fn) => fn()); };
  }, [activeChatId]);

  // P7: refresh context info when panel opens or the active chat changes
  useEffect(() => {
    if (visible) loadSessionInfo();
  }, [visible, activeChatId]);

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

  const loadSessionInfo = async () => {
    try {
      const info = await invoke<ChatSessionInfo>("get_chat_session_info", { sessionId: activeChatId });
      setSessionInfo(info);
    } catch {}
  };

  // P7 (D7.4): reset model context, keep the visible transcript with a divider
  const resetContext = async () => {
    setConfirmReset(false);
    try {
      await invoke("reset_chat_session", { sessionId: activeChatId });
      setMessages((prev) => [...prev, { role: "system", content: "— context reset —" }]);
      loadSessionInfo();
    } catch (e) {
      setError(String(e));
    }
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

        // Streaming still uses full messages (TODO: migrate ai_chat_stream to session-based)
        const apiMessages = newMessages.filter((m) => m.role !== "system" && !m.chip);
        const response = await invoke<AiResponse>("ai_chat_stream", { messages: apiMessages });
        unlisten();
        const assistantMsg: ChatMessage = { role: "assistant", content: response.content };
        setMessages((prev) => [...prev, assistantMsg]);
        setStreamingContent("");
      } else {
        // Session-based: backend owns conversation state, preserves compaction across calls
        const response = await invoke<AiResponse>("ai_chat_session", {
          sessionId: activeChatId,
          userMessage: input.trim(),
        });
        const assistantMsg: ChatMessage = { role: "assistant", content: response.content };
        setMessages((prev) => [...prev, assistantMsg]);
      }
      loadStats();
      loadSessionInfo();
    } catch (e) {
      setError(String(e));
      setStreamingContent("");
    } finally {
      setLoading(false);
    }
  };

  // @ts-expect-error kept for future use
  const _submitMockResponse = async (requestId: string) => {
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
              // Group consecutive tool-call chips into batches
              const grouped: Array<{ type: "single"; msg: ChatMessage; idx: number } | { type: "chips"; msgs: ChatMessage[]; startIdx: number }> = [];
              let i = 0;
              while (i < messages.length) {
                if (messages[i].chip) {
                  const batch: ChatMessage[] = [];
                  const startIdx = i;
                  while (i < messages.length && messages[i].chip) {
                    batch.push(messages[i]);
                    i++;
                  }
                  grouped.push({ type: "chips", msgs: batch, startIdx });
                } else {
                  grouped.push({ type: "single", msg: messages[i], idx: i });
                  i++;
                }
              }

              const statusIcon = (chip: ToolChipData) =>
                chip.status === "running" ? <span className="ai-chip-spinner">◌</span>
                : chip.status === "completed" ? "✓"
                : "✗";

              const chipLabel = (chip: ToolChipData) => {
                const dur = chip.durationMs != null && chip.durationMs > 0
                  ? ` (${(chip.durationMs / 1000).toFixed(1)}s)` : "";
                return `${chip.toolName}${dur}`;
              };

              const renderChip = (msg: ChatMessage, idx: number) => {
                const chip = msg.chip!;
                const key = chip.callId ?? `chip-${idx}`;
                const expanded = !!expandedChips[key];
                return (
                  <div key={key} className={`ai-tool-chip ai-tool-chip-${chip.status}`}>
                    <button
                      className="ai-tool-chip-head"
                      onClick={() => setExpandedChips((prev) => ({ ...prev, [key]: !prev[key] }))}
                      title={chip.status === "failed" ? chip.error : undefined}
                    >
                      {statusIcon(chip)} {chipLabel(chip)}
                    </button>
                    {expanded && (
                      <div className="ai-tool-chip-detail">
                        {chip.argsPreview && (
                          <div><span className="ai-chip-detail-label">args</span><pre>{chip.argsPreview}</pre></div>
                        )}
                        {(chip.resultPreview || chip.error) && (
                          <div><span className="ai-chip-detail-label">{chip.status === "failed" ? "error" : "result"}</span><pre>{chip.status === "failed" ? (chip.error ?? chip.resultPreview) : chip.resultPreview}</pre></div>
                        )}
                      </div>
                    )}
                  </div>
                );
              };

              return grouped.map((entry) => {
                if (entry.type === "chips") {
                  // Summary label: read_file ×3, replace_str ×1 (1.2s total)
                  const counts: Record<string, number> = {};
                  let totalMs = 0;
                  let anyRunning = false;
                  let anyFailed = false;
                  for (const msg of entry.msgs) {
                    const chip = msg.chip!;
                    counts[chip.toolName] = (counts[chip.toolName] || 0) + 1;
                    totalMs += chip.durationMs ?? 0;
                    if (chip.status === "running") anyRunning = true;
                    if (chip.status === "failed") anyFailed = true;
                  }
                  const summary = Object.entries(counts)
                    .map(([name, count]) => count > 1 ? `${name} ×${count}` : name)
                    .join(", ");
                  const dur = totalMs > 0 ? ` (${(totalMs / 1000).toFixed(1)}s)` : "";
                  const batchKey = `batch-${entry.startIdx}`;
                  const batchExpanded = !!expandedChips[batchKey];
                  if (entry.msgs.length === 1) {
                    return renderChip(entry.msgs[0], entry.startIdx);
                  }
                  return (
                    <div key={batchKey} className={`ai-tool-chip-batch${anyFailed ? " ai-tool-chip-batch-failed" : ""}`}>
                      <button
                        className="ai-tool-chip-head"
                        onClick={() => setExpandedChips((prev) => ({ ...prev, [batchKey]: !prev[batchKey] }))}
                      >
                        {anyRunning ? <span className="ai-chip-spinner">◌</span> : anyFailed ? "✗" : "✓"} {summary}{dur}
                      </button>
                      {batchExpanded && (
                        <div className="ai-tool-chip-batch-items">
                          {entry.msgs.map((msg, j) => renderChip(msg, entry.startIdx + j))}
                        </div>
                      )}
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
              {sessionInfo && (() => {
                // P7 (D7.3): real context utilization vs the model's window
                const used = sessionInfo.estimated_context_tokens;
                const win = Math.max(sessionInfo.context_window, 1);
                const pct = Math.min(100, Math.round((used / win) * 100));
                const level = pct >= 85 ? "red" : pct >= 60 ? "amber" : "green";
                const winLabel = `${sessionInfo.context_window_known ? "" : "~"}${compactNum(win)}`;
                const tooltip =
                  `Context: ${used.toLocaleString()} of ${sessionInfo.context_window_known ? "" : "~"}${win.toLocaleString()} tokens\n` +
                  `Messages in model view: ${sessionInfo.model_view_len} | turns: ${sessionInfo.turns} | compactions: ${sessionInfo.compaction_count}`;
                return (
                  <span className="ai-ctx-cell" title={tooltip}>
                    <span className={`ai-ctx-meter ai-ctx-${level}`}>
                      <span className="ai-ctx-fill" style={{ width: `${pct}%` }} />
                    </span>
                    <span className="ai-ctx-text">
                      {pct}% · {compactNum(used)}/{winLabel}
                    </span>
                    {level === "red" && (
                      <span className="ai-ctx-warning">context nearly full — reset or continue with compaction</span>
                    )}
                    {confirmReset ? (
                      <span className="ai-ctx-reset-confirm">
                        reset context?
                        <button onClick={resetContext}>yes</button>
                        <button onClick={() => setConfirmReset(false)}>no</button>
                      </span>
                    ) : (
                      <button
                        className="ai-ctx-reset-btn"
                        title="Reset model context (transcript is kept)"
                        onClick={() => setConfirmReset(true)}
                      >
                        ⟲
                      </button>
                    )}
                  </span>
                );
              })()}
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

          {models.length > 0 && (
            <>
              <h4>Summary / Compression Model</h4>
              <p className="ai-hint">Cheaper model used for log summarisation and context compression.</p>
              <select
                value={settings.summary_model?.model_id || ""}
                onChange={(e) => {
                  const m = models.find((x) => x.model_id === e.target.value) || null;
                  saveSettings({ ...settings, summary_model: m });
                }}
              >
                <option value="">-- None (use main model) --</option>
                {models.map((m) => {
                  const price = `$${m.input_cost_per_m.toFixed(2)}/${m.output_cost_per_m.toFixed(2)}`;
                  return (
                    <option key={m.model_id} value={m.model_id}>
                      {m.display_name} ({price}/1M)
                    </option>
                  );
                })}
              </select>
            </>
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
              {inspectEntry.was_compacted && (
                <span className="ai-log-compacted-badge">⚡ compacted</span>
              )}

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
