import { useEffect, useState, useRef, useCallback } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";

interface FileEntry {
  name: string;
  path: string;
  is_dir: boolean;
}

interface FileTreeProps {
  projectRoot: string;
  onFileSelect: (path: string) => void;
  selectedFile: string | null;
}

/** Per-file +added/−removed badge from the undo-tree hover diff (P5, D5.3). */
interface HoverBadge {
  added: number;
  removed: number;
}

import { mythKeyName, type KeyBindingInfo } from "./mythKeys";

export function FileTree({ projectRoot, onFileSelect, selectedFile }: FileTreeProps) {
  const [entries, setEntries] = useState<FileEntry[]>([]);
  const [expanded, setExpanded] = useState<Set<string>>(new Set());
  const [hoverBadges, setHoverBadges] = useState<Map<string, HoverBadge>>(new Map());
  const [selIndex, setSelIndex] = useState(0);
  const [mythMode, setMythMode] = useState("Main");
  const [whichKey, setWhichKey] = useState<KeyBindingInfo[]>([]);
  const expandedRef = useRef<Set<string>>(expanded);

  // Keep ref in sync so event listeners can read current expanded set
  useEffect(() => {
    expandedRef.current = expanded;
  }, [expanded]);

  const loadDirectory = useCallback(async (path: string) => {
    try {
      const result = await invoke<FileEntry[]>("list_files", { path });
      if (path === "") {
        setEntries(result);
      } else {
        setEntries((prev) => {
          const filtered = prev.filter(
            (e) => !e.path.startsWith(path + "/") || e.path === path
          );
          return [...filtered, ...result];
        });
      }
    } catch (e) {
      console.error("Failed to list files:", e);
    }
  }, []);

  const refreshAll = useCallback(() => {
    loadDirectory("");
    expandedRef.current.forEach((dir) => loadDirectory(dir));
  }, [loadDirectory]);

  useEffect(() => {
    loadDirectory("");
  }, [projectRoot, loadDirectory]);

  // Refresh file tree when AI agent creates/deletes files or undo/redo
  useEffect(() => {
    const unlistenUndo = listen("undo-tree-changed", () => refreshAll());
    const unlistenFiles = listen("files-changed", () => refreshAll());
    return () => {
      unlistenUndo.then((fn) => fn());
      unlistenFiles.then((fn) => fn());
    };
  }, [refreshAll]);

  // P5 (D5.3): show +/− badges on files touched by the hovered undo node;
  // clearing hover clears the badges.
  useEffect(() => {
    const handler = (e: Event) => {
      const detail = (e as CustomEvent).detail;
      if (!detail || !detail.nodeDiff) {
        setHoverBadges(new Map());
        return;
      }
      const next = new Map<string, HoverBadge>();
      for (const f of detail.nodeDiff.files as Array<{ path: string; added: number; removed: number }>) {
        const norm = f.path.replace(/^\/+/, "");
        next.set(norm, { added: f.added, removed: f.removed });
      }
      setHoverBadges(next);
    };
    window.addEventListener("undo-hover-diff", handler);
    return () => window.removeEventListener("undo-hover-diff", handler);
  }, []);

  const toggleDir = (path: string) => {
    setExpanded((prev) => {
      const next = new Set(prev);
      if (next.has(path)) {
        next.delete(path);
      } else {
        next.add(path);
        loadDirectory(path);
      }
      return next;
    });
  };

  const handleClick = (entry: FileEntry) => {
    if (entry.is_dir) {
      toggleDir(entry.path);
    } else {
      onFileSelect(entry.path);
    }
  };

  // Build tree view from flat entries
  const rootEntries = entries.filter((e) => !e.path.includes("/"));
  const getChildren = (dirPath: string) =>
    entries.filter((e) => {
      const parent = e.path.substring(0, e.path.lastIndexOf("/"));
      return parent === dirPath;
    });

  // ─── Myth keyboard layer (docs/myth_fable.md) ─────────────────────────────
  // The tree is a surface: keys go through the core keymap mode machine
  // (myth_key_event); dispatched actions run through the action registry, so
  // file operations land in the undo tree like any other command.

  // Visible entries in render order — the keyboard selection model.
  const visible: FileEntry[] = [];
  {
    const collect = (entry: FileEntry) => {
      visible.push(entry);
      if (entry.is_dir && expanded.has(entry.path)) {
        getChildren(entry.path).forEach(collect);
      }
    };
    rootEntries.forEach(collect);
  }
  const selected = visible[Math.min(selIndex, Math.max(visible.length - 1, 0))] ?? null;

  const applyUiEffect = (effect: any) => {
    if (!effect) return;
    if (effect.kind === "open_file" && effect.path) onFileSelect(String(effect.path));
    else if (effect.kind === "undo") invoke("undo").catch(console.error);
    else if (effect.kind === "redo") invoke("redo").catch(console.error);
    else if (effect.kind === "copy") navigator.clipboard?.writeText(effect.text ?? "");
  };

  const runAction = async (action: string) => {
    const ctx: Record<string, unknown> = {
      surface: "file_tree",
      capture: selected ? (selected.is_dir ? "dir" : "file") : "",
      node_text: selected?.name ?? "",
      file: selected?.path ?? null,
    };
    if (action === "rename_file") {
      if (!selected) return;
      const newName = window.prompt(`Rename ${selected.name} to:`, selected.name);
      if (!newName || newName === selected.name) return;
      ctx.args = { new_name: newName };
    } else if (action === "create_file") {
      const dir = selected
        ? selected.is_dir
          ? selected.path
          : selected.path.substring(0, selected.path.lastIndexOf("/"))
        : "";
      const path = window.prompt("New file path:", dir ? dir + "/" : "");
      if (!path) return;
      ctx.file = null;
      ctx.args = { path };
    } else if (action === "delete_file") {
      if (!selected || !window.confirm(`Delete ${selected.path}?`)) return;
    } else if (action === "open_file" && selected?.is_dir) {
      toggleDir(selected.path);
      return;
    }
    try {
      const outcome = await invoke<any>("dispatch_action", { name: action, ctx });
      if (outcome?.kind === "ui") applyUiEffect(outcome.effect);
    } catch (e) {
      console.error(`action ${action} failed:`, e);
    }
  };

  const handleKeyDown = async (e: React.KeyboardEvent) => {
    // Plain list navigation stays native (keymap Main falls through anyway).
    if (!e.ctrlKey && !e.altKey && !e.shiftKey) {
      if (e.key === "ArrowDown") {
        e.preventDefault();
        setSelIndex((i) => Math.min(i + 1, Math.max(visible.length - 1, 0)));
        return;
      }
      if (e.key === "ArrowUp") {
        e.preventDefault();
        setSelIndex((i) => Math.max(i - 1, 0));
        return;
      }
      if (e.key === "Enter") {
        e.preventDefault();
        if (selected) handleClick(selected);
        return;
      }
    }
    const key = mythKeyName(e);
    if (!key) return;
    // preventDefault must be synchronous: consume everything while a mode is
    // active, and the mode-entry chords themselves while in Main.
    // (C-. is the IME-safe alternate — Ctrl+Space is often grabbed on Linux.)
    if (mythMode !== "Main" || key === "C-Space" || key === "C-.") {
      e.preventDefault();
    }
    try {
      const res = await invoke<{
        result: { kind: string; action?: string; state?: string };
        state: string;
        bindings: KeyBindingInfo[];
      }>("myth_key_event", { key });
      setMythMode(res.state);
      setWhichKey(res.state !== "Main" ? res.bindings : []);
      if (res.result.kind === "dispatch" && res.result.action) {
        await runAction(res.result.action);
      }
    } catch (err) {
      console.error("myth_key_event failed:", err);
    }
  };

  // Badge for a file entry: exact or suffix path match (diff paths may be
  // absolute while tree paths are project-relative).
  const badgeForFile = (path: string): HoverBadge | null => {
    for (const [k, v] of hoverBadges) {
      if (k === path || k.endsWith("/" + path) || path.endsWith("/" + k)) return v;
    }
    return null;
  };

  // Collapsed dirs aggregate the badges of files hidden beneath them.
  const badgeForDir = (dirPath: string): HoverBadge | null => {
    let added = 0;
    let removed = 0;
    let hit = false;
    for (const [k, v] of hoverBadges) {
      if (k.startsWith(dirPath + "/") || k.includes("/" + dirPath + "/")) {
        added += v.added;
        removed += v.removed;
        hit = true;
      }
    }
    return hit ? { added, removed } : null;
  };

  const renderEntry = (entry: FileEntry, depth: number = 0) => {
    const isExpanded = expanded.has(entry.path);
    const isSelected = entry.path === selectedFile;
    const isKbSelected = selected?.path === entry.path;
    const children = entry.is_dir ? getChildren(entry.path) : [];
    const badge = entry.is_dir
      ? (isExpanded ? null : badgeForDir(entry.path))
      : badgeForFile(entry.path);

    return (
      <div key={entry.path}>
        <div
          className={`file-entry ${isSelected ? "selected" : ""} ${isKbSelected ? "kb-selected" : ""}`}
          style={{ paddingLeft: `${depth * 16 + 8}px` }}
          onClick={() => handleClick(entry)}
        >
          <span className="icon">
            {entry.is_dir ? (isExpanded ? "▼" : "▶") : "📄"}
          </span>
          <span className="name">{entry.name}</span>
          {badge && (
            <span className="file-diff-badge" title="Changes if you jump to the hovered undo node">
              <span className="file-diff-dot" />
              {badge.added > 0 && <span className="file-diff-added">+{badge.added}</span>}
              {badge.removed > 0 && <span className="file-diff-removed">−{badge.removed}</span>}
            </span>
          )}
        </div>
        {isExpanded &&
          children.map((child) => renderEntry(child, depth + 1))}
      </div>
    );
  };

  return (
    <div
      className="file-tree"
      tabIndex={0}
      onKeyDown={handleKeyDown}
      title="Keyboard: ↑↓ select, Enter open, Ctrl+Space or Ctrl+. for modes"
    >
      <div className="file-tree-header">
        <span>EXPLORER</span>
        {mythMode !== "Main" && <span className="myth-mode-badge">{mythMode}</span>}
        <button className="file-tree-refresh-btn" onClick={refreshAll} title="Refresh file tree">⟳</button>
      </div>
      <div className="file-tree-content">
        {rootEntries.map((entry) => renderEntry(entry))}
      </div>
      {whichKey.length > 0 && (
        <div className="which-key">
          <div className="which-key-title">{mythMode}</div>
          {whichKey.map((b) => (
            <div key={b.key} className="which-key-row">
              <span className="which-key-key">{b.key}</span>
              <span className={`which-key-target which-key-${b.kind}`}>
                {b.kind === "transition" ? `→${b.target}` : b.target.replace(/_/g, " ")}
              </span>
            </div>
          ))}
        </div>
      )}
    </div>
  );
}
