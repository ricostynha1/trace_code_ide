import { useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";

interface TestFailure {
  name: string;
  file: string | null;
  line: number | null;
  message: string;
}

interface CommandOutput {
  command: string;
  stdout: string;
  stderr: string;
  exit_code: number;
  duration_ms: number;
  failures: TestFailure[];
}

/** bugs.md Feature 4: a shell command the agent ran, mirrored here. */
interface AgentShellEntry {
  command: string;
  output: string;
  success: boolean;
  duration_ms: number;
}

/**
 * P12: bottom terminal panel — run the project's test command (or any shell
 * command) in the project root; failures become clickable diagnostics and can
 * be sent to the AI chat with one click.
 */
export function TerminalPanel({ projectOpen }: { projectOpen: boolean }) {
  const [expanded, setExpanded] = useState(false);
  const [command, setCommand] = useState("");
  const [running, setRunning] = useState(false);
  const [result, setResult] = useState<CommandOutput | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [agentLog, setAgentLog] = useState<AgentShellEntry[]>([]);
  const outputRef = useRef<HTMLPreElement | null>(null);

  // bugs.md Feature 4: agent-run shell commands appear here too.
  useEffect(() => {
    const unlisten = listen("agent-shell", (event) => {
      const p = event.payload;
      let entry: AgentShellEntry | null = null;
      if (typeof p === "string") {
        try {
          entry = JSON.parse(p) as AgentShellEntry;
        } catch {}
      } else if (p && typeof p === "object") {
        entry = p as AgentShellEntry;
      }
      if (entry && typeof entry.command === "string") {
        setAgentLog((prev) => [...prev.slice(-49), entry!]);
      }
    });
    return () => {
      unlisten.then((fn) => fn());
    };
  }, []);

  // Load the configured/detected test command once a project is open.
  useEffect(() => {
    if (!projectOpen) return;
    invoke<string>("get_test_command")
      .then((c) => setCommand((prev) => prev || c))
      .catch(() => {});
  }, [projectOpen]);

  useEffect(() => {
    outputRef.current?.scrollTo(0, outputRef.current.scrollHeight);
  }, [result]);

  const run = async (cmd?: string) => {
    const toRun = (cmd ?? command).trim();
    if (!toRun || running) return;
    setRunning(true);
    setError(null);
    setExpanded(true);
    try {
      const out = await invoke<CommandOutput>("run_project_command", { command: toRun });
      setResult(out);
      // Persist as the project's test command for next time
      invoke("set_test_command", { command: toRun }).catch(() => {});
    } catch (e) {
      setError(String(e));
    } finally {
      setRunning(false);
    }
  };

  const openFailure = (f: TestFailure) => {
    if (!f.file) return;
    window.dispatchEvent(new CustomEvent("tracelean-navigate", { detail: { path: f.file, line: f.line } }));
  };

  const fixWithAi = () => {
    if (!result) return;
    const failures = result.failures
      .map((f) => `- ${f.name}${f.file ? ` (${f.file}${f.line ? `:${f.line}` : ""})` : ""}${f.message ? `: ${f.message}` : ""}`)
      .join("\n");
    const tail = (result.stdout + "\n" + result.stderr).slice(-3000);
    const prompt = `The test command \`${result.command}\` failed (exit ${result.exit_code}).\n\nFailures:\n${failures || "(unparsed)"}\n\nOutput tail:\n\`\`\`\n${tail}\n\`\`\`\n\nPlease investigate and fix the failing tests.`;
    window.dispatchEvent(new CustomEvent("fix-with-ai", { detail: { prompt } }));
  };

  if (!projectOpen) return null;

  return (
    <div className={`terminal-panel ${expanded ? "expanded" : ""}`}>
      <div className="terminal-bar">
        <button className="terminal-toggle" onClick={() => setExpanded((v) => !v)}>
          {expanded ? "▾" : "▸"} Terminal
          {agentLog.length > 0 && (
            <span className="terminal-agent-badge" title="Shell commands run by the agent">
              🤖 {agentLog.length}
            </span>
          )}
        </button>
        <input
          className="terminal-cmd"
          value={command}
          onChange={(e) => setCommand(e.target.value)}
          onKeyDown={(e) => e.key === "Enter" && run()}
          placeholder="test command (runs in project root)"
        />
        <button className="terminal-run" onClick={() => run()} disabled={running}>
          {running ? "Running…" : "▶ Run tests"}
        </button>
        {result && (
          <span className={`terminal-status ${result.exit_code === 0 ? "ok" : "fail"}`}>
            {result.exit_code === 0 ? "✓ passed" : `✗ exit ${result.exit_code}`} ·{" "}
            {(result.duration_ms / 1000).toFixed(1)}s
          </span>
        )}
        {result && result.exit_code !== 0 && (
          <button className="terminal-fix-ai" onClick={fixWithAi}>🤖 Fix with AI</button>
        )}
      </div>
      {expanded && (
        <div className="terminal-body">
          {error && <div className="terminal-error">{error}</div>}
          {result && result.failures.length > 0 && (
            <div className="terminal-failures">
              {result.failures.map((f, i) => (
                <div
                  key={i}
                  className={`terminal-failure ${f.file ? "clickable" : ""}`}
                  onClick={() => openFailure(f)}
                  title={f.file ? `Open ${f.file}` : undefined}
                >
                  <span className="failure-name">✗ {f.name}</span>
                  {f.file && (
                    <span className="failure-loc">
                      {f.file}
                      {f.line ? `:${f.line}` : ""}
                    </span>
                  )}
                  {f.message && <span className="failure-msg">{f.message.split("\n")[0]}</span>}
                </div>
              ))}
            </div>
          )}
          {agentLog.length > 0 && (
            <div className="terminal-agent-log">
              {agentLog.map((e, i) => (
                <div key={i} className={`terminal-agent-entry ${e.success ? "ok" : "fail"}`}>
                  <div className="terminal-agent-cmd">
                    🤖 $ {e.command}{" "}
                    <span className="terminal-agent-meta">
                      {e.success ? "✓" : "✗"} {(e.duration_ms / 1000).toFixed(1)}s
                    </span>
                  </div>
                  {e.output && <pre className="terminal-agent-output">{e.output}</pre>}
                </div>
              ))}
            </div>
          )}
          <pre className="terminal-output" ref={outputRef}>
            {result ? `${result.stdout}${result.stderr ? "\n" + result.stderr : ""}` : "Run a command to see output."}
          </pre>
        </div>
      )}
    </div>
  );
}
