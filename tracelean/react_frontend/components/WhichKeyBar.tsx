import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import type { KeyBindingInfo } from "./mythKeys";

/**
 * Emacs-style which-key bar: a full-width strip at the bottom of the app that
 * shows the active keymap mode and its bindings. FileTree/Editor broadcast
 * mode changes via the "myth-mode" CustomEvent after each myth_key_event.
 *
 * Discoverability fix: this used to hide itself entirely whenever the mode
 * was "Main" (`mode === "Main" || ... return null`), including on first
 * load before any key had been pressed. That meant the *only* way to learn
 * the leader key that opens the keymap at all (`C-Space`/`C-.`, per
 * ui_settings/keymap.json) was to already know it — the bar had nothing to
 * discover it with. Main's own bindings are shown here like any other mode;
 * only a genuinely empty binding set hides the bar.
 */
export function WhichKeyBar() {
  const [mode, setMode] = useState("Main");
  const [bindings, setBindings] = useState<KeyBindingInfo[]>([]);

  useEffect(() => {
    // Fetch Main's bindings on mount so the leader-key hint (e.g. "C-Space
    // → Options") is visible immediately, not only after the first keypress
    // inside Editor/FileTree ever dispatches a "myth-mode" event.
    invoke<KeyBindingInfo[]>("myth_which_key", { state: "Main" })
      .then((b) => setBindings(b ?? []))
      .catch((err) => console.error("myth_which_key failed:", err));

    const handler = (e: Event) => {
      const detail = (e as CustomEvent).detail as
        | { state?: string; bindings?: KeyBindingInfo[] }
        | undefined;
      setMode(detail?.state ?? "Main");
      setBindings(detail?.bindings ?? []);
    };
    window.addEventListener("myth-mode", handler);
    return () => window.removeEventListener("myth-mode", handler);
  }, []);

  if (bindings.length === 0) return null;

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
