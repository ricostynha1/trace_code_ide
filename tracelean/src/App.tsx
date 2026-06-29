import { useState } from "react";
import { FileTree } from "./components/FileTree";
import { Editor } from "./components/Editor";
import { MenuBar } from "./components/MenuBar";
import { UndoTreePanel } from "./components/UndoTreePanel";
import "./App.css";

function App() {
  const [projectOpen, setProjectOpen] = useState(false);
  const [currentFile, setCurrentFile] = useState<string | null>(null);
  const [projectRoot, setProjectRoot] = useState<string>("");
  const [undoTreeVisible, setUndoTreeVisible] = useState(false);
  // Key to force editor remount on undo-tree jump
  const [editorKey, setEditorKey] = useState(0);

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
        <UndoTreePanel
          visible={undoTreeVisible}
          onClose={() => setUndoTreeVisible(false)}
          onNodeJump={handleNodeJump}
          onFileSelect={handleFileSelect}
          currentFile={currentFile}
        />
      </div>
    </div>
  );
}

export default App;
