import { invoke } from "@tauri-apps/api/core";

interface MenuBarProps {
  onProjectOpened: (root: string) => void;
  onToggleUndoTree: () => void;
  undoTreeVisible: boolean;
  onToggleRequirements: () => void;
  reqsPanelVisible: boolean;
}

export function MenuBar({
  onProjectOpened,
  onToggleUndoTree,
  undoTreeVisible,
  onToggleRequirements,
  reqsPanelVisible,
}: MenuBarProps) {
  const handleOpenFolder = async () => {
    try {
      const { open } = await import("@tauri-apps/plugin-dialog");
      const selected = await open({
        directory: true,
        multiple: false,
        title: "Open Project Folder",
      });
      if (selected && typeof selected === "string") {
        await invoke("open_project", { path: selected });
        onProjectOpened(selected);
      }
    } catch (e) {
      console.error("Failed to open folder:", e);
    }
  };

  const handleSave = async () => {
    try {
      await invoke("save_checkpoint");
    } catch (e) {
      console.error("Failed to save checkpoint:", e);
    }
  };

  return (
    <div className="menu-bar">
      <button onClick={handleOpenFolder}>File → Open Folder</button>
      <button onClick={handleSave}>Save Checkpoint</button>
      <button
        onClick={onToggleRequirements}
        className={reqsPanelVisible ? "active" : ""}
      >
        Requirements
      </button>
      <button
        onClick={onToggleUndoTree}
        className={undoTreeVisible ? "active" : ""}
      >
        Undo Tree
      </button>
      <span className="title">TraceLean IDE</span>
    </div>
  );
}
