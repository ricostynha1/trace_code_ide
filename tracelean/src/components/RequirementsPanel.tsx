import { useEffect, useState, useCallback } from "react";
import { invoke } from "@tauri-apps/api/core";

interface RequirementInfo {
  id: string;
  title: string;
  status: "Draft" | "Approved" | "Linked";
  file: string;
  description: string;
  has_spec: boolean;
}

interface RequirementsPanelProps {
  visible: boolean;
  onClose: () => void;
  onFileSelect: (path: string) => void;
}

const STATUS_COLORS: Record<string, string> = {
  Draft: "#888",
  Approved: "#d19a66",
  Linked: "#4ec9b0",
};

const STATUS_ICONS: Record<string, string> = {
  Draft: "○",
  Approved: "◐",
  Linked: "●",
};

export function RequirementsPanel({
  visible,
  onClose,
  onFileSelect,
}: RequirementsPanelProps) {
  const [requirements, setRequirements] = useState<RequirementInfo[]>([]);
  const [showCreate, setShowCreate] = useState(false);
  const [newId, setNewId] = useState("");
  const [newTitle, setNewTitle] = useState("");
  const [message, setMessage] = useState("");

  const refresh = useCallback(async () => {
    try {
      const reqs = await invoke<RequirementInfo[]>("list_requirements");
      setRequirements(reqs);
    } catch (e) {
      console.error("Failed to list requirements:", e);
    }
  }, []);

  useEffect(() => {
    if (visible) {
      refresh();
    }
  }, [visible, refresh]);

  const handleStatusChange = async (reqId: string, newStatus: string) => {
    try {
      const result = await invoke<string>("update_requirement_status", {
        reqId,
        newStatus,
      });
      setMessage(result);
      setTimeout(() => setMessage(""), 3000);
      refresh();
    } catch (e: any) {
      setMessage(`Error: ${e}`);
      setTimeout(() => setMessage(""), 4000);
    }
  };

  const handleCreate = async () => {
    if (!newId || !newTitle) return;
    try {
      const result = await invoke<string>("create_requirement", {
        reqId: newId,
        title: newTitle,
      });
      setMessage(result);
      setShowCreate(false);
      setNewId("");
      setNewTitle("");
      setTimeout(() => setMessage(""), 3000);
      refresh();
    } catch (e: any) {
      setMessage(`Error: ${e}`);
      setTimeout(() => setMessage(""), 4000);
    }
  };

  const handleNavigateSpec = async (reqId: string) => {
    const fromPath = `reqs/${reqId}.md`;
    try {
      const target = await invoke<string | null>("navigate_trace_link", {
        fromPath,
      });
      if (target) {
        onFileSelect(target);
      }
    } catch (e) {
      console.error("Navigation failed:", e);
    }
  };

  if (!visible) return null;

  return (
    <div className="requirements-panel">
      <div className="panel-header">
        <span className="panel-title">REQUIREMENTS</span>
        <div className="panel-actions">
          <button
            className="panel-btn"
            onClick={() => setShowCreate(!showCreate)}
            title="New Requirement"
          >
            +
          </button>
          <button className="panel-btn" onClick={refresh} title="Refresh">
            ↻
          </button>
          <button className="panel-close" onClick={onClose}>
            ✕
          </button>
        </div>
      </div>

      {message && <div className="req-message">{message}</div>}

      {showCreate && (
        <div className="req-create-form">
          <input
            placeholder="REQ-XX"
            value={newId}
            onChange={(e) => setNewId(e.target.value)}
            className="req-input"
          />
          <input
            placeholder="Title"
            value={newTitle}
            onChange={(e) => setNewTitle(e.target.value)}
            className="req-input"
          />
          <button className="req-create-btn" onClick={handleCreate}>
            Create
          </button>
        </div>
      )}

      <div className="req-list">
        {requirements.length === 0 ? (
          <div className="empty-state">
            No requirements found.
            <br />
            Create a <code>reqs/</code> folder with markdown files.
          </div>
        ) : (
          requirements.map((req) => (
            <div key={req.id} className="req-item">
              <div className="req-item-header">
                <span
                  className="req-status-icon"
                  style={{ color: STATUS_COLORS[req.status] }}
                  title={req.status}
                >
                  {STATUS_ICONS[req.status]}
                </span>
                <span
                  className="req-id"
                  onClick={() => onFileSelect(req.file)}
                  title="Open requirement file"
                >
                  {req.id}
                </span>
                <span className="req-title">{req.title}</span>
              </div>
              <div className="req-item-actions">
                {req.status === "Draft" && (
                  <button
                    className="req-action-btn"
                    onClick={() => handleStatusChange(req.id, "approved")}
                    title="Approve (creates spec file)"
                  >
                    Approve
                  </button>
                )}
                {req.status === "Approved" && (
                  <button
                    className="req-action-btn"
                    onClick={() => handleStatusChange(req.id, "linked")}
                    title="Mark as linked to implementation"
                  >
                    Link
                  </button>
                )}
                {req.has_spec && (
                  <button
                    className="req-action-btn spec-btn"
                    onClick={() => handleNavigateSpec(req.id)}
                    title="Open Lean spec"
                  >
                    Spec →
                  </button>
                )}
              </div>
            </div>
          ))
        )}
      </div>
    </div>
  );
}
