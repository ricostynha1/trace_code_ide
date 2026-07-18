import { useCallback, useEffect, useState } from "react";
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

/**
 * P10 agent edit review: pending diffs staged by review mode. Per-hunk ✓/✗,
 * accept all, apply (one undo node), discard. Appears automatically when the
 * agent stages an edit; hidden when nothing is pending.
 */
export function DiffReviewPanel() {
  const [diffs, setDiffs] = useState<PendingDiff[]>([]);
  const [status, setStatus] = useState<string | null>(null);

  const refresh = useCallback(async () => {
    try {
      setDiffs(await invoke<PendingDiff[]>("get_pending_diffs"));
    } catch (e) {
      console.error("get_pending_diffs failed:", e);
    }
  }, []);

  useEffect(() => {
    refresh();
    const unlisten = listen("pending-diffs-changed", () => refresh());
    return () => { unlisten.then((fn) => fn()); };
  }, [refresh]);

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

  return (
    <div className="diff-review-panel">
      <div className="diff-review-header">
        <strong>Agent edits pending review</strong>
        <span className="diff-review-count">{diffs.length}</span>
      </div>
      {status && <div className="diff-review-status">{status}</div>}
      {diffs.map((d) => (
        <div key={d.id} className="diff-review-file">
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
      ))}
    </div>
  );
}
