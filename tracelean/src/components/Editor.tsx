import { useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { EditorState } from "@codemirror/state";
import { EditorView, keymap, lineNumbers, highlightActiveLine, hoverTooltip, Tooltip } from "@codemirror/view";
import { defaultKeymap } from "@codemirror/commands";
import { oneDark } from "@codemirror/theme-one-dark";
import { javascript } from "@codemirror/lang-javascript";
import { python } from "@codemirror/lang-python";
import { rust } from "@codemirror/lang-rust";
import { cpp } from "@codemirror/lang-cpp";
import { markdown } from "@codemirror/lang-markdown";
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

function getLanguageExtension(path: string) {
  const ext = path.split(".").pop()?.toLowerCase();
  switch (ext) {
    case "js":
    case "jsx":
    case "ts":
    case "tsx":
      return javascript({ jsx: true, typescript: ext.includes("t") });
    case "py":
      return python();
    case "rs":
      return rust();
    case "c":
    case "cpp":
    case "cc":
    case "h":
    case "hpp":
      return cpp();
    case "md":
      return markdown();
    case "lean":
      // No dedicated Lean CM extension yet — use plain text with custom highlighting
      return [];
    default:
      return [];
  }
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

/** Hover tooltip that shows tree-sitter symbol info */
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

  const mode = getEditorMode(filePath);

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

        const langExt = getLanguageExtension(filePath);

        const state = EditorState.create({
          doc: content,
          extensions: [
            lineNumbers(),
            highlightActiveLine(),
            highlightSelectionMatches(),
            oneDark,
            ...(Array.isArray(langExt) ? langExt : [langExt]),
            ...(mode === "code" ? [symbolHoverTooltip(filePath)] : []),
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
      } catch (e) {
        console.error("Failed to open file:", e);
        setStatus(`Error: ${e}`);
      }
    };

    initEditor();

    return () => {
      destroyed = true;
      if (viewRef.current) {
        viewRef.current.destroy();
        viewRef.current = null;
      }
    };
  }, [filePath]);

  const sendChangesAsCommands = async (update: any) => {
    update.changes.iterChanges(
      async (fromA: number, toA: number, _fromB: number, _toB: number, inserted: any) => {
        const insertedText = inserted.toString();
        const deletedLen = toA - fromA;

        try {
          if (deletedLen > 0 && insertedText.length > 0) {
            const oldText = update.startState.doc.sliceString(fromA, toA);
            await invoke("apply_command", {
              command: {
                Replace: {
                  file: filePath,
                  offset: fromA,
                  old_text: oldText,
                  new_text: insertedText,
                },
              },
            });
          } else if (deletedLen > 0) {
            const deletedText = update.startState.doc.sliceString(fromA, toA);
            await invoke("apply_command", {
              command: {
                Delete: {
                  file: filePath,
                  offset: fromA,
                  len: deletedLen,
                  deleted_text: deletedText,
                },
              },
            });
          } else if (insertedText.length > 0) {
            await invoke("apply_command", {
              command: {
                Insert: {
                  file: filePath,
                  offset: fromA,
                  text: insertedText,
                },
              },
            });
          }
        } catch (e) {
          console.error("Failed to send command:", e);
        }
      }
    );
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

      // If Lean file, type-check on save
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
    // This triggers file open in parent — we'll use a custom event or prop
    // For now, set window location hash as a signal
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
