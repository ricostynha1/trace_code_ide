import { useState, useEffect, useRef } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { FileTree } from "./components/FileTree";
import { Editor } from "./components/Editor";
import { MenuBar } from "./components/MenuBar";
import { UndoTreePanel } from "./components/UndoTreePanel";
import { RequirementsPanel } from "./components/RequirementsPanel";
import { AiChatPanel } from "./components/AiChatPanel";
import { MockPromptWindow } from "./components/MockPromptWindow";
import { TraceabilityDashboard } from "./components/TraceabilityDashboard";
import { WhichKeyBar } from "./components/WhichKeyBar";
import { Splitter } from "./components/Splitter";
import { DiffReviewPanel } from "./components/DiffReviewPanel";
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
  // Track whether AI is actively running — prevent file switching during AI edits
  const aiActiveRef = useRef(false);

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

  // Track AI activity to prevent unwanted file switching during AI edits
  useEffect(() => {
    const unlistenStart = listen("tool-call", (event: any) => {
      if (event.payload?.status === "running") aiActiveRef.current = true;
      else aiActiveRef.current = false;
    });
    return () => { unlistenStart.then((fn) => fn()); };
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
    // Don't switch files while AI is actively executing tools
    if (aiActiveRef.current) return;
    setCurrentFile(path);
  };

  // Direct file select — always works, used for explicit user navigation
  const handleFileSelectDirect = (path: string) => {
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
          <>
            <FileTree
              projectRoot={projectRoot}
              onFileSelect={handleFileSelectDirect}
              selectedFile={currentFile}
            />
            <Splitter cssVar="--file-tree-width" side="left" defaultWidth={240} />
          </>
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
          <DiffReviewPanel />
        </div>
        {reqsPanelVisible && (
          <Splitter cssVar="--reqs-panel-width" side="right" defaultWidth={280} />
        )}
        <RequirementsPanel
          visible={reqsPanelVisible}
          onClose={() => setReqsPanelVisible(false)}
          onFileSelect={handleFileSelect}
        />
        {undoTreeVisible && (
          <Splitter cssVar="--undo-panel-width" side="right" defaultWidth={220} />
        )}
        <UndoTreePanel
          visible={undoTreeVisible}
          onClose={() => setUndoTreeVisible(false)}
          onNodeJump={handleNodeJump}
          onFileSelect={handleFileSelect}
          currentFile={currentFile}
        />
        {aiPanelVisible && (
          <Splitter cssVar="--ai-panel-width" side="right" defaultWidth={360} />
        )}
        <AiChatPanel
          visible={aiPanelVisible}
          onClose={() => setAiPanelVisible(false)}
        />
        <MockPromptWindow />
        <TraceabilityDashboard
          visible={traceDashVisible}
          onClose={() => setTraceDashVisible(false)}
          onNavigate={(file, _line) => {
            handleFileSelectDirect(file);
            // Could also navigate to line in future
          }}
        />
      </div>
      <WhichKeyBar />
    </div>
  );
}

export default App;
