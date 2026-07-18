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

export function FileTree({ projectRoot, onFileSelect, selectedFile }: FileTreeProps) {
  const [entries, setEntries] = useState<FileEntry[]>([]);
  const [expanded, setExpanded] = useState<Set<string>>(new Set());
  const [hoverBadges, setHoverBadges] = useState<Map<string, HoverBadge>>(new Map());
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
    const children = entry.is_dir ? getChildren(entry.path) : [];
    const badge = entry.is_dir
      ? (isExpanded ? null : badgeForDir(entry.path))
      : badgeForFile(entry.path);

    return (
      <div key={entry.path}>
        <div
          className={`file-entry ${isSelected ? "selected" : ""}`}
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
    <div className="file-tree">
      <div className="file-tree-header">
        <span>EXPLORER</span>
        <button className="file-tree-refresh-btn" onClick={refreshAll} title="Refresh file tree">⟳</button>
      </div>
      <div className="file-tree-content">
        {rootEntries.map((entry) => renderEntry(entry))}
      </div>
    </div>
  );
}
