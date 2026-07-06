import { useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { EditorState, StateField, StateEffect, RangeSet } from "@codemirror/state";
import { EditorView, keymap, lineNumbers, highlightActiveLine, hoverTooltip, Tooltip, Decoration, DecorationSet } from "@codemirror/view";
import { defaultKeymap } from "@codemirror/commands";
import { oneDark } from "@codemirror/theme-one-dark";
import { searchKeymap, highlightSelectionMatches } from "@codemirror/search";

interface EditorProps {
  filePath: string;
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

export function Editor({ filePath }: EditorProps) {
  const editorRef = useRef<HTMLDivElement>(null);
  const viewRef = useRef<EditorView | null>(null);
  const [status, setStatus] = useState("");
  const [leanDiagnostics, setLeanDiagnostics] = useState<LeanDiagnostic[]>([]);
  const [traceLink, setTraceLink] = useState<string | null>(null);
  const syncingFromBackend = useRef(false);
  const highlightTimer = useRef<ReturnType<typeof setTimeout> | null>(null);

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
            ]),
            EditorView.updateListener.of((update) => {
              if (update.docChanged && !syncingFromBackend.current) {
                sendChangesAsCommands(update);
                scheduleHighlights(update.view);
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

  const sendChangesAsCommands = async (update: any) => {
    // Collect all changes and send them — avoids sequential IPC round-trips
    const commands: any[] = [];
    update.changes.iterChanges(
      (fromA: number, toA: number, _fromB: number, _toB: number, inserted: any) => {
        const insertedText = inserted.toString();
        const deletedLen = toA - fromA;

        if (deletedLen > 0 && insertedText.length > 0) {
          const oldText = update.startState.doc.sliceString(fromA, toA);
          commands.push({
            Replace: {
              file: filePath,
              offset: fromA,
              old_text: oldText,
              new_text: insertedText,
            },
          });
        } else if (deletedLen > 0) {
          const deletedText = update.startState.doc.sliceString(fromA, toA);
          commands.push({
            Delete: {
              file: filePath,
              offset: fromA,
              len: deletedLen,
              deleted_text: deletedText,
            },
          });
        } else if (insertedText.length > 0) {
          commands.push({
            Insert: {
              file: filePath,
              offset: fromA,
              text: insertedText,
            },
          });
        }
      }
    );

    // Send as batch if multiple, else single
    if (commands.length === 0) return;
    try {
      if (commands.length === 1) {
        await invoke("apply_command", { command: commands[0] });
      } else {
        await invoke("apply_command", { command: { Batch: { commands } } });
      }
    } catch (e) {
      console.error("Failed to send command:", e);
    }
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

  const handleUndo = async () => {
    try {
      const success = await invoke<boolean>("undo");
      if (success) {
        await syncFromBackend();
      }
    } catch (e) {
      console.error("Undo failed:", e);
    }
  };

  const handleRedo = async () => {
    try {
      const success = await invoke<boolean>("redo");
      if (success) {
        await syncFromBackend();
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

  return (
    <div className="editor-container">
      <div className="editor-tab">
        <span className="editor-mode-badge" style={{ color: modeColor(mode) }}>
          {modeLabel(mode)}
        </span>
        <span className="editor-filename">{filePath.split("/").pop()}</span>
        {traceLink && (
          <button className="trace-link-btn" onClick={handleNavigate} title={`Go to ${traceLink}`}>
            {mode === "lean" ? "← Req" : mode === "requirement" ? "Spec →" : ""}
          </button>
        )}
      </div>
      <div className="editor-content" ref={editorRef} />
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
