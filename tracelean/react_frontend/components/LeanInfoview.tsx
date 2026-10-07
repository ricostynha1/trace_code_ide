import { useCallback, useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";

/**
 * The Lean infoview: proof state beside the file, the way the VS Code extension
 * does it.
 *
 * It is built entirely out of LSP information -- `$/lean/plainGoal`,
 * `$/lean/plainTermGoal` and `textDocument/publishDiagnostics`, batched into one
 * `lsp_goal_at` round trip per cursor move.
 *
 * The limit, stated rather than hidden: those endpoints return *rendered text*.
 * The VS Code infoview is richer -- clickable subterms, `Try this` widgets --
 * and that comes from Lean's own RPC layer (`$/lean/rpc/connect`, then
 * `Lean.Widget.getInteractiveGoals`), a session protocol layered on top of LSP
 * rather than part of it. The text view is most of the day-to-day value for a
 * fraction of the work, so it comes first.
 *
 * The rule this panel must never break: **an empty goal list means the proof is
 * complete**. When no Lean server is running there is no goal list at all, and
 * the panel says so. Rendering "no goals" for a server that never started would
 * be precisely the green-when-unchecked lie the project exists to prevent.
 */

interface ServerStatus {
  kind: string;
  running: boolean;
  command: string[] | null;
  problem: string | null;
  hint: string | null;
}

interface RawDiagnostic {
  file: string;
  line: number;
  character: number;
  severity: string;
  message: string;
  source: string | null;
}

interface GoalPayload {
  status: ServerStatus | null;
  /** `{ rendered, goals }` from `$/lean/plainGoal`, or null. */
  goals: { rendered?: string; goals?: string[] } | null;
  /** `{ goal }` from `$/lean/plainTermGoal`, or null. */
  term_goal: { goal?: string } | string | null;
  diagnostics: RawDiagnostic[];
  line: number;
}

const SEVERITY_COLOR: Record<string, string> = {
  error: "#f14c4c",
  warning: "#d19a66",
  information: "#61afef",
  hint: "#7f848e",
};

/** Pull the goal strings out of whichever shape the server answered with. */
function goalTexts(payload: GoalPayload): string[] {
  const plain = payload.goals;
  if (plain) {
    if (Array.isArray(plain.goals) && plain.goals.length > 0) return plain.goals;
    if (typeof plain.rendered === "string" && plain.rendered.trim()) return [plain.rendered];
  }
  const term = payload.term_goal;
  if (typeof term === "string" && term.trim()) return [term];
  if (term && typeof term === "object" && typeof term.goal === "string" && term.goal.trim()) {
    return [term.goal];
  }
  return [];
}

/**
 * One goal, with the turnstile line set apart.
 *
 * Hypotheses above, target below: that is how a proof is read, and running them
 * together as one blob is what makes a raw LSP dump hard to use.
 */
function Goal({ text, index, total }: { text: string; index: number; total: number }) {
  const lines = text.split("\n");
  const turnstile = lines.findIndex((l) => l.trimStart().startsWith("⊢"));

  return (
    <div className="lean-goal">
      {total > 1 && (
        <div className="lean-goal-label">
          goal {index + 1} of {total}
        </div>
      )}
      <pre className="lean-goal-body">
        {lines.map((line, i) => (
          <span
            key={i}
            className={
              turnstile >= 0 && i >= turnstile ? "lean-goal-target" : "lean-goal-hyp"
            }
          >
            {line}
            {"\n"}
          </span>
        ))}
      </pre>
    </div>
  );
}

export function LeanInfoview({
  visible,
  filePath,
  onClose,
  onFileSelect,
}: {
  visible: boolean;
  filePath: string | null;
  onClose: () => void;
  onFileSelect: (file: string, line?: number) => void;
}) {
  const [payload, setPayload] = useState<GoalPayload | null>(null);
  const [error, setError] = useState("");
  const [pinned, setPinned] = useState<number | null>(null);
  const [busy, setBusy] = useState(false);
  const latest = useRef(0);

  const isLean = !!filePath && filePath.endsWith(".lean");

  const load = useCallback(
    async (charPos: number) => {
      if (!filePath) return;
      const ticket = ++latest.current;
      setBusy(true);
      try {
        const result = await invoke<GoalPayload>("lsp_goal_at", {
          file: filePath,
          charPos,
        });
        // Cursor moves faster than the server answers; a reply that has been
        // overtaken describes a position the user has already left.
        if (ticket === latest.current) {
          setPayload(result);
          setError("");
        }
      } catch (e) {
        if (ticket === latest.current) setError(String(e));
      } finally {
        if (ticket === latest.current) setBusy(false);
      }
    },
    [filePath]
  );

  // Follow the cursor, debounced: elaboration is not free and the cursor moves
  // on every arrow key.
  useEffect(() => {
    if (!visible || !isLean) return;
    let timer: ReturnType<typeof setTimeout> | null = null;
    const handler = (e: Event) => {
      if (pinned !== null) return;
      const detail = (e as CustomEvent).detail as { file?: string; charPos?: number };
      if (!detail || detail.file !== filePath || typeof detail.charPos !== "number") return;
      if (timer) clearTimeout(timer);
      const at = detail.charPos;
      timer = setTimeout(() => load(at), 120);
    };
    window.addEventListener("tracelean-cursor", handler);
    return () => {
      if (timer) clearTimeout(timer);
      window.removeEventListener("tracelean-cursor", handler);
    };
  }, [visible, isLean, filePath, pinned, load]);

  // A pin request from Verify mode (`v g`) freezes the panel at that position,
  // so a tactic can be edited while the goal it has to close stays on screen.
  useEffect(() => {
    if (!visible) return;
    const handler = (e: Event) => {
      const detail = (e as CustomEvent).detail as { charPos?: number };
      if (typeof detail?.charPos !== "number") return;
      setPinned(detail.charPos);
      load(detail.charPos);
    };
    window.addEventListener("tracelean-pin-goal", handler);
    return () => window.removeEventListener("tracelean-pin-goal", handler);
  }, [visible, load]);

  useEffect(() => {
    setPayload(null);
    setPinned(null);
    setError("");
  }, [filePath]);

  if (!visible) return null;

  const status = payload?.status ?? null;
  const serverDown = !!status && !status.running;
  const goals = payload ? goalTexts(payload) : [];
  const diagnostics = payload?.diagnostics ?? [];
  const here = diagnostics.filter((d) => d.line === (payload?.line ?? -1));
  const elsewhere = diagnostics.filter((d) => d.line !== (payload?.line ?? -1));

  return (
    <div className="requirements-panel lean-infoview">
      <div className="panel-header">
        <span className="panel-title">Lean</span>
        {pinned !== null && (
          <button
            className="lean-pin active"
            title="Unpin: follow the cursor again"
            onClick={() => setPinned(null)}
          >
            pinned
          </button>
        )}
        {busy && <span className="lean-busy">...</span>}
        <button onClick={onClose}>x</button>
      </div>

      {!isLean ? (
        <div className="req-empty">Open a `.lean` file to see its proof state.</div>
      ) : serverDown ? (
        // Never an empty goal list here: that would read as "proof complete".
        <div className="lean-server-down">
          <div className="lean-server-problem">
            {status?.problem ?? "The Lean language server is not running."}
          </div>
          {status?.hint && <div className="lean-server-hint">{status.hint}</div>}
          <div className="lean-server-note">
            Until it starts, this panel can say nothing about the proof -- which is
            not the same as saying the proof is finished.
          </div>
        </div>
      ) : error ? (
        <div className="lean-server-down">
          <div className="lean-server-problem">{error}</div>
        </div>
      ) : !payload ? (
        <div className="req-empty">Put the cursor in the file to see the goal here.</div>
      ) : (
        <div className="lean-sections">
          <section className="lean-section">
            <h4>Goal{pinned !== null ? " (pinned)" : ""}</h4>
            {goals.length === 0 ? (
              <div className="lean-no-goals">
                No goals at this position.
                <span className="lean-no-goals-note">
                  {" "}
                  In a tactic block this means the proof is complete here.
                </span>
              </div>
            ) : (
              goals.map((g, i) => <Goal key={i} text={g} index={i} total={goals.length} />)
            )}
          </section>

          {here.length > 0 && (
            <section className="lean-section">
              <h4>Messages here</h4>
              {here.map((d, i) => (
                <div key={i} className="lean-diagnostic">
                  <span style={{ color: SEVERITY_COLOR[d.severity] ?? "#abb2bf" }}>
                    {d.severity}
                  </span>
                  <pre>{d.message}</pre>
                </div>
              ))}
            </section>
          )}

          <section className="lean-section">
            <h4>All messages ({diagnostics.length})</h4>
            {diagnostics.length === 0 ? (
              <div className="lean-no-goals">No messages -- the file elaborated cleanly.</div>
            ) : elsewhere.length === 0 ? (
              <div className="lean-no-goals">All of them are at the cursor, above.</div>
            ) : (
              elsewhere.map((d, i) => (
                <div
                  key={i}
                  className="lean-diagnostic clickable"
                  onClick={() => onFileSelect(d.file, d.line)}
                  title={`${d.file}:${d.line + 1}`}
                >
                  <span style={{ color: SEVERITY_COLOR[d.severity] ?? "#abb2bf" }}>
                    {d.line + 1}
                  </span>
                  <pre>{d.message}</pre>
                </div>
              ))
            )}
          </section>
        </div>
      )}
    </div>
  );
}
