import { useState, useRef } from "react";

export interface ChatInstance {
  id: string;
  messages: { role: string; content: string; tool_call_id?: string; tool_calls?: any[] }[];
  createdAt: Date;
  label: string;
  /** P11: cumulative session cost from the persisted session file. */
  cost?: number;
}

interface Props {
  chatInstances: ChatInstance[];
  activeChatId: string;
  onSwitch: (id: string) => void;
  onNewChat: () => void;
  onReorder: (instances: ChatInstance[]) => void;
  onClose: () => void;
}

export function ChatSwitcher({ chatInstances, activeChatId, onSwitch, onNewChat, onReorder, onClose }: Props) {
  const [dragIdx, setDragIdx] = useState<number | null>(null);
  const [dragOverIdx, setDragOverIdx] = useState<number | null>(null);
  const dragNode = useRef<HTMLDivElement | null>(null);

  const handleDragStart = (e: React.DragEvent, idx: number) => {
    setDragIdx(idx);
    dragNode.current = e.currentTarget as HTMLDivElement;
    e.dataTransfer.effectAllowed = "move";
  };

  const handleDragOver = (e: React.DragEvent, idx: number) => {
    e.preventDefault();
    if (dragIdx === null || dragIdx === idx) return;
    setDragOverIdx(idx);
  };

  const handleDrop = (e: React.DragEvent, idx: number) => {
    e.preventDefault();
    if (dragIdx === null || dragIdx === idx) return;
    const reordered = [...chatInstances];
    const [moved] = reordered.splice(dragIdx, 1);
    reordered.splice(idx, 0, moved);
    onReorder(reordered);
    setDragIdx(null);
    setDragOverIdx(null);
  };

  const handleDragEnd = () => {
    setDragIdx(null);
    setDragOverIdx(null);
  };

  const getPreview = (inst: ChatInstance): string => {
    const firstUser = inst.messages.find((m) => m.role === "user");
    if (firstUser) {
      return firstUser.content.length > 50 ? firstUser.content.slice(0, 50) + "…" : firstUser.content;
    }
    return inst.label || "Empty chat";
  };

  const formatTime = (d: Date): string => {
    const date = new Date(d);
    return date.toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" });
  };

  return (
    <div className="chat-switcher-overlay" onClick={onClose}>
      <div className="chat-switcher-dialog" onClick={(e) => e.stopPropagation()}>
        <div className="chat-switcher-header">
          <span className="chat-switcher-title">Chat Sessions</span>
          <button className="chat-switcher-close" onClick={onClose}>×</button>
        </div>
        <div className="chat-switcher-list">
          {chatInstances.map((inst, idx) => (
            <div
              key={inst.id}
              className={`chat-switcher-item${inst.id === activeChatId ? " active" : ""}${dragOverIdx === idx ? " drag-over" : ""}`}
              draggable
              onDragStart={(e) => handleDragStart(e, idx)}
              onDragOver={(e) => handleDragOver(e, idx)}
              onDrop={(e) => handleDrop(e, idx)}
              onDragEnd={handleDragEnd}
              onClick={() => { onSwitch(inst.id); onClose(); }}
            >
              <div className="chat-switcher-item-preview">{getPreview(inst)}</div>
              <div className="chat-switcher-item-meta">
                <span>{inst.messages.length} msgs</span>
                {inst.cost !== undefined && inst.cost > 0 && (
                  <span className="chat-switcher-cost">${inst.cost.toFixed(4)}</span>
                )}
                <span>{formatTime(inst.createdAt)}</span>
              </div>
            </div>
          ))}
          {chatInstances.length === 0 && (
            <div className="chat-switcher-empty">No chats yet</div>
          )}
        </div>
        <div className="chat-switcher-footer">
          <button className="chat-switcher-new-btn" onClick={() => { onNewChat(); onClose(); }}>+ New Chat</button>
        </div>
      </div>
    </div>
  );
}
