import { useState, useEffect, useRef } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { ChatSwitcher, ChatInstance } from "./ChatSwitcher";

interface ChatMessage {
  role: "system" | "user" | "assistant" | "tool" | "thinking";
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
  total_output_cost_usd: number;
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
  /** P10: agent edits stage diffs for per-hunk approval instead of applying. */
  review_edits?: boolean;
  /** bugs.md Feature 4: agent shell commands require approval before running. */
  review_commands?: boolean;
  /** bugs.md Feature 5: log detail hides messages equal to the previous request. */
  log_show_only_diffs?: boolean;
  /** bugs.md Bug 1: expected remaining rounds (N) in the trim/summarize
   * break-even math. Higher = keep more context. */
  n_expected_rounds?: number;
  /** sandboxing_better.md: shell sandbox mode ("off" | "detect" | "strict"). */
  shell_sandbox?: string;
  /** Sandbox network policy ("deny" | "ask" | "allow"). */
  shell_network?: string;
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
  response_thinking?: string | null;
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
  /** Compaction that ran before this request (bugs.md: log icons + detail). */
  compaction?: {
    kind: string; // "summarized" | "trimmed"
    messages_removed: number;
    tokens_before: number;
    tokens_after: number;
    /** Feature 1.1: per-message decisions (trimmed / trim_candidate / summarized / kept_user). */
    details?: {
      role: string;
      preview: string;
      tokens: number;
      action: string;
    }[];
  };
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

type Tab = "chat" | "settings" | "log" | "stats" | "model";

/** T14: characteristics of the picked model, from get_model_characteristics. */
interface ModelCharacteristics {
  model: ModelConfig & {
    context_window?: number;
    context_window_known?: boolean;
    tool_call_format?: string;
    tool_passing?: string;
  };
  cache_config: {
    cache_mode: string;
    cache_read_discount: number;
    cache_write_multiplier: number;
    ttl_seconds: number | null;
    requires_markers: boolean;
    notes: string | null;
  };
  cache_source: string;
  summary_model: ModelConfig | null;
}

/** Format token count compactly: 1234 -> '1.2k', 1234567 -> '1.2M' */
function compactNum(n: number): string {
  if (n >= 1_000_000) return (n / 1_000_000).toFixed(1) + "M";
  if (n >= 1_000) return (n / 1_000).toFixed(n >= 10_000 ? 0 : 1) + "k";
  return String(n);
}

/** Chars taken up by the tool schemas sent with a request (part of the paid input). */
function toolSchemaChars(e: InteractionEntry): number {
  return e.tool_schemas && e.tool_schemas.length > 0
    ? JSON.stringify(e.tool_schemas).length
    : 0;
}

/** bugs.md: chars/token ratio for one interaction, from provider-reported
 * input tokens vs total request chars. Tool schemas count — they are part of
 * the billed input, so omitting them inflates every token estimate.
 * Null if usage is unknown. */
function charsPerToken(e: InteractionEntry): number | null {
  const msgChars = e.request_messages.reduce(
    (s, m) =>
      s +
      m.content.length +
      ((m as any).tool_calls ?? []).reduce(
        (a: number, tc: any) => a + (tc.function?.arguments?.length ?? 0),
        0
      ),
    0
  );
  const totalChars = msgChars + toolSchemaChars(e);
  return e.usage.input_tokens > 0 && totalChars > 0 ? totalChars / e.usage.input_tokens : null;
}

/** Render a size in both units: "~1.1k tokens · 4500 chars". */
function sizeBoth(chars: number, ratio: number | null): string {
  if (!ratio) return `${chars} chars`;
  return `~${compactNum(Math.round(chars / ratio))} tokens · ${chars} chars`;
}

/** Per-message cached flags: walk messages accumulating estimated tokens
 * until the provider-reported cached prefix is exhausted. Tool schemas sit
 * before the messages in the request, so they consume the cached prefix
 * first — seed the accumulator with them or every message looks cached. */
function cachedFlags(e: InteractionEntry): boolean[] {
  const ratio = charsPerToken(e) ?? 4;
  let cum = toolSchemaChars(e) / ratio;
  return e.request_messages.map((m) => {
    const t =
      (m.content.length +
        ((m as any).tool_calls ?? []).reduce(
          (a: number, tc: any) => a + (tc.function?.arguments?.length ?? 0),
          0
        )) /
      ratio;
    const isCached = e.usage.cached_tokens > 0 && cum + t <= e.usage.cached_tokens;
    cum += t;
    return isCached;
  });
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
  const [toolLoopPaused, setToolLoopPaused] = useState<{
    message: string;
    kind?: string;
    command?: string;
    annotations?: string[];
    networkPolicy?: string;
  } | null>(null);
  // Sandbox network checkbox in a shell-approval prompt (sandboxing_better.md T4).
  const [approveWithNetwork, setApproveWithNetwork] = useState(false);
  const [chatInstances, setChatInstances] = useState<ChatInstance[]>([{ id: crypto.randomUUID(), messages: [], createdAt: new Date(), label: "Chat 1" }]);
  const [activeChatId, setActiveChatId] = useState<string>(chatInstances[0].id);
  const [showSwitcher, setShowSwitcher] = useState(false);
  const [expandedChips, setExpandedChips] = useState<Record<string, boolean>>({});
  const [sessionInfo, setSessionInfo] = useState<ChatSessionInfo | null>(null);
  const [confirmReset, setConfirmReset] = useState(false);
  const [summarizing, setSummarizing] = useState(false);
  // T14: characteristics of the picked model (Model tab)
  const [modelInfo, setModelInfo] = useState<ModelCharacteristics | null>(null);
  const [modelInfoError, setModelInfoError] = useState<string | null>(null);
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
      setMessages((prev) => [...prev, { role: payload.role as ChatMessage["role"], content: payload.content }]);
    });
    return () => { unlisten.then((fn) => fn()); };
  }, []);

  // P9b (D9b.3): surface cache-marker anomalies (paid writes, no cached reads)
  useEffect(() => {
    const unlisten = listen("cache-anomaly", (event) => {
      const payload = normalizePayload<{ message: string }>(event.payload);
      if (!payload || typeof payload.message !== "string") return;
      setMessages((prev) => [...prev, { role: "system", content: `⚠ ${payload.message}` }]);
    });
    return () => { unlisten.then((fn) => fn()); };
  }, []);

  // bugs.md: a staged edit is easy to miss inside a tool-result preview.
  // Surface it in the chat like an approval prompt, and open the (first)
  // edited file so the pending hunks are in front of the user.
  useEffect(() => {
    const unlisten = listen("pending-diffs-changed", (event) => {
      const payload = normalizePayload<{
        count: number;
        new_files?: { file: string; hunks: number; first_line?: number | null }[];
      }>(event.payload);
      if (!payload || !payload.new_files || payload.new_files.length === 0) return;
      const lines = payload.new_files.map(
        (f) => `📝 ${f.file} — ${f.hunks} hunk${f.hunks === 1 ? "" : "s"} staged for review`
      );
      setMessages((prev) => [
        ...prev,
        {
          role: "system",
          content: `${lines.join("\n")}\nAccept or reject the hunks in the editor's diff bar (nothing is applied until you do).`,
        },
      ]);
      const first = payload.new_files[0];
      window.dispatchEvent(
        new CustomEvent("tracelean-navigate", {
          detail: { path: first.file, line: first.first_line ?? null },
        })
      );
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
        // Update the matching running chip in place if we have a call_id.
        // bugs.md Bug 1.5: text-parsed tool calls reuse ids (call_0, call_1 …)
        // across iterations — if the newest chip with this id is already
        // final, this event belongs to a NEW call, so append a fresh chip
        // instead of dropping the event.
        if (chip.callId) {
          for (let i = prev.length - 1; i >= 0; i--) {
            const existing = prev[i].chip;
            if (existing && existing.callId === chip.callId) {
              if (existing.status !== "running") break; // id reused → new chip below
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

  // Listen for tool loop pause (Bug 3: user-prompted pause instead of hard
  // abort) and shell-approval prompts (sandboxing_better.md T4/T5).
  useEffect(() => {
    const unlisten = listen<{
      loops_completed: number;
      message: string;
      kind?: string;
      command?: string;
      annotations?: string[];
      network_policy?: string;
    }>("tool-loop-pause", (event) => {
      const p = event.payload;
      setApproveWithNetwork(false);
      setToolLoopPaused({
        message: p.message,
        kind: p.kind,
        command: p.command,
        annotations: p.annotations,
        networkPolicy: p.network_policy,
      });
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

  // P12: terminal "Fix with AI" pre-seeds the chat input with failing output.
  useEffect(() => {
    const handler = (e: Event) => {
      const detail = (e as CustomEvent).detail;
      if (detail?.prompt) {
        setTab("chat");
        setInput(detail.prompt);
      }
    };
    window.addEventListener("fix-with-ai", handler);
    return () => window.removeEventListener("fix-with-ai", handler);
  }, []);

  // P11: restore persisted sessions into the switcher on startup.
  useEffect(() => {
    (async () => {
      try {
        const summaries = await invoke<
          { id: string; label: string; message_count: number; total_cost_usd: number; updated_at: string }[]
        >("list_chat_sessions");
        if (summaries.length === 0) return;
        const restored: ChatInstance[] = summaries.map((s) => ({
          id: s.id,
          messages: [],
          createdAt: s.updated_at ? new Date(s.updated_at) : new Date(),
          label: s.label,
          cost: s.total_cost_usd,
        }));
        setChatInstances(restored);
        const first = restored[0];
        setActiveChatId(first.id);
        const msgs = await invoke<ChatMessage[]>("get_chat_session_messages", { sessionId: first.id });
        setMessages(msgs.filter((m) => m.role !== "system"));
      } catch (e) {
        console.error("session restore failed:", e);
      }
    })();
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

  // bugs.md Feature 2: user-forced summarization of the model context.
  const summarizeContext = async () => {
    if (summarizing) return;
    setSummarizing(true);
    try {
      await invoke("summarize_chat_session", { sessionId: activeChatId });
      setMessages((prev) => [...prev, { role: "system", content: "— context summarized —" }]);
      loadSessionInfo();
    } catch (e) {
      setError(String(e));
    } finally {
      setSummarizing(false);
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

        // Session-based (bugs.md Bug 1): backend owns conversation state, so
        // the context bar and compaction survive across streamed turns too.
        const response = await invoke<AiResponse>("ai_chat_stream", {
          sessionId: activeChatId,
          userMessage: input.trim(),
        });
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

  const resumeToolLoop = async (shouldContinue: boolean, allowNetwork = false) => {
    setToolLoopPaused(null);
    setApproveWithNetwork(false);
    try {
      await invoke("resume_tool_loop", { shouldContinue, allowNetwork });
    } catch (e) {
      setError(String(e));
    }
  };

  // T12: hard stop — cancels the run's token (aborts the in-flight LLM
  // request, kills a running shell command) and answers any pending
  // pause/approval prompt with "stop". The chat invoke then returns an error
  // ("Stopped by user…"), which the normal send path surfaces and clears
  // `loading` with.
  const stopAgentRun = async () => {
    setToolLoopPaused(null);
    try {
      await invoke("stop_agent_run");
    } catch (e) {
      setError(String(e));
    }
  };

  // T14: load the picked model's characteristics for the Model tab.
  const loadModelInfo = async () => {
    setModelInfoError(null);
    try {
      const info = await invoke<ModelCharacteristics>("get_model_characteristics");
      setModelInfo(info);
    } catch (e) {
      setModelInfo(null);
      setModelInfoError(String(e));
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

  const switchChat = async (id: string) => {
    // Save current messages to active instance
    setChatInstances((prev) =>
      prev.map((inst) => inst.id === activeChatId ? { ...inst, messages } : inst)
    );
    // Load target instance
    const target = chatInstances.find((inst) => inst.id === id);
    if (target) {
      let msgs = target.messages as ChatMessage[];
      // P11: restored sessions carry no local transcript — fetch from backend
      if (msgs.length === 0) {
        try {
          const backendMsgs = await invoke<ChatMessage[]>("get_chat_session_messages", { sessionId: id });
          msgs = backendMsgs.filter((m) => m.role !== "system");
        } catch { /* keep empty */ }
      }
      setMessages(msgs);
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
          <button className={tab === "model" ? "active" : ""} onClick={() => { setTab("model"); loadModelInfo(); }}>Model</button>
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
                // callIds are deliberately reused across tool-loop iterations, so the
                // message index must be part of the key or expanding one chip expands
                // every chip sharing that callId (and duplicate React keys collide).
                const key = `${idx}-${chip.callId ?? "none"}`;
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
                if (m.role === "thinking") {
                  // bugs.md Bug 1.8: model reasoning, collapsed by default.
                  return (
                    <details key={entry.idx} className="ai-msg ai-msg-thinking">
                      <summary>💭 thinking ({m.content.length} chars)</summary>
                      <pre className="ai-msg-content">{m.content}</pre>
                    </details>
                  );
                }
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
            {toolLoopPaused && (() => {
              const isShell = toolLoopPaused.kind === "shell-approval";
              const netAsk = toolLoopPaused.networkPolicy === "ask";
              return (
                <div className="ai-msg ai-msg-pause">
                  {isShell && toolLoopPaused.command ? (
                    <>
                      <span>Agent wants to run a shell command:</span>
                      <pre className="ai-shell-approval-cmd">$ {toolLoopPaused.command}</pre>
                    </>
                  ) : (
                    <span>{toolLoopPaused.message}</span>
                  )}
                  {isShell && (toolLoopPaused.annotations?.length ?? 0) > 0 && (
                    <ul className="ai-shell-approval-warnings">
                      {toolLoopPaused.annotations!.map((a, i) => (
                        <li key={i}>⚠ {a}</li>
                      ))}
                    </ul>
                  )}
                  {isShell && netAsk && (
                    <label className="ai-shell-approval-net">
                      <input
                        type="checkbox"
                        checked={approveWithNetwork}
                        onChange={(e) => setApproveWithNetwork(e.target.checked)}
                      />
                      Allow network access for this command
                    </label>
                  )}
                  <div className="ai-pause-buttons">
                    <button onClick={() => resumeToolLoop(true, isShell && netAsk ? approveWithNetwork : false)}>
                      {isShell ? "Allow" : "Continue"}
                    </button>
                    <button onClick={() => resumeToolLoop(false)}>{isShell ? "Deny" : "Stop"}</button>
                  </div>
                </div>
              );
            })()}
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
              {(() => {
                // bugs.md Feature 2: a segmented bar where each cost category
                // occupies its share of the width — cached input, input, output.
                const rates = settings?.selected_model;
                const cachedCost =
                  (stats.total_cached_tokens / 1_000_000) * (rates?.cached_input_cost_per_m ?? 0);
                const outputCost = stats.total_output_cost_usd ?? 0;
                const inputCost = Math.max(0, stats.total_cost_usd - cachedCost - outputCost);
                const total = cachedCost + inputCost + outputCost;
                if (total <= 0) return null;
                const segments = [
                  { key: "cached", label: "Cached input", cost: cachedCost, cls: "ai-costseg-cached" },
                  { key: "input", label: "Input (not cached)", cost: inputCost, cls: "ai-costseg-input" },
                  { key: "output", label: "Output", cost: outputCost, cls: "ai-costseg-output" },
                ];
                return (
                  <span className="ai-cost-split-bar">
                    {segments.map((s) => {
                      const pct = (s.cost / total) * 100;
                      if (pct <= 0) return null;
                      return (
                        <span
                          key={s.key}
                          className={`ai-costseg ${s.cls}`}
                          style={{ width: `${pct}%` }}
                          title={`${s.label}: $${s.cost.toFixed(4)} (${pct.toFixed(0)}% of cost)`}
                        />
                      );
                    })}
                  </span>
                );
              })()}
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
                    <button
                      className="ai-ctx-reset-btn"
                      title="Summarize model context now (uses the summary model)"
                      disabled={summarizing}
                      onClick={summarizeContext}
                    >
                      {summarizing ? "◌" : "Σ"}
                    </button>
                    <button
                      className="ai-ctx-reset-btn ai-stop-btn"
                      title="Hard stop the running agent (aborts the in-flight request and kills running shell commands)"
                      disabled={!loading}
                      onClick={stopAgentRun}
                    >
                      ⏹
                    </button>
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

          <h3>Context Trimming</h3>
          <label>Expected remaining rounds (N)</label>
          <input
            type="number"
            step="1"
            min="1"
            value={settings.n_expected_rounds ?? 8}
            onChange={(e) =>
              saveSettings({ ...settings, n_expected_rounds: Math.max(1, parseInt(e.target.value) || 8) })
            }
          />
          <p className="ai-hint">
            The N in the trim/summarize break-even math (N · savings ≥ penalty). Higher keeps
            more context and trims/summarizes later; lower is more aggressive. Default 8.
          </p>

          <h3>Edit Review</h3>
          <label className="ai-checkbox-row">
            <input
              type="checkbox"
              checked={settings.review_edits ?? false}
              onChange={(e) => saveSettings({ ...settings, review_edits: e.target.checked })}
            />
            Review agent edits before applying (diff-first)
          </label>
          <p className="ai-hint">
            When on, agent edits stage as pending diffs — accept or reject hunks in the review
            panel before they touch your files.
          </p>

          <h3>Command Review</h3>
          <label className="ai-checkbox-row">
            <input
              type="checkbox"
              checked={settings.review_commands ?? false}
              onChange={(e) => saveSettings({ ...settings, review_commands: e.target.checked })}
            />
            Ask before the agent runs shell commands
          </label>
          <p className="ai-hint">
            When on, every agent run_shell command waits for your approval before executing.
          </p>

          <h3>Shell Sandbox</h3>
          <label className="ai-select-row">
            Containment
            <select
              value={settings.shell_sandbox ?? "detect"}
              onChange={(e) => saveSettings({ ...settings, shell_sandbox: e.target.value })}
            >
              <option value="off">Off — run directly (no containment, no diff)</option>
              <option value="detect">Detect — overlay if available, else strace fallback</option>
              <option value="strict">Strict — require the overlay sandbox or refuse</option>
            </select>
          </label>
          <p className="ai-hint">
            Runs agent shell commands in a bubblewrap overlay: the project is read-only, so
            shell writes are captured as ordinary undoable edits instead of silently mutating
            files. <code>.git/</code> and <code>.tracelean/</code> stay protected; build dirs
            (<code>target/</code>, <code>node_modules/</code>, …) stay writable. Needs Linux with
            bwrap <code>--overlay-src</code> and kernel ≥ 5.11.
          </p>
          <label className="ai-select-row">
            Network
            <select
              value={settings.shell_network ?? "ask"}
              onChange={(e) => saveSettings({ ...settings, shell_network: e.target.value })}
            >
              <option value="deny">Deny — no network in the sandbox</option>
              <option value="ask">Ask — grant per command (needs command review on)</option>
              <option value="allow">Allow — sandbox always has network</option>
            </select>
          </label>
          <p className="ai-hint">
            "Ask" adds a network checkbox to the shell-approval prompt, so package installs and
            fetches only reach the network when you say so.
          </p>

          <h3>Log View</h3>
          <label className="ai-checkbox-row">
            <input
              type="checkbox"
              checked={settings.log_show_only_diffs ?? false}
              onChange={(e) => saveSettings({ ...settings, log_show_only_diffs: e.target.checked })}
            />
            Log: show only diffs vs previous request
          </label>
          <p className="ai-hint">
            In the log detail, request messages already sent (and cached) in the previous
            request collapse into one "[N previous messages equal]" line — what remains is
            what this request actually paid for.
          </p>
        </div>
      )}

      {tab === "model" && (
        <div className="ai-model-info">
          <h3>Picked model</h3>
          {modelInfoError && <div className="ai-msg ai-msg-error">{modelInfoError}</div>}
          {!modelInfoError && !modelInfo && <p>Loading…</p>}
          {modelInfo && (() => {
            const m = modelInfo.model;
            const c = modelInfo.cache_config;
            const guessed = modelInfo.cache_source.includes("conservative default");
            const provider = typeof m.provider === "string" ? m.provider : JSON.stringify(m.provider);
            return (
              <>
                <table className="ai-stats-table">
                  <tbody>
                    <tr><td>Model</td><td><strong>{m.display_name}</strong></td></tr>
                    <tr><td>Model ID</td><td><code>{m.model_id}</code></td></tr>
                    <tr><td>Provider</td><td>{provider}</td></tr>
                    <tr><td>Context window</td><td>{m.context_window_known ? "" : "~"}{(m.context_window ?? 0).toLocaleString()} tokens{m.context_window_known ? "" : " (fallback guess)"}</td></tr>
                    <tr><td>Max output tokens</td><td>{m.max_tokens.toLocaleString()}</td></tr>
                    <tr><td>Tool calling</td><td>{m.supports_tools ? "yes" : "no"}{m.tool_call_format ? ` (${m.tool_call_format})` : ""}{m.tool_passing ? `, passed via ${m.tool_passing}` : ""}</td></tr>
                    {m.coding_rank != null && <tr><td>Coding rank</td><td>#{m.coding_rank}</td></tr>}
                  </tbody>
                </table>
                <h3>Pricing</h3>
                <table className="ai-stats-table">
                  <tbody>
                    <tr><td>Input</td><td>${m.input_cost_per_m.toFixed(3)} / M tokens</td></tr>
                    <tr><td>Output</td><td>${m.output_cost_per_m.toFixed(3)} / M tokens</td></tr>
                    <tr><td>Cached input</td><td>{m.cached_input_cost_per_m > 0 ? `$${m.cached_input_cost_per_m.toFixed(3)} / M tokens` : "not in catalog"}</td></tr>
                  </tbody>
                </table>
                <h3>Cache model (used by the compaction cost math)</h3>
                {guessed && (
                  <div className="ai-model-cache-warning">
                    ⚠ Unknown provider — the values below are a conservative guess, not this
                    provider's real cache pricing. Add it to provider_cache.json.
                  </div>
                )}
                <table className="ai-stats-table">
                  <tbody>
                    <tr><td>Source</td><td>{modelInfo.cache_source}</td></tr>
                    <tr><td>Mode</td><td>{c.cache_mode}</td></tr>
                    <tr><td>Cached reads</td><td>billed at {(c.cache_read_discount * 100).toFixed(0)}% of input price ({(100 - c.cache_read_discount * 100).toFixed(0)}% discount)</td></tr>
                    <tr><td>Write surcharge</td><td>{c.cache_write_multiplier > 0 ? `${((c.cache_write_multiplier - 1) * 100).toFixed(0)}%` : "none"}</td></tr>
                    <tr><td>TTL</td><td>{c.ttl_seconds != null ? `${c.ttl_seconds}s` : "unknown"}</td></tr>
                    <tr><td>Explicit markers</td><td>{c.requires_markers ? "required" : "not needed"}</td></tr>
                    {c.notes && <tr><td>Notes</td><td>{c.notes}</td></tr>}
                  </tbody>
                </table>
                {modelInfo.summary_model && (
                  <>
                    <h3>Summary model</h3>
                    <table className="ai-stats-table">
                      <tbody>
                        <tr><td>Model</td><td>{modelInfo.summary_model.display_name}</td></tr>
                        <tr><td>Input / output</td><td>${modelInfo.summary_model.input_cost_per_m.toFixed(3)} / ${modelInfo.summary_model.output_cost_per_m.toFixed(3)} per M</td></tr>
                      </tbody>
                    </table>
                  </>
                )}
              </>
            );
          })()}
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
                  <tr><td>Cache hit %</td><td>{(() => {
                    const inp = log.reduce((s, e) => s + e.usage.input_tokens, 0);
                    const cached = log.reduce((s, e) => s + e.usage.cached_tokens, 0);
                    return inp > 0 ? ((cached / inp) * 100).toFixed(1) + "%" : "—";
                  })()}</td></tr>
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
              <p className="ai-meta">
                {inspectEntry.timestamp} | {inspectEntry.duration_ms}ms | ${inspectEntry.cost.total_usd.toFixed(6)}
                {charsPerToken(inspectEntry) && (
                  <> | {charsPerToken(inspectEntry)!.toFixed(2)} chars/token</>
                )}
              </p>
              {inspectEntry.compaction ? (
                <>
                  <span className="ai-log-compacted-badge">
                    {inspectEntry.compaction.kind === "summarized"
                      ? `📝 summarized from ~${compactNum(inspectEntry.compaction.tokens_before)} to ~${compactNum(inspectEntry.compaction.tokens_after)} tokens`
                      : `✂️ trimmed ${inspectEntry.compaction.messages_removed} messages: saved ~${compactNum(Math.max(0, inspectEntry.compaction.tokens_before - inspectEntry.compaction.tokens_after))} tokens`}
                  </span>
                  {(inspectEntry.compaction.details?.length ?? 0) > 0 && (
                    /* Feature 1.1: what the compactor decided, per message */
                    <details className="ai-log-compaction-details">
                      <summary>
                        compaction decisions ({inspectEntry.compaction.details!.length} messages)
                      </summary>
                      {inspectEntry.compaction.details!.map((det, i) => {
                        const icon =
                          det.action === "trimmed" ? "✂️"
                          : det.action === "summarized" ? "📝"
                          : det.action === "trim_candidate" ? "◔"
                          : "🔒";
                        const label =
                          det.action === "trimmed" ? "trimmed"
                          : det.action === "summarized" ? "folded into summary"
                          : det.action === "trim_candidate" ? "candidate (kept — cost math)"
                          : "kept (user message)";
                        return (
                          <div key={i} className={`ai-compact-det ai-compact-${det.action}`}>
                            <span className="ai-compact-icon" title={label}>{icon}</span>
                            <span className="ai-compact-role">{det.role}</span>
                            <span className="ai-compact-tokens">~{compactNum(det.tokens)} tok</span>
                            <span className="ai-compact-preview">{det.preview}</span>
                          </div>
                        );
                      })}
                    </details>
                  )}
                </>
              ) : inspectEntry.was_compacted ? (
                <span className="ai-log-compacted-badge">⚡ compacted</span>
              ) : null}
              {inspectEntry.usage.cached_tokens > 0 && (
                <p className="ai-log-cache-legend">
                  <span className="legend-cached">■</span> cached input&nbsp;&nbsp;
                  <span className="legend-uncached">■</span> not cached
                </p>
              )}

              {(inspectEntry.tool_names?.length ?? 0) > 0 && (
                <details className="ai-log-tools-provided">
                  <summary>
                    Tools provided: {inspectEntry.tools_provided ?? 0} (
                    {sizeBoth(
                      JSON.stringify(inspectEntry.tool_schemas ?? []).length,
                      charsPerToken(inspectEntry)
                    )}
                    ) — [{inspectEntry.tool_names?.join(", ")}]
                  </summary>
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
              {(() => {
                // bugs.md Feature 5: collapse the message prefix already sent
                // in the previous request (log is newest-first, so the
                // previous request lives at index + 1).
                let skip = 0;
                if (settings?.log_show_only_diffs) {
                  const idx = log.findIndex((e) => e.id === inspectEntry.id);
                  const prev = idx >= 0 ? log[idx + 1] : undefined;
                  if (prev) {
                    const key = (m: any) =>
                      JSON.stringify([m.role, m.content, m.tool_calls ?? [], m.tool_call_id ?? null]);
                    const cur = inspectEntry.request_messages;
                    const old = prev.request_messages;
                    while (
                      skip < cur.length - 1 &&
                      skip < old.length &&
                      key(cur[skip]) === key(old[skip])
                    ) {
                      skip++;
                    }
                  }
                }
                return (
                  <>
                    {skip > 0 && (
                      <div className="ai-log-equal-note">
                        [{skip} previous message{skip === 1 ? "" : "s"} equal — hidden]
                      </div>
                    )}
                    {inspectEntry.request_messages.slice(skip).map((m, i0) => {
                      const i = i0 + skip;
                      return (
              <div
                  key={i}
                  className={`ai-log-msg ${
                    inspectEntry.usage.cached_tokens > 0
                      ? cachedFlags(inspectEntry)[i]
                        ? "log-cached"
                        : "log-uncached"
                      : ""
                  }`}
                >
                  {m.role.toUpperCase() === "SYSTEM" ? (
                    <details className="ai-log-system-prompt">
                      <summary><strong>SYSTEM PROMPT</strong> ({sizeBoth(m.content.length, charsPerToken(inspectEntry))})</summary>
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
                      );
                    })}
                  </>
                );
              })()}

              <h5>Response</h5>
              {inspectEntry.response_thinking && (
                <details className="ai-log-thinking">
                  <summary>💭 thinking ({inspectEntry.response_thinking.length} chars)</summary>
                  <pre>{inspectEntry.response_thinking}</pre>
                </details>
              )}
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
                  {entry.compaction && (
                    <span
                      className="ai-log-compact-icon"
                      title={
                        entry.compaction.kind === "summarized"
                          ? `summarized from ~${compactNum(entry.compaction.tokens_before)} to ~${compactNum(entry.compaction.tokens_after)} tokens`
                          : `trimmed ${entry.compaction.messages_removed} messages (saved ~${compactNum(Math.max(0, entry.compaction.tokens_before - entry.compaction.tokens_after))} tokens)`
                      }
                    >
                      {entry.compaction.kind === "summarized" ? "📝" : "✂️"}
                    </span>
                  )}
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
