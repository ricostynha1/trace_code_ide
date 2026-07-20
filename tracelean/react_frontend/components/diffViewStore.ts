import type { NodeDiffT } from "./Editor";

/** Diff-view state that must survive Editor/EditorDiffBar remounts.
 *
 * App.tsx keys `<Editor>` on the current file, so every "next/previous file
 * with diffs" navigation unmounts and remounts the whole editor subtree. All
 * diff-view state used to live in EditorDiffBar's useState and was wiped on
 * every remount: the AI pending bar flickered empty until its re-fetch came
 * back, and the pinned undo diff (populated only by a fire-and-forget
 * `undo-hover-diff` CustomEvent) never came back at all.
 *
 * This module is the durable home for that state. It listens to the same
 * window events the components do, so it stays correct even while no editor
 * is mounted; components initialize from it on mount and write navigation
 * cursors (file/hunk index) back into it.
 */

export interface UndoPinState {
  nodeId: string;
  diff: NodeDiffT;
}

interface DiffViewCache {
  /** Currently pinned undo-node diff, or null when unpinned. */
  undoPin: UndoPinState | null;
  /** File/hunk navigation cursors within the pinned diff. */
  undoFileIdx: number;
  undoHunkIdx: number;
  /** Last broadcast AI pending-edit diff (nodeId "ai-pending"), or null when
   * no diffs are pending. Editors repaint from this after a remount (bugs.md
   * Bug 1: switching file lost the AI additions/deletions view). */
  aiPending: NodeDiffT | null;
}

export const diffViewCache: DiffViewCache = {
  undoPin: null,
  undoFileIdx: 0,
  undoHunkIdx: 0,
  aiPending: null,
};

// Mirror the pin lifecycle exactly as EditorDiffBar interprets it: a null /
// diff-less detail unpins, `pinned: true` pins, plain hover events are
// ignored. AI pending broadcasts (nodeId "ai-pending") only update aiPending —
// they never touch the pin. Registered at module scope so unpinning (Esc in
// the undo tree, etc.) is recorded even when no editor is mounted.
window.addEventListener("undo-hover-diff", (e: Event) => {
  const detail = (e as CustomEvent).detail;
  if (detail && detail.nodeId === "ai-pending") {
    diffViewCache.aiPending = (detail.nodeDiff as NodeDiffT) ?? null;
    return;
  }
  if (!detail || !detail.nodeDiff) {
    diffViewCache.undoPin = null;
    return;
  }
  if (detail.pinned && detail.nodeId) {
    diffViewCache.undoPin = { nodeId: detail.nodeId, diff: detail.nodeDiff as NodeDiffT };
    diffViewCache.undoFileIdx = 0;
    diffViewCache.undoHunkIdx = 0;
  }
});
window.addEventListener("undo-unpin", () => {
  diffViewCache.undoPin = null;
});
