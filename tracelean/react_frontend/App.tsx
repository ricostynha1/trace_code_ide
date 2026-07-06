import { useState, useEffect } from "react";
import { invoke } from "@tauri-apps/api/core";
import { FileTree } from "./components/FileTree";
import { Editor } from "./components/Editor";
import { MenuBar } from "./components/MenuBar";
import { UndoTreePanel } from "./components/UndoTreePanel";
import { RequirementsPanel } from "./components/RequirementsPanel";
import { AiChatPanel } from "./components/AiChatPanel";
import { MockPromptWindow } from "./components/MockPromptWindow";
import { TraceabilityDashboard } from "./components/TraceabilityDashboard";
import "./App.css";

function App() {
  const [projectOpen, setProjectOpen] = useState(false);
  const [currentFile, setCurrentFile] = useState<string | null>(null);
  const [projectRoot, setProjectRoot] = useState<string>("");
  const [undoTreeVisible, setUndoTreeVisible] = useState(false);
  const [reqsPanelVisible, setReqsPanelVisible] = useState(false);
  const [aiPanelVisible, setAiPanelVisible] = useState(false);
  const [traceDashVisible, setTraceDashVisible] = useState(false);
  // Key to force editor remount on undo-tree jump
  const [editorKey, setEditorKey] = useState(0);

  // Auto-open /project if mounted (Docker usage)
  useEffect(() => {
    invoke<string | null>("get_initial_project").then(async (path) => {
      if (path) {
        await invoke("open_project", { path });
        setProjectRoot(path);
        setProjectOpen(true);
      }
    }).catch(() => {});
  }, []);

  // Listen for trace navigation events from Editor
  useEffect(() => {
    const handler = (e: Event) => {
      const detail = (e as CustomEvent).detail;
      if (detail?.path) {
        setCurrentFile(detail.path);
      }
    };
    window.addEventListener("tracelean-navigate", handler);
    return () => window.removeEventListener("tracelean-navigate", handler);
  }, []);

  const handleProjectOpened = (root: string) => {
    setProjectRoot(root);
    setProjectOpen(true);
  };

  const handleFileSelect = (path: string) => {
    setCurrentFile(path);
  };

  const handleNodeJump = () => {
    // Force editor to reload content from backend
    setEditorKey((k) => k + 1);
  };

  return (
    <div className="app">
      <MenuBar
        onProjectOpened={handleProjectOpened}
        onToggleUndoTree={() => setUndoTreeVisible((v) => !v)}
        undoTreeVisible={undoTreeVisible}
        onToggleRequirements={() => setReqsPanelVisible((v) => !v)}
        reqsPanelVisible={reqsPanelVisible}
        onToggleAiChat={() => setAiPanelVisible((v) => !v)}
        aiChatVisible={aiPanelVisible}
        onToggleTraceDashboard={() => setTraceDashVisible((v) => !v)}
        traceDashVisible={traceDashVisible}
      />
      <div className="main-content">
        {projectOpen && (
          <FileTree
            projectRoot={projectRoot}
            onFileSelect={handleFileSelect}
            selectedFile={currentFile}
          />
        )}
        <div className="editor-area">
          {currentFile ? (
            <Editor key={`${currentFile}-${editorKey}`} filePath={currentFile} />
          ) : (
            <div className="welcome">
              <h2>TraceLean IDE</h2>
              <p>Open a project folder to get started (File → Open Folder)</p>
            </div>
          )}
        </div>
        <RequirementsPanel
          visible={reqsPanelVisible}
          onClose={() => setReqsPanelVisible(false)}
          onFileSelect={handleFileSelect}
        />
        <UndoTreePanel
          visible={undoTreeVisible}
          onClose={() => setUndoTreeVisible(false)}
          onNodeJump={handleNodeJump}
          onFileSelect={handleFileSelect}
          currentFile={currentFile}
        />
        <AiChatPanel
          visible={aiPanelVisible}
          onClose={() => setAiPanelVisible(false)}
        />
        <MockPromptWindow />
        <TraceabilityDashboard
          visible={traceDashVisible}
          onClose={() => setTraceDashVisible(false)}
          onNavigate={(file, _line) => {
            setCurrentFile(file);
            // Could also navigate to line in future
          }}
        />
      </div>
    </div>
  );
}

export default App;
