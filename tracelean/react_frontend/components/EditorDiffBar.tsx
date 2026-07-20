import { useCallback, useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { NodeDiffT } from "./Editor";
import { diffViewCache } from "./diffViewStore";

/** bugs.md Bug 0.7 / 0.75: the diff "merge bar" on top of the editor.
 *
 * Two independent rows can show:
 * - AI pending-edit review for the open file: accept/reject current hunk,
 *   hunk ◀/▶ navigation, apply/discard, and prev/next file-with-diffs.
 * - A pinned undo-tree node diff: hunk/file navigation + "jump here".
 *
 * The bottom DiffReviewPanel no longer renders content — the inline editor
 * decorations show the diff itself; this bar carries the controls. */

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
  hunks: DiffHunk[];
  agent: string;
  timestamp: string;
}

function matchesFile(diffPath: string, currentFile: string): boolean {
  const normDiff = diffPath.replace(/^\/+/, "");
  const normFile = currentFile.replace(/^\/+/, "");
  return normDiff === normFile || normFile.endsWith(normDiff) || normDiff.endsWith(normFile);
}

interface Props {
  filePath: string;
  /** Scroll the editor to a 1-based line. */
  onGotoLine: (line: number) => void;
  /** Reload buffer content from the backend (after a jump). */
  onFileSync: () => Promise<void>;
}

// Last known pending-diff list, module-scoped so a remounted bar renders it
// immediately instead of flashing empty while get_pending_diffs round-trips.
let cachedPendingDiffs: PendingDiff[] = [];

export function EditorDiffBar({ filePath, onGotoLine, onFileSync }: Props) {
  // ── AI pending diffs (P10 review mode) ──────────────────────────────────
  const [diffs, setDiffs] = useState<PendingDiff[]>(() => cachedPendingDiffs);
  const [hunkIdx, setHunkIdx] = useState(0);
  // ── Pinned undo-node diff (Feature 2b / bug 0.75) ───────────────────────
  // Initialized from diffViewCache: the pin is delivered by a one-shot window
  // event, so after a remount (file navigation) only the cache still has it.
  const [undoPin, setUndoPin] = useState<{ nodeId: string; diff: NodeDiffT } | null>(
    () => diffViewCache.undoPin
  );
  const [undoFileIdx, setUndoFileIdx] = useState(() => diffViewCache.undoFileIdx);
  const [undoHunkIdx, setUndoHunkIdx] = useState(() => diffViewCache.undoHunkIdx);
  const diffsRef = useRef(diffs);
  diffsRef.current = diffs;

  // Keep the durable cache in step with local navigation state.
  useEffect(() => {
    diffViewCache.undoFileIdx = undoFileIdx;
    diffViewCache.undoHunkIdx = undoHunkIdx;
  }, [undoFileIdx, undoHunkIdx]);

  const refresh = useCallback(async () => {
    try {
      const next = await invoke<PendingDiff[]>("get_pending_diffs");
      cachedPendingDiffs = next;
      setDiffs(next);
    } catch (e) {
      console.error("get_pending_diffs failed:", e);
    }
  }, []);

  useEffect(() => {
    refresh();
    const unlisten = listen("pending-diffs-changed", () => refresh());
    const winHandler = () => refresh();
    window.addEventListener("pending-diffs-refresh", winHandler);
    return () => {
      unlisten.then((fn) => fn());
      window.removeEventListener("pending-diffs-refresh", winHandler);
    };
  }, [refresh]);

  // Pinned undo diffs arrive on the same channel as hover diffs, flagged
  // with `pinned: true`; a null detail closes the view.
  useEffect(() => {
    const handler = (e: Event) => {
      const detail = (e as CustomEvent).detail;
      if (!detail || !detail.nodeDiff) {
        setUndoPin(null);
        return;
      }
      if (detail.pinned && detail.nodeId && detail.nodeId !== "ai-pending") {
        setUndoPin({ nodeId: detail.nodeId, diff: detail.nodeDiff as NodeDiffT });
        setUndoFileIdx(0);
        setUndoHunkIdx(0);
      }
    };
    window.addEventListener("undo-hover-diff", handler);
    return () => window.removeEventListener("undo-hover-diff", handler);
  }, []);

  const fileIdx = diffs.findIndex((d) => matchesFile(d.file, filePath));
  const current = fileIdx >= 0 ? diffs[fileIdx] : null;

  // Keep the hunk cursor in range when the diff shrinks / changes.
  useEffect(() => {
    setHunkIdx((h) => Math.min(h, Math.max(0, (current?.hunks.length ?? 1) - 1)));
  }, [current?.id, current?.hunks.length]);

  const gotoHunk = useCallback(
    (idx: number) => {
      if (!current || current.hunks.length === 0) return;
      const n = current.hunks.length;
      const next = ((idx % n) + n) % n;
      setHunkIdx(next);
      onGotoLine(current.hunks[next].original_start + 1);
    },
    [current, onGotoLine]
  );

  const notifyChanged = () => {
    window.dispatchEvent(new CustomEvent("pending-diffs-refresh"));
  };

  const setHunk = async (accepted: boolean) => {
    if (!current) return;
    const hunk = current.hunks[hunkIdx];
    if (!hunk) return;
    try {
      await invoke(accepted ? "accept_diff_hunk" : "reject_diff_hunk", {
        diffId: current.id,
        hunkId: hunk.id,
      });
      await refresh();
      notifyChanged();
      if (hunkIdx < current.hunks.length - 1) gotoHunk(hunkIdx + 1);
    } catch (e) {
      console.error(e);
    }
  };

  const apply = async () => {
    if (!current) return;
    try {
      await invoke<string>("apply_accepted_hunks", { diffId: current.id });
      await refresh();
      notifyChanged();
      await onFileSync();
    } catch (e) {
      console.error(e);
    }
  };

  const acceptAllAndApply = async () => {
    if (!current) return;
    try {
      await invoke("accept_all_hunks", { diffId: current.id });
      await invoke<string>("apply_accepted_hunks", { diffId: current.id });
      await refresh();
      notifyChanged();
      await onFileSync();
    } catch (e) {
      console.error(e);
    }
  };

  const discard = async () => {
    if (!current) return;
    try {
      await invoke("discard_pending_diff", { diffId: current.id });
      await refresh();
      notifyChanged();
    } catch (e) {
      console.error(e);
    }
  };

  const gotoFile = (dir: 1 | -1) => {
    if (diffs.length === 0) return;
    const from = fileIdx >= 0 ? fileIdx : 0;
    const next = (((from + dir) % diffs.length) + diffs.length) % diffs.length;
    const d = diffs[next];
    const line = d.hunks.length > 0 ? d.hunks[0].original_start + 1 : null;
    window.dispatchEvent(
      new CustomEvent("tracelean-navigate", { detail: { path: d.file, line } })
    );
  };

  // ── Undo-pin actions ────────────────────────────────────────────────────
  const undoFiles = undoPin?.diff.files ?? [];
  const undoFile = undoFiles[Math.min(undoFileIdx, Math.max(0, undoFiles.length - 1))];
  const undoHunks = undoFile?.hunks ?? [];

  const gotoUndoHunk = (idx: number) => {
    if (undoHunks.length === 0) return;
    const n = undoHunks.length;
    const next = ((idx % n) + n) % n;
    setUndoHunkIdx(next);
    if (undoFile && matchesFile(undoFile.path, filePath)) {
      onGotoLine(undoHunks[next].current_start_line);
    }
  };

  const gotoUndoFile = (dir: 1 | -1) => {
    if (undoFiles.length === 0) return;
    const next = (((undoFileIdx + dir) % undoFiles.length) + undoFiles.length) % undoFiles.length;
    setUndoFileIdx(next);
    setUndoHunkIdx(0);
    const f = undoFiles[next];
    window.dispatchEvent(
      new CustomEvent("tracelean-navigate", {
        detail: { path: f.path, line: f.hunks[0]?.current_start_line ?? null },
      })
    );
  };

  const closeUndoPin = () => {
    setUndoPin(null);
    window.dispatchEvent(new CustomEvent("undo-unpin"));
    window.dispatchEvent(new CustomEvent("undo-hover-diff", { detail: null }));
  };

  const jumpToNode = async () => {
    if (!undoPin) return;
    try {
      await invoke("jump_to_node", { nodeId: undoPin.nodeId });
      closeUndoPin();
      await onFileSync();
    } catch (e) {
      console.error("jump_to_node failed:", e);
    }
  };

  if (!current && !undoPin) return null;

  return (
    <>
      {current && (
        <div className="editor-diffbar editor-diffbar-ai">
          <span className="diffbar-label">AI edit</span>
          <span className="diffbar-info">
            hunk {Math.min(hunkIdx + 1, current.hunks.length)}/{current.hunks.length}
            {current.hunks[hunkIdx] && (
              <> @ line {current.hunks[hunkIdx].original_start + 1}</>
            )}
          </span>
          <button onClick={() => gotoHunk(hunkIdx - 1)} title="Previous hunk">◀</button>
          <button onClick={() => gotoHunk(hunkIdx + 1)} title="Next hunk">▶</button>
          <button
            className={`diffbar-accept ${current.hunks[hunkIdx]?.accepted ? "active" : ""}`}
            onClick={() => setHunk(true)}
            title="Accept current hunk"
          >
            ✓ accept
          </button>
          <button
            className="diffbar-reject"
            onClick={() => setHunk(false)}
            title="Reject current hunk"
          >
            ✗ reject
          </button>
          <span className="diffbar-sep" />
          <button
            onClick={apply}
            disabled={!current.hunks.some((h) => h.accepted)}
            title="Apply accepted hunks as one undo step"
          >
            Apply
          </button>
          <button onClick={acceptAllAndApply} title="Accept every hunk and apply">
            ✓✓ all
          </button>
          <button className="diffbar-reject" onClick={discard} title="Discard this file's diff">
            Discard
          </button>
          {diffs.length > 1 && (
            <>
              <span className="diffbar-sep" />
              <button onClick={() => gotoFile(-1)} title="Previous file with diffs">⏮</button>
              <span className="diffbar-info">
                file {(fileIdx >= 0 ? fileIdx : 0) + 1}/{diffs.length}
              </span>
              <button onClick={() => gotoFile(1)} title="Next file with diffs">⏭</button>
            </>
          )}
        </div>
      )}
      {undoPin && (
        <div className="editor-diffbar editor-diffbar-undo">
          <span className="diffbar-label">Undo diff</span>
          {undoFile && (
            <span className="diffbar-info">
              {undoFile.path.split("/").pop()} (+{undoFile.added}/−{undoFile.removed}) — hunk{" "}
              {Math.min(undoHunkIdx + 1, undoHunks.length)}/{undoHunks.length}
            </span>
          )}
          <button onClick={() => gotoUndoHunk(undoHunkIdx - 1)} title="Previous hunk">◀</button>
          <button onClick={() => gotoUndoHunk(undoHunkIdx + 1)} title="Next hunk">▶</button>
          {undoFiles.length > 1 && (
            <>
              <span className="diffbar-sep" />
              <button onClick={() => gotoUndoFile(-1)} title="Previous file in this diff">⏮</button>
              <span className="diffbar-info">
                file {undoFileIdx + 1}/{undoFiles.length}
              </span>
              <button onClick={() => gotoUndoFile(1)} title="Next file in this diff">⏭</button>
            </>
          )}
          <span className="diffbar-sep" />
          <button className="diffbar-accept" onClick={jumpToNode} title="Jump the buffer to this node">
            Jump here
          </button>
          <button onClick={closeUndoPin} title="Unpin (Esc)">✕</button>
        </div>
      )}
    </>
  );
}
