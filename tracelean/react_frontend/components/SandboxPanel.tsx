import { useEffect, useState, useCallback } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { setActiveSandboxSession } from "./sandboxStore";

/** Mirrors `tracelean_core::sandbox::session::SessionSpec`. */
interface SessionSpec {
  id: string;
  project_root: string;
  work_dir: string;
  allow_network: boolean;
  created: string;
}

/** Mirrors `tracelean_core::sandbox::session::SandboxCapabilities`. */
interface SandboxCapabilities {
  bwrap_available: boolean;
  reflink_available: boolean;
}

/** Mirrors `sandbox_commands::SandboxChangeEntry`. */
interface SandboxChangeEntry {
  path: string;
  kind: "created" | "modified" | "deleted";
}

interface SandboxPanelProps {
  visible: boolean;
  onClose: () => void;
  onFileSelect: (path: string) => void;
}

const KIND_BADGE: Record<string, { label: string; className: string }> = {
  created: { label: "A", className: "sandbox-badge-add" },
  modified: { label: "M", className: "sandbox-badge-mod" },
  deleted: { label: "D", className: "sandbox-badge-del" },
};

/** Sandboxed-workspace panel: create/discard a session, get the command (or
 * a button) to enter it in a real terminal, and watch what it changes.
 * tracelean never launches anything inside the session itself — see
 * `core/src/bin/tracelean-sandbox.rs` — this panel only manages the
 * workspace and displays what the watcher observes.
 */
export function SandboxPanel({ visible, onClose, onFileSelect }: SandboxPanelProps) {
  const [caps, setCaps] = useState<SandboxCapabilities | null>(null);
  const [sessions, setSessions] = useState<SessionSpec[]>([]);
  const [activeId, setActiveId] = useState<string | null>(null);
  const [changes, setChanges] = useState<SandboxChangeEntry[]>([]);
  const [shellCmd, setShellCmd] = useState<string>("");
  const [busy, setBusy] = useState(false);
  const [message, setMessage] = useState<string>("");
  const [copied, setCopied] = useState(false);

  const refreshSessions = useCallback(async () => {
    try {
      setSessions(await invoke<SessionSpec[]>("sandbox_list_sessions"));
    } catch {
      setSessions([]);
    }
  }, []);

  const refreshChanges = useCallback(async (id: string) => {
    try {
      setChanges(await invoke<SandboxChangeEntry[]>("sandbox_changes", { sessionId: id }));
    } catch {
      setChanges([]);
    }
  }, []);

  useEffect(() => {
    if (!visible) return;
    invoke<SandboxCapabilities>("sandbox_capabilities").then(setCaps).catch(() => setCaps(null));
    refreshSessions();
  }, [visible, refreshSessions]);

  useEffect(() => {
    if (!activeId) {
      setShellCmd("");
      setChanges([]);
      return;
    }
    invoke<string>("sandbox_shell_command", { sessionId: activeId }).then(setShellCmd).catch(() => setShellCmd(""));
    refreshChanges(activeId);
  }, [activeId, refreshChanges]);

  // Live updates from the session's filesystem watcher (core/src/sandbox/watch.rs).
  useEffect(() => {
    const unlisten = listen("sandbox-changed", (event: any) => {
      const payload = typeof event.payload === "string" ? JSON.parse(event.payload) : event.payload;
      if (activeId && payload?.session_id === activeId) refreshChanges(activeId);
    });
    return () => { unlisten.then((fn) => fn()); };
  }, [activeId, refreshChanges]);

  const handleCreate = async () => {
    setBusy(true);
    setMessage("");
    try {
      const spec = await invoke<SessionSpec>("sandbox_create_session", { allowNetwork: true });
      setSessions((prev) => [spec, ...prev]);
      setActiveId(spec.id);
      setActiveSandboxSession(spec.id);
    } catch (e) {
      setMessage(`Failed to create session: ${e}`);
    } finally {
      setBusy(false);
    }
  };

  const handleDestroy = async (id: string) => {
    setBusy(true);
    try {
      await invoke("sandbox_destroy_session", { sessionId: id });
      setSessions((prev) => prev.filter((s) => s.id !== id));
      if (activeId === id) {
        setActiveId(null);
        setActiveSandboxSession(null);
      }
    } catch (e) {
      setMessage(`Failed to destroy session: ${e}`);
    } finally {
      setBusy(false);
    }
  };

  const handleDestroyAll = async () => {
    if (sessions.length === 0) return;
    if (!window.confirm(`Destroy all ${sessions.length} sandbox session(s) for this project? This deletes their working copies on disk.`)) {
      return;
    }
    setBusy(true);
    setMessage("");
    try {
      const failures = await invoke<string[]>("sandbox_destroy_all_sessions");
      await refreshSessions();
      setActiveId(null);
      setActiveSandboxSession(null);
      if (failures.length > 0) {
        setMessage(`Some sessions could not be removed: ${failures.join("; ")}`);
      }
    } catch (e) {
      setMessage(`Failed to destroy sessions: ${e}`);
    } finally {
      setBusy(false);
    }
  };

  const handleOpenTerminal = async (id: string) => {
    try {
      await invoke("sandbox_open_terminal", { sessionId: id });
    } catch (e) {
      setMessage(`Couldn't launch a terminal automatically (${e}) — copy the command below instead.`);
    }
  };

  const handleCopy = async () => {
    try {
      await navigator.clipboard.writeText(shellCmd);
      setCopied(true);
      setTimeout(() => setCopied(false), 1500);
    } catch {
      // Clipboard may be unavailable in some webview contexts — the text is
      // still selectable in the box below.
    }
  };

  const handleRevert = async (path: string) => {
    if (!activeId) return;
    try {
      await invoke("sandbox_revert_file", { sessionId: activeId, path });
      refreshChanges(activeId);
    } catch (e) {
      setMessage(`Failed to revert ${path}: ${e}`);
    }
  };

  if (!visible) return null;

  return (
    <div className="sandbox-panel">
      <div className="panel-header">
        <span className="panel-title">SANDBOX</span>
        <div className="panel-actions">
          <button className="panel-btn" onClick={refreshSessions} title="Refresh sessions">⟳</button>
          <button className="panel-close" onClick={onClose}>✕</button>
        </div>
      </div>

      {caps && !caps.bwrap_available && (
        <div className="sandbox-warning">bwrap not found — sandbox sessions need bubblewrap installed.</div>
      )}
      {caps && caps.bwrap_available && !caps.reflink_available && (
        <div className="sandbox-hint">
          Reflink copies aren't supported on this filesystem — session start will do a full copy instead (slower, more disk).
        </div>
      )}

      <div className="sandbox-create-row">
        <button className="req-create-btn" disabled={busy || (caps ? !caps.bwrap_available : false)} onClick={handleCreate}>
          + New sandbox session
        </button>
        {sessions.length > 0 && (
          <button className="panel-btn sandbox-destroy-all-btn" disabled={busy} onClick={handleDestroyAll} title="Destroy every session for this project, including ones left over from a previous run">
            🗑 Destroy all ({sessions.length})
          </button>
        )}
      </div>

      {message && <div className="req-message">{message}</div>}

      <div className="sandbox-session-list">
        {sessions.length === 0 && <div className="sandbox-empty">No sessions yet for this project.</div>}
        {sessions.map((s) => (
          <div key={s.id} className={`sandbox-session-item${activeId === s.id ? " active" : ""}`}>
            <div
              className="sandbox-session-row"
              onClick={() => { setActiveId(s.id); setActiveSandboxSession(s.id); }}
            >
              <span className="sandbox-session-id">{s.id.slice(0, 8)}</span>
              <span className="sandbox-session-created">{new Date(s.created).toLocaleString()}</span>
              {!s.allow_network && <span className="sandbox-net-off" title="Network disabled">🚫net</span>}
            </div>
            <div className="sandbox-session-actions">
              <button className="panel-btn" onClick={() => handleOpenTerminal(s.id)} title="Open a terminal into this session">▶ Terminal</button>
              <button className="panel-btn" onClick={() => handleDestroy(s.id)} title="Discard session">🗑</button>
            </div>
          </div>
        ))}
      </div>

      {activeId && (
        <>
          <div className="sandbox-shell-cmd-row">
            <div className="panel-title">RUN IN YOUR TERMINAL</div>
            <code className="sandbox-shell-cmd">{shellCmd || "…"}</code>
            <button className="panel-btn" onClick={handleCopy}>{copied ? "Copied!" : "Copy"}</button>
          </div>

          <div className="panel-title sandbox-changes-header">SESSION CHANGES ({changes.length})</div>
          <div className="sandbox-changes-list">
            {changes.length === 0 && <div className="sandbox-empty">No changes yet — run something in the session.</div>}
            {changes.map((c) => {
              const badge = KIND_BADGE[c.kind] ?? { label: "?", className: "" };
              return (
                <div key={c.path} className="sandbox-change-item">
                  <span className={`sandbox-badge ${badge.className}`}>{badge.label}</span>
                  <span className="sandbox-change-path" onClick={() => onFileSelect(c.path)}>{c.path}</span>
                  <button className="panel-btn" onClick={() => handleRevert(c.path)} title="Revert to the real tree's content">↺</button>
                </div>
              );
            })}
          </div>
        </>
      )}
    </div>
  );
}
