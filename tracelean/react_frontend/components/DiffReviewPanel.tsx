import { useCallback, useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";

/** P10: staged agent edit awaiting per-hunk approval (core diff_pipeline). */
interface DiffHunk {
  id: string;
  original_start: number;
  original_count: number;
  proposed_start: number;
  proposed_count: number;
  original_lines: string[];
  proposed_lines: string[];
  accepted: boolean;
}

interface PendingDiff {
  id: string;
  file: string;
  original: string;
  proposed: string;
  hunks: DiffHunk[];
  agent: string;
  timestamp: string;
}

/** bugs.md Feature 2b: pending AI diffs reuse the undo-tree hover pipeline —
 * the same NodeDiff shape drives the editor's inline red/green decorations
 * and the file tree's +/− badges. */
function toNodeDiff(diffs: PendingDiff[]) {
  return {
    files: diffs.map((d) => ({
      path: d.file,
      added: d.hunks.reduce((s, h) => s + h.proposed_lines.length, 0),
      removed: d.hunks.reduce((s, h) => s + h.original_lines.length, 0),
      hunks: d.hunks.map((h) => ({
        current_start_line: h.original_start + 1,
        removed_lines: h.original_lines,
        added_lines: h.proposed_lines,
      })),
    })),
  };
}

/**
 * P10 agent edit review, unified with the undo-tree diff view (Feature 2b):
 * pending edits render in the editor with the same inline diff decorations as
 * undo-node hovers, changed files get the same tree badges, and the panel is
 * a file strip (click or Alt+↑/↓ to cycle) with per-hunk ✓/✗ on the right.
 */
export function DiffReviewPanel() {
  const [diffs, setDiffs] = useState<PendingDiff[]>([]);
  const [selected, setSelected] = useState(0);
  const [status, setStatus] = useState<string | null>(null);
  const diffsRef = useRef<PendingDiff[]>([]);
  diffsRef.current = diffs;

  const broadcast = useCallback((all: PendingDiff[]) => {
    window.dispatchEvent(
      new CustomEvent("undo-hover-diff", {
        detail: all.length > 0 ? { nodeDiff: toNodeDiff(all), nodeId: "ai-pending" } : null,
      })
    );
  }, []);

  const refresh = useCallback(async () => {
    try {
      const next = await invoke<PendingDiff[]>("get_pending_diffs");
      setDiffs(next);
      setSelected((s) => Math.min(s, Math.max(0, next.length - 1)));
      broadcast(next);
    } catch (e) {
      console.error("get_pending_diffs failed:", e);
    }
  }, [broadcast]);

  useEffect(() => {
    refresh();
    const unlisten = listen("pending-diffs-changed", () => refresh());
    return () => { unlisten.then((fn) => fn()); };
  }, [refresh]);

  // Clear editor/tree decorations when the last pending diff goes away.
  useEffect(() => () => broadcast([]), [broadcast]);

  const selectFile = useCallback((idx: number) => {
    const all = diffsRef.current;
    if (all.length === 0) return;
    const next = ((idx % all.length) + all.length) % all.length;
    setSelected(next);
    const d = all[next];
    // Open the file so the inline decorations are visible, jump to first hunk.
    const line = d.hunks.length > 0 ? d.hunks[0].original_start + 1 : null;
    window.dispatchEvent(new CustomEvent("tracelean-navigate", { detail: { path: d.file, line } }));
    broadcast(all);
  }, [broadcast]);

  // Alt+↑/↓ cycles through files with pending differences (Feature 2b).
  useEffect(() => {
    const handler = (e: KeyboardEvent) => {
      if (!e.altKey || diffsRef.current.length === 0) return;
      if (e.key === "ArrowDown") {
        e.preventDefault();
        selectFile(selected + 1);
      } else if (e.key === "ArrowUp") {
        e.preventDefault();
        selectFile(selected - 1);
      }
    };
    window.addEventListener("keydown", handler);
    return () => window.removeEventListener("keydown", handler);
  }, [selectFile, selected]);

  const setHunk = async (diffId: string, hunkId: string, accepted: boolean) => {
    try {
      await invoke(accepted ? "accept_diff_hunk" : "reject_diff_hunk", { diffId, hunkId });
      await refresh();
    } catch (e) {
      setStatus(String(e));
    }
  };

  const acceptAll = async (diffId: string) => {
    try {
      await invoke("accept_all_hunks", { diffId });
      await refresh();
    } catch (e) {
      setStatus(String(e));
    }
  };

  const apply = async (diffId: string) => {
    try {
      const msg = await invoke<string>("apply_accepted_hunks", { diffId });
      setStatus(msg);
      await refresh();
    } catch (e) {
      setStatus(String(e));
    }
  };

  const discard = async (diffId: string) => {
    try {
      await invoke("discard_pending_diff", { diffId });
      setStatus(null);
      await refresh();
    } catch (e) {
      setStatus(String(e));
    }
  };

  if (diffs.length === 0) return null;
  const d = diffs[Math.min(selected, diffs.length - 1)];

  return (
    <div className="diff-review-panel">
      <div className="diff-review-header">
        <strong>Agent edits pending review</strong>
        <span className="diff-review-count">{diffs.length}</span>
        {diffs.length > 1 && (
          <span className="diff-review-nav">
            <button onClick={() => selectFile(selected - 1)} title="Previous file (Alt+↑)">▲</button>
            <button onClick={() => selectFile(selected + 1)} title="Next file (Alt+↓)">▼</button>
          </span>
        )}
      </div>
      {status && <div className="diff-review-status">{status}</div>}
      {/* Changed-files strip: same role as the file-tree badges on undo hover */}
      <div className="diff-review-files">
        {diffs.map((f, i) => (
          <button
            key={f.id}
            className={`diff-review-filechip ${i === selected ? "active" : ""}`}
            onClick={() => selectFile(i)}
            title={`by ${f.agent}`}
          >
            {f.file.split("/").pop()}
            <span className="chip-added">+{f.hunks.reduce((s, h) => s + h.proposed_lines.length, 0)}</span>
            <span className="chip-removed">−{f.hunks.reduce((s, h) => s + h.original_lines.length, 0)}</span>
          </button>
        ))}
      </div>
      <div className="diff-review-file">
        <div className="diff-review-file-header">
          <span className="diff-review-path">{d.file}</span>
          <span className="diff-review-agent">by {d.agent}</span>
          <button onClick={() => acceptAll(d.id)} title="Accept every hunk">✓ all</button>
          <button
            onClick={() => apply(d.id)}
            disabled={!d.hunks.some((h) => h.accepted)}
            title="Apply accepted hunks as one undo step"
          >
            Apply
          </button>
          <button onClick={() => discard(d.id)} title="Throw the whole diff away">✗ Discard</button>
        </div>
        {d.hunks.map((h) => (
          <div key={h.id} className={`diff-hunk ${h.accepted ? "hunk-accepted" : ""}`}>
            <div className="diff-hunk-bar">
              <span className="diff-hunk-loc">@ line {h.original_start + 1}</span>
              <button
                className={h.accepted ? "hunk-btn active" : "hunk-btn"}
                onClick={() => setHunk(d.id, h.id, true)}
              >
                ✓
              </button>
              <button
                className={!h.accepted ? "hunk-btn reject active" : "hunk-btn reject"}
                onClick={() => setHunk(d.id, h.id, false)}
              >
                ✗
              </button>
            </div>
            {h.original_lines.map((l, i) => (
              <pre key={`o${i}`} className="diff-line removed">− {l}</pre>
            ))}
            {h.proposed_lines.map((l, i) => (
              <pre key={`p${i}`} className="diff-line added">+ {l}</pre>
            ))}
          </div>
        ))}
      </div>
    </div>
  );
}
