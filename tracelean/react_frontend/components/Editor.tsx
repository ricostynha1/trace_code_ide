import { useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { EditorState, StateField, StateEffect, RangeSet } from "@codemirror/state";
import { EditorView, keymap, lineNumbers, highlightActiveLine, hoverTooltip, Tooltip, Decoration, DecorationSet, WidgetType, gutter, GutterMarker } from "@codemirror/view";
import { defaultKeymap } from "@codemirror/commands";
import { oneDark } from "@codemirror/theme-one-dark";
import { searchKeymap, highlightSelectionMatches } from "@codemirror/search";
import { mythKeyName, type KeyBindingInfo } from "./mythKeys";
import { EditorDiffBar } from "./EditorDiffBar";
import { diffViewCache } from "./diffViewStore";

interface EditorProps {
  filePath: string;
  /** 1-based line to scroll to once the file is loaded (diff navigation). */
  initialLine?: number | null;
}

interface SymbolInfo {
  name: string;
  kind: string;
  file: string;
  start_line: number;
  end_line: number;
  start_col: number;
}

interface HighlightSpan {
  from: number;
  to: number;
  color: string;
}

interface LeanCheckResult {
  success: boolean;
  errors: LeanDiagnostic[];
  warnings: LeanDiagnostic[];
}

interface LeanDiagnostic {
  file: string;
  line: number;
  col: number;
  message: string;
  severity: string;
}

type EditorMode = "code" | "lean" | "requirement";

// Myth (docs/myth_fable.md): the node under a position + the actions its
// captures carry, from the core binding map.

/** `file:///a/b.rs` → `/a/b.rs`, with percent-escapes decoded. */
function uriToPath(uri: string): string {
  const rest = uri.startsWith("file://") ? uri.slice("file://".length) : uri;
  try {
    return decodeURIComponent(rest);
  } catch {
    return rest;
  }
}

/**
 * Render one LSP answer as text. Deliberately lossy: the popup exists to answer
 * a question at the cursor, and dumping the raw protocol payload would bury the
 * answer in `range` objects.
 */
function summarizeLsp(mode: string, value: any): string {
  if (value == null) return "no answer";
  if (mode === "hover") {
    const c = value.contents ?? value;
    if (typeof c === "string") return c;
    if (Array.isArray(c)) return c.map((x) => (typeof x === "string" ? x : x?.value ?? "")).join("\n");
    return c?.value ?? JSON.stringify(value, null, 2);
  }
  if (mode === "goal") {
    const goals = value.goals ?? value.rendered ?? value;
    if (Array.isArray(goals)) return goals.length === 0 ? "no goals" : goals.join("\n\n");
    return typeof goals === "string" ? goals : JSON.stringify(value, null, 2);
  }
  if (mode === "references" || mode === "symbols" || mode === "diagnostics") {
    const list: any[] = Array.isArray(value) ? value : (value?.diagnostics ?? []);
    if (list.length === 0) return "none";
    return list
      .slice(0, 50)
      .map((x) => {
        const line = x.line ?? x.range?.start?.line ?? x.location?.range?.start?.line ?? 0;
        const where = x.uri ? uriToPath(x.uri).split("/").pop() : (x.file ?? "");
        const what = x.message ?? x.name ?? "";
        return `${where}:${line + 1}  ${what}`.trim();
      })
      .join("\n");
  }
  return JSON.stringify(value, null, 2);
}

interface MythNodeInfo {
  captures: string[];
  kind: string;
  from: number;
  to: number;
  text: string;
}

/** One offered action (core `myth::provider::Action`). */
interface MythAction {
  name: string;
  title: string;
  group: string;
  priority: number;
  args?: unknown;
  provider: string;
}

interface MythContextMenu {
  x: number;
  y: number;
  /** Groups in core's display order: [group, actions]. */
  groups: Array<[string, MythAction[]]>;
  charPos: number;
  node: MythNodeInfo | null;
}

// --- Backend command protocol (P34) ---
// The backend's only text primitive is Replace { file, at, old, new } where
// `at` is a Unicode code-point (char) index — NOT a UTF-16 offset. CodeMirror
// works in UTF-16, so we convert at the boundary.

interface CursorHint {
  file: string;
  char_pos: number;
}

interface ApplyResult {
  revision: number;
  file: string | null;
  content_hash: number | null;
  cursor: CursorHint | null;
}

interface EditOutcome {
  changed: boolean;
  cursor: CursorHint | null;
}

/** Number of Unicode code points in a JS (UTF-16) string. */
function countCodePoints(s: string): number {
  const pairs = s.match(/[\uD800-\uDBFF][\uDC00-\uDFFF]/g);
  return s.length - (pairs ? pairs.length : 0);
}

/** UTF-16 offset corresponding to a code-point index. */
function utf16OffsetOfCharIndex(s: string, charIdx: number): number {
  let count = 0;
  let i = 0;
  while (i < s.length && count < charIdx) {
    const cp = s.codePointAt(i)!;
    i += cp > 0xffff ? 2 : 1;
    count++;
  }
  return i;
}

/** FNV-1a 32-bit over UTF-8 bytes — must mirror the backend implementation. */
function fnv1a32(s: string): number {
  const bytes = new TextEncoder().encode(s);
  let h = 0x811c9dc5;
  for (let i = 0; i < bytes.length; i++) {
    h ^= bytes[i];
    h = Math.imul(h, 0x01000193);
  }
  return h >>> 0;
}

function getEditorMode(path: string): EditorMode {
  if (path.endsWith(".lean")) return "lean";
  if (path.startsWith("reqs/") && path.endsWith(".md")) return "requirement";
  return "code";
}

function modeLabel(mode: EditorMode): string {
  switch (mode) {
    case "lean": return "Lean Spec";
    case "requirement": return "Requirement";
    case "code": return "Code";
  }
}

function modeColor(mode: EditorMode): string {
  switch (mode) {
    case "lean": return "#c678dd";
    case "requirement": return "#d19a66";
    case "code": return "#61afef";
  }
}

// --- Tree-sitter highlight decorations ---

const setHighlights = StateEffect.define<DecorationSet>();

const highlightField = StateField.define<DecorationSet>({
  create() { return Decoration.none; },
  update(value, tr) {
    for (const e of tr.effects) {
      if (e.is(setHighlights)) return e.value;
    }
    if (tr.docChanged) return value.map(tr.changes);
    return value;
  },
  provide: (f) => EditorView.decorations.from(f),
});


// --- Traceability gutter (T1/T4) ---
//
// A chip beside every line an annotation anchors to, so the link between a
// requirement and the code is visible *in the code* rather than only in a
// panel. The role decides the glyph and colour; the tooltip names the
// requirement, which is what you actually want to read at a glance.

interface TraceLink {
  role: string;
  req_id: string;
  clause: string | null;
  line: number;
  anchor: { start_line: number; end_line: number };
}

const ROLE_CHIP: Record<string, { glyph: string; color: string; label: string }> = {
  models: { glyph: "M", color: "#c678dd", label: "Lean model" },
  implements: { glyph: "I", color: "#61afef", label: "implementation" },
  tests: { glyph: "T", color: "#98c379", label: "test" },
  drt: { glyph: "D", color: "#e5c07b", label: "differential-test harness" },
  proves: { glyph: "P", color: "#56b6c2", label: "proof" },
};

class TraceChip extends GutterMarker {
  constructor(private readonly links: TraceLink[]) {
    super();
  }

  eq(other: TraceChip) {
    return (
      other.links.length === this.links.length &&
      other.links.every((l, i) => l.req_id === this.links[i].req_id && l.role === this.links[i].role)
    );
  }

  toDOM() {
    const span = document.createElement("span");
    span.className = "trace-gutter-chip";
    const first = ROLE_CHIP[this.links[0].role] ?? { glyph: "?", color: "#7f848e", label: this.links[0].role };
    span.textContent = this.links.length > 1 ? String(this.links.length) : first.glyph;
    span.style.color = first.color;
    span.title = this.links
      .map((l) => {
        const chip = ROLE_CHIP[l.role] ?? { label: l.role };
        return `${chip.label} of ${l.req_id}${l.clause ? "." + l.clause : ""}`;
      })
      .join("\n");
    return span;
  }
}

const setTraceLinks = StateEffect.define<TraceLink[]>();

const traceLinkField = StateField.define<TraceLink[]>({
  create() {
    return [];
  },
  update(value, tr) {
    for (const e of tr.effects) {
      if (e.is(setTraceLinks)) return e.value;
    }
    // The index is rebuilt from disk, so an unsaved edit invalidates the line
    // numbers rather than shifting them: drop the chips instead of drawing
    // them next to the wrong lines.
    if (tr.docChanged) return [];
    return value;
  },
});

const traceGutter = gutter({
  class: "trace-gutter",
  lineMarker(view, line) {
    const links = view.state.field(traceLinkField);
    if (links.length === 0) return null;
    const lineNo = view.state.doc.lineAt(line.from).number - 1;
    const here = links.filter((l) => l.line === lineNo);
    return here.length > 0 ? new TraceChip(here) : null;
  },
  initialSpacer: () => new TraceChip([{ role: "implements", req_id: "", clause: null, line: 0, anchor: { start_line: 0, end_line: 0 } }]),
  domEventHandlers: {
    // Clicking a chip opens the requirement it names in the trace panel.
    mousedown(view, line) {
      const lineNo = view.state.doc.lineAt(line.from).number - 1;
      const here = view.state.field(traceLinkField).filter((l) => l.line === lineNo);
      if (here.length === 0) return false;
      window.dispatchEvent(
        new CustomEvent("tracelean-trace-action", {
          detail: { kind: "show_evidence", req_id: here[0].req_id, clause: here[0].clause },
        })
      );
      return true;
    },
  },
});

// --- T0: Diff overlay decorations for undo tree hover ---

const setDiffDecorations = StateEffect.define<DecorationSet>();

const diffField = StateField.define<DecorationSet>({
  create() { return Decoration.none; },
  update(value, tr) {
    for (const e of tr.effects) {
      if (e.is(setDiffDecorations)) return e.value;
    }
    if (tr.docChanged) return Decoration.none;
    return value;
  },
  provide: (f) => EditorView.decorations.from(f),
});

const diffRemovedLine = Decoration.line({ attributes: { style: "background-color: rgba(200, 50, 50, 0.15); border-left: 3px solid #e06c75;" } });
// @ts-expect-error kept for future use
const _diffContextLine = Decoration.line({ attributes: { style: "opacity: 0.6;" } });

// Cache: color hex → Decoration with inline style
const decoCache: Map<string, Decoration> = new Map();

function getDecoration(color: string): Decoration {
  let deco = decoCache.get(color);
  if (!deco) {
    deco = Decoration.mark({ attributes: { style: `color: ${color}` } });
    decoCache.set(color, deco);
  }
  return deco;
}

function buildDecorations(spans: HighlightSpan[], docLen: number): DecorationSet {
  const builder: { from: number; to: number; value: Decoration }[] = [];
  for (const span of spans) {
    if (span.from >= span.to || span.to > docLen) continue;
    builder.push({ from: span.from, to: span.to, value: getDecoration(span.color) });
  }
  builder.sort((a, b) => a.from - b.from || a.to - b.to);
  return RangeSet.of(builder);
}

// --- P5: structured hover diff (NodeDiff from get_undo_node_diff_structured) ---

interface DiffHunkT {
  current_start_line: number;
  removed_lines: string[];
  added_lines: string[];
}

interface FileDiffT {
  path: string;
  added: number;
  removed: number;
  hunks: DiffHunkT[];
}

export interface NodeDiffT {
  files: FileDiffT[];
}

/** Green block widget showing the lines a jump would insert (D5.2). */
class AddedLinesWidget extends WidgetType {
  constructor(readonly lines: string[]) {
    super();
  }
  eq(other: AddedLinesWidget): boolean {
    return other.lines.length === this.lines.length
      && other.lines.every((l, i) => l === this.lines[i]);
  }
  toDOM(): HTMLElement {
    const wrap = document.createElement("div");
    wrap.className = "cm-diff-added-block";
    for (const line of this.lines) {
      const el = document.createElement("div");
      el.className = "cm-diff-added-line";
      el.textContent = line.length > 0 ? line : " ";
      wrap.appendChild(el);
    }
    return wrap;
  }
  get estimatedHeight(): number {
    return this.lines.length * 18;
  }
}

function matchesFile(diffPath: string, currentFile: string): boolean {
  const normDiff = diffPath.replace(/^\/+/, "");
  const normFile = currentFile.replace(/^\/+/, "");
  return normDiff === normFile || normFile.endsWith(normDiff) || normDiff.endsWith(normFile);
}

/** Build decorations for the currently open file from a structured NodeDiff:
 * red line styles on lines that would be removed, green block widgets at
 * insertion points showing the incoming text. */
function buildNodeDiffDecorations(nodeDiff: NodeDiffT, currentFile: string, view: EditorView): DecorationSet {
  const fileDiff = nodeDiff.files.find((f) => matchesFile(f.path, currentFile));
  if (!fileDiff) return Decoration.none;

  const doc = view.state.doc;
  const decos: { from: number; to: number; value: Decoration }[] = [];

  for (const hunk of fileDiff.hunks) {
    for (let i = 0; i < hunk.removed_lines.length; i++) {
      const lineNum = hunk.current_start_line + i;
      if (lineNum >= 1 && lineNum <= doc.lines) {
        const lineObj = doc.line(lineNum);
        decos.push({ from: lineObj.from, to: lineObj.from, value: diffRemovedLine });
      }
    }
    if (hunk.added_lines.length > 0) {
      // Incoming lines replace the removed run — show them right after it.
      const afterLine = hunk.current_start_line + hunk.removed_lines.length;
      const pos = afterLine <= doc.lines ? doc.line(afterLine).from : doc.length;
      decos.push({
        from: pos,
        to: pos,
        value: Decoration.widget({
          widget: new AddedLinesWidget(hunk.added_lines),
          block: true,
          side: afterLine <= doc.lines ? -1 : 1,
        }),
      });
    }
  }

  if (decos.length === 0) return Decoration.none;
  decos.sort((a, b) => a.from - b.from);
  return Decoration.set(decos);
}

// --- Symbol hover tooltip ---

function symbolHoverTooltip(filePath: string) {
  return hoverTooltip(async (view, pos): Promise<Tooltip | null> => {
    const line = view.state.doc.lineAt(pos);
    const lineNum = line.number - 1;

    try {
      const symbols = await invoke<SymbolInfo[]>("get_file_symbols", { path: filePath });
      const symbol = symbols.find(
        (s) => lineNum >= s.start_line && lineNum <= s.end_line
      );

      if (!symbol) return null;

      const wordAt = view.state.wordAt(pos);
      if (!wordAt) return null;
      const word = view.state.doc.sliceString(wordAt.from, wordAt.to);

      if (word !== symbol.name && lineNum !== symbol.start_line) return null;

      return {
        pos: wordAt.from,
        end: wordAt.to,
        above: true,
        create() {
          const dom = document.createElement("div");
          dom.className = "symbol-tooltip";
          dom.textContent = `${symbol.kind}: ${symbol.name} (lines ${symbol.start_line + 1}–${symbol.end_line + 1})`;
          return { dom };
        },
      };
    } catch {
      return null;
    }
  }, { hoverTime: 300 });
}

export function Editor({ filePath, initialLine }: EditorProps) {
  const editorRef = useRef<HTMLDivElement>(null);
  const viewRef = useRef<EditorView | null>(null);
  const [status, setStatus] = useState("");
  const [leanDiagnostics, setLeanDiagnostics] = useState<LeanDiagnostic[]>([]);
  const [traceLink, setTraceLink] = useState<string | null>(null);
  const [ctxMenu, setCtxMenu] = useState<MythContextMenu | null>(null);
  const [mythMode, setMythMode] = useState("Main");
  // Entries of a dynamic mode (CodeActions): computed at the cursor by the
  // providers in core, not looked up in keymap.json.
  const dynamicRef = useRef<KeyBindingInfo[]>([]);
  const [lspResult, setLspResult] = useState<{ title: string; body: string } | null>(null);
  // Read by the DOM capture handler (state would be stale inside CodeMirror callbacks)
  const mythModeRef = useRef("Main");
  const syncingFromBackend = useRef(false);
  const highlightTimer = useRef<ReturnType<typeof setTimeout> | null>(null);
  const sendQueue = useRef<Promise<void>>(Promise.resolve());

  const mode = getEditorMode(filePath);

  // Fetch highlights from backend tree-sitter and apply as decorations
  const applyHighlights = async (view: EditorView) => {
    try {
      const spans = await invoke<HighlightSpan[]>("get_highlights", { path: filePath });
      const decos = buildDecorations(spans, view.state.doc.length);
      view.dispatch({ effects: setHighlights.of(decos) });

      // DEV feature-flag: log legacy vs query-based for comparison
      if (import.meta.env.DEV) {
        try {
          const legacySpans = await invoke<HighlightSpan[]>("get_highlights_legacy", { path: filePath });
          const diff = spans.length - legacySpans.length;
          if (diff !== 0) {
            console.debug(`[highlight-compare] query=${spans.length} legacy=${legacySpans.length} diff=${diff} file=${filePath}`);
          }
        } catch { /* legacy compare optional */ }
      }
    } catch (e) {
      console.error("Highlight fetch failed:", e);
    }
  };

  // Debounced highlight refresh (after edits)
  const scheduleHighlights = (view: EditorView) => {
    if (highlightTimer.current) clearTimeout(highlightTimer.current);
    highlightTimer.current = setTimeout(() => applyHighlights(view), 150);
  };

  // Check for trace navigation link
  useEffect(() => {
    const checkLink = async () => {
      try {
        const target = await invoke<string | null>("navigate_trace_link", {
          fromPath: filePath,
        });
        setTraceLink(target);
      } catch {
        setTraceLink(null);
      }
    };
    checkLink();
  }, [filePath]);

  // Always stay in sync with backend state — covers agent edits, undo, redo, etc.
  useEffect(() => {
    const unlisten = listen("undo-tree-changed", () => {
      syncFromBackend();
    });
    // Backend refused an edit (witness mismatch): hard resync.
    const unlistenIntegrity = listen("state-integrity-error", (event) => {
      console.warn("State integrity error:", event.payload);
      syncFromBackend();
    });
    return () => {
      unlisten.then((fn) => fn());
      unlistenIntegrity.then((fn) => fn());
    };
  }, [filePath]);

  // P5: Listen for undo tree hover diff — show structured diff decorations
  useEffect(() => {
    const handler = (e: Event) => {
      const view = viewRef.current;
      if (!view) return;
      const detail = (e as CustomEvent).detail;
      if (!detail || !detail.nodeDiff) {
        // Hover/pin/AI-pending cleared — fall back to whatever durable diff
        // still applies (pinned node diff, else AI pending edits) instead of
        // blanking; the module-scope diffViewStore listener has already
        // processed this same event, so the cache reflects the clear.
        const fallback = diffViewCache.undoPin?.diff ?? diffViewCache.aiPending;
        view.dispatch({
          effects: setDiffDecorations.of(
            fallback
              ? buildNodeDiffDecorations(fallback, filePath, view)
              : Decoration.none
          ),
        });
        return;
      }
      const decos = buildNodeDiffDecorations(detail.nodeDiff as NodeDiffT, filePath, view);
      view.dispatch({ effects: setDiffDecorations.of(decos) });
    };
    window.addEventListener("undo-hover-diff", handler);
    return () => window.removeEventListener("undo-hover-diff", handler);
  }, [filePath]);

  useEffect(() => {
    if (!editorRef.current) return;

    let destroyed = false;

    const initEditor = async () => {
      try {
        const content = await invoke<string>("open_file", { path: filePath });
        if (destroyed) return;

        if (viewRef.current) {
          viewRef.current.destroy();
        }

        const state = EditorState.create({
          doc: content,
          extensions: [
            lineNumbers(),
            highlightActiveLine(),
            highlightSelectionMatches(),
            oneDark,
            highlightField,
            diffField,
            traceLinkField,
            traceGutter,
            symbolHoverTooltip(filePath),
            keymap.of([
              ...defaultKeymap.filter(
                (k) => k.key !== "Mod-z" && k.key !== "Mod-y" && k.key !== "Mod-Shift-z"
              ),
              ...searchKeymap,
              {
                key: "Mod-z",
                run: () => { handleUndo(); return true; },
              },
              {
                key: "Mod-Shift-z",
                run: () => { handleRedo(); return true; },
              },
              {
                key: "Mod-y",
                run: () => { handleRedo(); return true; },
              },
              {
                key: "Mod-s",
                run: () => { handleSave(); return true; },
              },
              // Myth semantic navigation (keymap.json Main mode)
              { key: "Shift-ArrowUp", run: () => mythKey("S-ArrowUp") },
              { key: "Shift-ArrowDown", run: () => mythKey("S-ArrowDown") },
              { key: "Shift-ArrowLeft", run: () => mythKey("S-ArrowLeft") },
              { key: "Shift-ArrowRight", run: () => mythKey("S-ArrowRight") },
              // Mode-entry chords (C-. is the IME-safe alternate: Ctrl+Space
              // is often grabbed by the input method on Linux)
              { key: "Ctrl-Space", run: () => mythKey("C-Space") },
              { key: "Ctrl-.", run: () => mythKey("C-.") },
            ]),
            EditorView.updateListener.of((update) => {
              if (update.docChanged && !syncingFromBackend.current) {
                sendChangesAsCommands(update);
                scheduleHighlights(update.view);
              }
              // Anything that wants to follow the cursor subscribes to this
              // rather than each panel installing its own CodeMirror listener.
              // Debouncing is the subscriber's job: what "too often" means
              // depends on what the subscriber does with it.
              if (update.selectionSet || update.docChanged) {
                const head = update.state.selection.main.head;
                window.dispatchEvent(
                  new CustomEvent("tracelean-cursor", {
                    detail: {
                      file: filePath,
                      charPos: countCodePoints(update.state.doc.sliceString(0, head)),
                    },
                  })
                );
              }
            }),
          ],
        });

        const view = new EditorView({
          state,
          parent: editorRef.current!,
        });

        viewRef.current = view;
        setStatus(`${filePath}`);

        // Initial highlights
        applyHighlights(view);

        // A pinned undo diff or AI pending edits must survive the remount
        // file navigation causes: the `undo-hover-diff` event that painted
        // the decorations fired long ago, so re-derive this file's
        // decorations from the durable cache (bugs.md Bug 1).
        const cachedDiff = diffViewCache.undoPin?.diff ?? diffViewCache.aiPending;
        if (cachedDiff) {
          view.dispatch({
            effects: setDiffDecorations.of(
              buildNodeDiffDecorations(cachedDiff, filePath, view)
            ),
          });
        }

        // Diff navigation opened this file at a specific line (bug 0.7).
        if (initialLine && initialLine > 0) {
          const ln = Math.min(initialLine, view.state.doc.lines);
          view.dispatch({
            selection: { anchor: view.state.doc.line(ln).from },
            scrollIntoView: true,
          });
        }
      } catch (e) {
        console.error("Failed to open file:", e);
        setStatus(`Error: ${e}`);
      }
    };

    initEditor();

    return () => {
      destroyed = true;
      if (highlightTimer.current) clearTimeout(highlightTimer.current);
      if (viewRef.current) {
        viewRef.current.destroy();
        viewRef.current = null;
      }
    };
  }, [filePath]);

  const sendChangesAsCommands = (update: any) => {
    // Every edit is a Replace { at, old, new } with `at` in char (code point)
    // units against the pre-edit document. Positions from iterChanges are all
    // in startState coordinates, so we apply them back-to-front: edits later
    // in the document don't shift earlier positions.
    const commands: any[] = [];
    const startDoc = update.startState.doc;
    update.changes.iterChanges(
      (fromA: number, toA: number, _fromB: number, _toB: number, inserted: any) => {
        const oldText = startDoc.sliceString(fromA, toA);
        const newText = inserted.toString();
        if (oldText.length === 0 && newText.length === 0) return;
        commands.push({
          Replace: {
            file: filePath,
            at: countCodePoints(startDoc.sliceString(0, fromA)),
            old: oldText,
            new: newText,
          },
        });
      }
    );
    if (commands.length === 0) return;
    commands.reverse(); // back-to-front: keeps every `at` valid sequentially

    const command =
      commands.length === 1 ? commands[0] : { Batch: { commands } };
    // Hash of the doc as of this update — compared against the backend's hash
    // for the same command to detect divergence.
    const expectedHash = fnv1a32(update.state.doc.toString());

    // Serialize sends: a later keystroke must never overtake an earlier one.
    sendQueue.current = sendQueue.current.then(async () => {
      try {
        const result = await invoke<ApplyResult>("apply_command", { command });
        if (
          result.content_hash !== null &&
          result.file === filePath &&
          result.content_hash !== expectedHash
        ) {
          console.warn("Buffer divergence detected — resyncing from backend");
          await syncFromBackend();
        }
      } catch (e) {
        // Witness mismatch: the backend refused the edit. Resync to its state.
        console.error("Command rejected, resyncing:", e);
        await syncFromBackend();
      }
    });
  };

  const syncFromBackend = async () => {
    const content = await invoke<string>("get_file_content", { path: filePath });
    if (content !== null && viewRef.current) {
      const view = viewRef.current;
      const currentContent = view.state.doc.toString();
      if (content !== currentContent) {
        syncingFromBackend.current = true;
        view.dispatch({
          changes: {
            from: 0,
            to: view.state.doc.length,
            insert: content,
          },
        });
        syncingFromBackend.current = false;
        applyHighlights(view);
      }
    }
  };

  // Scroll the editor to a 1-based line (diff bar navigation, bug 0.7).
  // Traceability chips for this file. Reloaded when the file changes and when
  // anything asks the trace layer to rescan, because an annotation added in
  // another buffer changes what this one is linked to.
  useEffect(() => {
    let cancelled = false;
    const load = () => {
      invoke<TraceLink[]>("trace_links_in_file", { path: filePath })
        .then((links) => {
          const view = viewRef.current;
          if (cancelled || !view) return;
          view.dispatch({ effects: setTraceLinks.of(links ?? []) });
        })
        // No project open, or no annotations: chips simply do not appear.
        .catch(() => {});
    };
    load();
    window.addEventListener("tracelean-trace-refresh", load);
    return () => {
      cancelled = true;
      window.removeEventListener("tracelean-trace-refresh", load);
    };
  }, [filePath]);

  // The component only remounts when the *file* changes, so a second jump into
  // the file already open (two annotations in one file) needs its own effect.
  useEffect(() => {
    if (initialLine && initialLine > 0) gotoLine(initialLine);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [initialLine]);

  const gotoLine = (line: number) => {
    const view = viewRef.current;
    if (!view) return;
    const ln = Math.max(1, Math.min(line, view.state.doc.lines));
    view.dispatch({
      selection: { anchor: view.state.doc.line(ln).from },
      scrollIntoView: true,
    });
    view.focus();
  };

  // Place the CodeMirror cursor from a backend char-index hint and scroll it
  // into view (used after undo/redo/jump).
  const applyCursorHint = (cursor: CursorHint | null) => {
    const view = viewRef.current;
    if (!view || !cursor || cursor.file !== filePath) return;
    const docStr = view.state.doc.toString();
    const pos = Math.min(
      utf16OffsetOfCharIndex(docStr, cursor.char_pos),
      view.state.doc.length
    );
    view.dispatch({
      selection: { anchor: pos },
      scrollIntoView: true,
    });
    view.focus();
  };

  const handleUndo = async () => {
    // Item 6: Ctrl+Z is scoped to the active file — global undo is only
    // reachable via clicking a node in the undo tree panel.
    try {
      const outcome = await invoke<EditOutcome>("undo_file", { path: filePath });
      if (outcome.changed) {
        await syncFromBackend();
        applyCursorHint(outcome.cursor);
      }
    } catch (e) {
      console.error("Undo failed:", e);
    }
  };

  const handleRedo = async () => {
    try {
      const outcome = await invoke<EditOutcome>("redo_file", { path: filePath });
      if (outcome.changed) {
        await syncFromBackend();
        applyCursorHint(outcome.cursor);
      }
    } catch (e) {
      console.error("Redo failed:", e);
    }
  };

  const handleSave = async () => {
    try {
      await invoke("save_file", { path: filePath });
      setStatus(`${filePath} — saved`);

      if (mode === "lean") {
        checkLean();
      }

      setTimeout(() => setStatus(filePath), 2000);
    } catch (e) {
      console.error("Save failed:", e);
      setStatus(`Error saving: ${e}`);
    }
  };

  const checkLean = async () => {
    try {
      setStatus(`${filePath} — checking...`);
      const result = await invoke<LeanCheckResult>("check_lean_spec", {
        path: filePath,
      });
      if (result.success) {
        setStatus(`${filePath} — ✓ type-checked`);
        setLeanDiagnostics(result.warnings);
      } else {
        setStatus(`${filePath} — ✗ errors`);
        setLeanDiagnostics([...result.errors, ...result.warnings]);
      }
    } catch (e) {
      setStatus(`${filePath} — lean not available`);
      setLeanDiagnostics([]);
    }
  };

  const handleNavigate = async () => {
    if (!traceLink) return;
    window.dispatchEvent(
      new CustomEvent("tracelean-navigate", { detail: { path: traceLink } })
    );
  };

  // ─── Myth: capture→action context menu + keymap-driven semantic nav ───────

  const handleContextMenu = async (e: React.MouseEvent) => {
    const view = viewRef.current;
    if (!view) return;
    e.preventDefault();
    const pos = view.posAtCoords({ x: e.clientX, y: e.clientY });
    if (pos == null) return;
    const charPos = countCodePoints(view.state.doc.sliceString(0, pos));
    // Open immediately with whatever needs no round trip, then fill in the LSP
    // half: a context menu that waits on rust-analyzer feels broken even when
    // it is only slow.
    try {
      const node = await invoke<MythNodeInfo | null>("myth_node_at", {
        file: filePath,
        charPos,
      }).catch(() => null);
      const res = await invoke<any>("myth_actions_at", {
        file: filePath,
        charPos,
        includeLsp: true,
      });
      setCtxMenu({
        x: e.clientX,
        y: e.clientY,
        groups: res?.groups ?? [],
        charPos,
        node,
      });
    } catch (err) {
      console.error("myth_actions_at failed:", err);
      setCtxMenu(null);
    }
  };

  const runMythAction = async (
    action: string,
    charPos: number,
    node: MythNodeInfo | null,
    args?: unknown
  ) => {
    setCtxMenu(null);
    try {
      const outcome = await invoke<any>("dispatch_action", {
        name: action,
        ctx: {
          surface: "editor",
          capture: node?.captures?.[node.captures.length - 1] ?? "",
          node_text: node?.text ?? "",
          file: filePath,
          char_pos: charPos,
          args: args ?? null,
        },
      });
      if (outcome?.kind !== "ui") return;
      const eff = outcome.effect;
      const view = viewRef.current;
      if (eff?.kind === "select" && view) {
        const doc = view.state.doc.toString();
        view.dispatch({
          selection: {
            anchor: Math.min(utf16OffsetOfCharIndex(doc, eff.from), view.state.doc.length),
            head: Math.min(utf16OffsetOfCharIndex(doc, eff.to), view.state.doc.length),
          },
          scrollIntoView: true,
        });
        view.focus();
      } else if (eff?.kind === "copy") {
        navigator.clipboard?.writeText(eff.text ?? "");
      } else if (eff?.kind === "undo") {
        handleUndo();
      } else if (eff?.kind === "redo") {
        handleRedo();
      } else if (eff?.kind === "save_file") {
        handleSave();
      } else if (eff?.kind === "lsp_query" && eff.mode === "goal") {
        // `v g` pins the infoview at this position rather than flashing a
        // popup: the point of a goal is to keep looking at it while you edit
        // the tactic meant to close it.
        window.dispatchEvent(
          new CustomEvent("tracelean-pin-goal", { detail: { charPos: eff.char_pos ?? charPos } })
        );
        window.dispatchEvent(new CustomEvent("tracelean-show-lean-panel"));
      } else if (eff?.kind === "lsp_query") {
        await runLspQuery(eff.mode, eff.char_pos ?? charPos);
      } else if (eff?.kind === "reveal") {
        window.dispatchEvent(
          new CustomEvent("tracelean-navigate", {
            detail: { path: eff.file, line: eff.line },
          })
        );
      } else if (eff?.kind === "no_target") {
        // A gap is an answer, not an error — say it plainly and stay put.
        setLspResult({ title: "Nothing to go to", body: String(eff.message ?? "") });
      } else if (eff?.kind === "goto_node") {
        window.dispatchEvent(
          new CustomEvent("tracelean-goto-node", { detail: eff.provenance })
        );
      } else if (eff?.kind === "step_diagnostic") {
        await stepDiagnostic(eff.direction ?? 1, charPos);
      } else if (
        eff?.kind === "run_drt" ||
        eff?.kind === "run_judge" ||
        eff?.kind === "replay_witness" ||
        eff?.kind === "show_evidence" ||
        eff?.kind === "explain_gap" ||
        eff?.kind === "coverage_map" ||
        eff?.kind === "zoom"
      ) {
        // Everything that belongs to the trace panel is routed there rather
        // than duplicated in the editor.
        window.dispatchEvent(new CustomEvent("tracelean-trace-action", { detail: eff }));
      } else if (eff?.kind === "annotate") {
        window.dispatchEvent(new CustomEvent("tracelean-annotate", { detail: eff }));
      } else if (eff?.kind === "lake_build") {
        window.dispatchEvent(new CustomEvent("tracelean-lake-build", { detail: eff }));
      }
    } catch (err) {
      console.error("dispatch_action failed:", err);
      setLspResult({ title: "Action failed", body: String(err) });
    }
  };

  /// Run one language-server query and show the answer.
  const runLspQuery = async (mode: string, charPos: number) => {
    setLspResult({ title: mode, body: "…" });
    try {
      const value = await invoke<any>("lsp_query", { file: filePath, charPos, mode });
      if (mode === "definition") {
        const target = Array.isArray(value) ? value[0] : value;
        const uri: string | undefined = target?.uri ?? target?.targetUri;
        const range = target?.range ?? target?.targetSelectionRange;
        if (uri) {
          setLspResult(null);
          window.dispatchEvent(
            new CustomEvent("tracelean-navigate", {
              detail: { path: uriToPath(uri), line: range?.start?.line ?? 0 },
            })
          );
          return;
        }
      }
      setLspResult({ title: mode, body: summarizeLsp(mode, value) });
    } catch (err) {
      // "rust-analyzer is not installed" and "no answer here" must not look
      // the same; the registry's error text says which.
      setLspResult({ title: mode, body: String(err) });
    }
  };

  const stepDiagnostic = async (direction: number, charPos: number) => {
    try {
      const value = await invoke<any>("lsp_query", {
        file: filePath,
        charPos,
        mode: "diagnostics",
      });
      const list: any[] = Array.isArray(value) ? value : (value?.diagnostics ?? []);
      if (list.length === 0) {
        setLspResult({ title: "diagnostics", body: "no problems reported in this file" });
        return;
      }
      const view = viewRef.current;
      if (!view) return;
      const current = view.state.doc.lineAt(view.state.selection.main.head).number - 1;
      const lines = list
        .map((d) => ({ d, line: d.line ?? d.range?.start?.line ?? 0 }))
        .sort((a, b) => a.line - b.line);
      const next =
        direction > 0
          ? (lines.find((x) => x.line > current) ?? lines[0])
          : ([...lines].reverse().find((x) => x.line < current) ?? lines[lines.length - 1]);
      gotoLine(next.line + 1);
      setLspResult({ title: `line ${next.line + 1}`, body: next.d.message ?? "" });
    } catch (err) {
      setLspResult({ title: "diagnostics", body: String(err) });
    }
  };

  // Route a key through the core keymap mode machine; on Dispatch, run the
  // action at the cursor. Keeps behavior editable via ui_settings/keymap.json.
  const mythKey = (keyName: string): boolean => {
    const view = viewRef.current;
    if (!view) return false;
    (async () => {
      const head = view.state.selection.main.head;
      const charPos = countCodePoints(view.state.doc.sliceString(0, head));

      // A dynamic mode's entries came from the providers, not from
      // keymap.json, so its keys are resolved here rather than in core. Escape
      // still goes through the mode machine so the stack stays honest.
      if (mythModeRef.current === "CodeActions" && keyName !== "Escape") {
        const hit = dynamicRef.current.find((b) => b.key === keyName);
        if (hit) {
          await invoke<any>("myth_key_event", { key: "Escape" }).catch(() => {});
          mythModeRef.current = "Verify";
          setMythMode("Verify");
          await runMythAction(hit.target, charPos, null, hit.args);
          return;
        }
      }

      try {
        const res = await invoke<any>("myth_key_event", { key: keyName });
        const state = res?.state ?? "Main";
        mythModeRef.current = state;
        setMythMode(state);

        if (state === "CodeActions") {
          // Ask every provider what is available right here, and show that as
          // the mode's which-key list.
          try {
            const at = await invoke<any>("myth_actions_at", {
              file: filePath,
              charPos,
              includeLsp: true,
            });
            dynamicRef.current = at?.bindings ?? [];
            const dropped: number = at?.dropped ?? 0;
            window.dispatchEvent(
              new CustomEvent("myth-mode", {
                detail: {
                  state,
                  path: res?.path ?? [state],
                  bindings: dynamicRef.current.length
                    ? dynamicRef.current
                    : [{ key: "Escape", target: "back", kind: "pop" }],
                  note: dropped > 0 ? `${dropped} more not shown` : "",
                },
              })
            );
          } catch (err) {
            dynamicRef.current = [];
            window.dispatchEvent(
              new CustomEvent("myth-mode", {
                detail: {
                  state,
                  path: res?.path ?? [state],
                  bindings: [{ key: "Escape", target: "back", kind: "pop" }],
                  note: String(err),
                },
              })
            );
          }
          return;
        }

        dynamicRef.current = [];
        // Which-key renders in the global bottom bar (Emacs-style)
        window.dispatchEvent(
          new CustomEvent("myth-mode", {
            detail: { state, path: res?.path ?? [state], bindings: res?.bindings ?? [] },
          })
        );
        if (res?.result?.kind === "dispatch") {
          await runMythAction(res.result.action, charPos, null);
        }
      } catch (err) {
        console.error("myth_key_event failed:", err);
      }
    })();
    return true;
  };

  // While a mode is active, every key belongs to the mode machine — intercept
  // before CodeMirror inserts it as text (capture phase).
  const handleModeKeyCapture = (e: React.KeyboardEvent) => {
    if (mythModeRef.current === "Main") return;
    const key = mythKeyName(e);
    if (!key) return;
    e.preventDefault();
    e.stopPropagation();
    mythKey(key);
  };

  return (
    <div className="editor-container">
      <div className="editor-tab">
        <span className="editor-mode-badge" style={{ color: modeColor(mode) }}>
          {modeLabel(mode)}
        </span>
        <span className="editor-filename">{filePath.split("/").pop()}</span>
        {mythMode !== "Main" && <span className="myth-mode-badge">{mythMode}</span>}
        {traceLink && (
          <button className="trace-link-btn" onClick={handleNavigate} title={`Go to ${traceLink}`}>
            {mode === "lean" ? "← Req" : mode === "requirement" ? "Spec →" : ""}
          </button>
        )}
      </div>
      <EditorDiffBar
        filePath={filePath}
        onGotoLine={gotoLine}
        onFileSync={async () => {
          await syncFromBackend();
        }}
      />
      <div
        className="editor-content"
        ref={editorRef}
        onContextMenu={handleContextMenu}
        onKeyDownCapture={handleModeKeyCapture}
      />
      {ctxMenu && (
        <div
          className="myth-menu"
          style={{ left: ctxMenu.x, top: ctxMenu.y }}
          onMouseLeave={() => setCtxMenu(null)}
        >
          {ctxMenu.node && (
            <div className="myth-menu-header">
              @{ctxMenu.node.captures[ctxMenu.node.captures.length - 1] ?? ctxMenu.node.kind}
            </div>
          )}
          {ctxMenu.groups.length === 0 && <div className="myth-menu-empty">no actions</div>}
          {ctxMenu.groups.map(([group, actions]) => (
            <div key={group} className="myth-menu-group">
              <div className="myth-menu-group-label">{group}</div>
              {actions.map((a, i) => (
                <div
                  key={`${a.name}-${i}`}
                  className="myth-menu-item"
                  onClick={() => runMythAction(a.name, ctxMenu.charPos, ctxMenu.node, a.args)}
                >
                  {a.title}
                </div>
              ))}
            </div>
          ))}
        </div>
      )}
      {lspResult && (
        <div className="lsp-popup" onClick={() => setLspResult(null)}>
          <div className="lsp-popup-title">{lspResult.title}</div>
          <pre className="lsp-popup-body">{lspResult.body}</pre>
        </div>
      )}
      {leanDiagnostics.length > 0 && (
        <div className="lean-diagnostics">
          {leanDiagnostics.map((d, i) => (
            <div key={i} className={`diagnostic ${d.severity}`}>
              <span className="diag-loc">:{d.line}:{d.col}</span>
              <span className="diag-msg">{d.message}</span>
            </div>
          ))}
        </div>
      )}
      <div className="status-bar">{status}</div>
    </div>
  );
}
