import { useEffect, useState } from "react";
import type { KeyBindingInfo } from "./mythKeys";

/**
 * Emacs-style which-key bar: a full-width strip at the bottom of the app that
 * shows the active keymap mode and its bindings. FileTree/Editor broadcast
 * mode changes via the "myth-mode" CustomEvent after each myth_key_event.
 */
export function WhichKeyBar() {
  const [mode, setMode] = useState("Main");
  const [bindings, setBindings] = useState<KeyBindingInfo[]>([]);

  useEffect(() => {
    const handler = (e: Event) => {
      const detail = (e as CustomEvent).detail as
        | { state?: string; bindings?: KeyBindingInfo[] }
        | undefined;
      const state = detail?.state ?? "Main";
      setMode(state);
      setBindings(state !== "Main" ? detail?.bindings ?? [] : []);
    };
    window.addEventListener("myth-mode", handler);
    return () => window.removeEventListener("myth-mode", handler);
  }, []);

  if (mode === "Main" || bindings.length === 0) return null;

  return (
    <div className="which-key-bar">
      <span className="which-key-bar-mode">{mode}</span>
      <div className="which-key-bar-grid">
        {bindings
          .filter((b) => b.key !== "NM")
          .map((b) => (
            <span key={b.key} className="which-key-row">
              <span className="which-key-key">{b.key}</span>
              <span className={`which-key-target which-key-${b.kind}`}>
                {b.kind === "transition" ? `→${b.target}` : b.target.replace(/_/g, " ")}
              </span>
            </span>
          ))}
      </div>
    </div>
  );
}
