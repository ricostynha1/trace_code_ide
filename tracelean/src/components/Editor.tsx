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
    default:
      return [];
  }
}

/** Hover tooltip that shows tree-sitter symbol info */
function symbolHoverTooltip(filePath: string) {
  return hoverTooltip(async (view, pos): Promise<Tooltip | null> => {
    // Get the line number at cursor position
    const line = view.state.doc.lineAt(pos);
    const lineNum = line.number - 1; // 0-indexed

    try {
      const symbols = await invoke<SymbolInfo[]>("get_file_symbols", { path: filePath });
      // Find symbol that contains this line
      const symbol = symbols.find(
        (s) => lineNum >= s.start_line && lineNum <= s.end_line
      );

      if (!symbol) return null;

      // Get the word at position to check if it's the symbol name
      const wordAt = view.state.wordAt(pos);
      if (!wordAt) return null;
      const word = view.state.doc.sliceString(wordAt.from, wordAt.to);

      // Show tooltip if hovering the symbol name or if on definition line
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
  // Flag: when true, suppress sending changes to backend (we're syncing FROM backend)
  const syncingFromBackend = useRef(false);

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
            // NO CodeMirror history() — we use our own undo-tree
            oneDark,
            ...(Array.isArray(langExt) ? langExt : [langExt]),
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

  /** Sync editor content from backend state (suppresses command emission) */
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
      setTimeout(() => setStatus(filePath), 2000);
    } catch (e) {
      console.error("Save failed:", e);
      setStatus(`Error saving: ${e}`);
    }
  };

  return (
    <div className="editor-container">
      <div className="editor-tab">{filePath.split("/").pop()}</div>
      <div className="editor-content" ref={editorRef} />
      <div className="status-bar">{status}</div>
    </div>
  );
}
