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
 * bugs.md Bug 0.7: this component is now HEADLESS. The old bottom panel
 * duplicated the diff the editor already shows inline; all controls moved to
 * the diff bar on top of the editor (EditorDiffBar). What remains here is the
 * data plumbing: fetch pending diffs and broadcast them so the editor
 * decorations and the file-tree +/− badges stay live even with no file open.
 */
export function DiffReviewPanel() {
  const [diffs, setDiffs] = useState<PendingDiff[]>([]);

  const broadcast = useCallback((all: PendingDiff[]) => {
    // Always tagged "ai-pending" (even when empty) so diffViewStore can tell
    // "AI diffs went away" apart from a plain undo-tree unhover.
    window.dispatchEvent(
      new CustomEvent("undo-hover-diff", {
        detail: {
          nodeDiff: all.length > 0 ? toNodeDiff(all) : null,
          nodeId: "ai-pending",
        },
      })
    );
  }, []);

  const refresh = useCallback(async () => {
    try {
      const next = await invoke<PendingDiff[]>("get_pending_diffs");
      setDiffs(next);
      broadcast(next);
    } catch (e) {
      console.error("get_pending_diffs failed:", e);
    }
  }, [broadcast]);

  useEffect(() => {
    refresh();
    const unlisten = listen("pending-diffs-changed", () => refresh());
    // The diff bar signals its own mutations (accept/apply/discard) here.
    const winHandler = () => refresh();
    window.addEventListener("pending-diffs-refresh", winHandler);
    return () => {
      unlisten.then((fn) => fn());
      window.removeEventListener("pending-diffs-refresh", winHandler);
    };
  }, [refresh]);

  // Clear editor/tree decorations when the last pending diff goes away.
  useEffect(() => () => broadcast([]), [broadcast]);

  void diffs; // state kept for future use (count badge etc.)
  return null;
}
