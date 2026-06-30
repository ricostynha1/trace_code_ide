import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";

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

export function FileTree({ projectRoot, onFileSelect, selectedFile }: FileTreeProps) {
  const [entries, setEntries] = useState<FileEntry[]>([]);
  const [expanded, setExpanded] = useState<Set<string>>(new Set());

  useEffect(() => {
    loadDirectory("");
  }, [projectRoot]);

  const loadDirectory = async (path: string) => {
    try {
      const result = await invoke<FileEntry[]>("list_files", { path });
      if (path === "") {
        setEntries(result);
      } else {
        // Merge sub-entries
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
  };

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

  const renderEntry = (entry: FileEntry, depth: number = 0) => {
    const isExpanded = expanded.has(entry.path);
    const isSelected = entry.path === selectedFile;
    const children = entry.is_dir ? getChildren(entry.path) : [];

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
        </div>
        {isExpanded &&
          children.map((child) => renderEntry(child, depth + 1))}
      </div>
    );
  };

  return (
    <div className="file-tree">
      <div className="file-tree-header">EXPLORER</div>
      <div className="file-tree-content">
        {rootEntries.map((entry) => renderEntry(entry))}
      </div>
    </div>
  );
}
