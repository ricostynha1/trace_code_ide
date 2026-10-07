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
  const [path, setPath] = useState<string[]>(["Main"]);
  const [note, setNote] = useState("");
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
        | { state?: string; bindings?: KeyBindingInfo[]; path?: string[]; note?: string }
        | undefined;
      setMode(detail?.state ?? "Main");
      setPath(detail?.path ?? [detail?.state ?? "Main"]);
      setNote(detail?.note ?? "");
      setBindings(detail?.bindings ?? []);
    };
    window.addEventListener("myth-mode", handler);
    return () => window.removeEventListener("myth-mode", handler);
  }, []);

  if (bindings.length === 0) return null;

  // Modes nest, so the bar shows how you got here ("Options › Trace"): a
  // nested mode with only its own name on screen leaves you guessing what
  // Escape will do.
  const breadcrumb = path.length > 1 ? path.join(" › ") : mode;

  // Entries from a provider carry a group; keep those together, in the order
  // core sent them, so the list does not reshuffle between invocations.
  const groups: Array<[string, KeyBindingInfo[]]> = [];
  for (const b of bindings.filter((x) => x.key !== "NM")) {
    const key = b.group ?? "";
    const existing = groups.find(([g]) => g === key);
    if (existing) existing[1].push(b);
    else groups.push([key, [b]]);
  }

  const label = (b: KeyBindingInfo) =>
    b.kind === "transition"
      ? `→${b.target}`
      : b.kind === "pop"
        ? "back"
        : b.kind === "reset"
          ? "cancel"
          : b.title ?? b.target.replace(/_/g, " ");

  return (
    <div className="which-key-bar">
      <span className="which-key-bar-mode">{breadcrumb}</span>
      <div className="which-key-bar-grid">
        {groups.map(([group, entries]) => (
          <span key={group || "_"} className="which-key-group">
            {group && <span className="which-key-group-label">{group}</span>}
            {entries.map((b) => (
              <span key={group + b.key} className="which-key-row">
                <span className="which-key-key">{b.key}</span>
                <span className={`which-key-target which-key-${b.kind}`}>{label(b)}</span>
              </span>
            ))}
          </span>
        ))}
      </div>
      {note && <span className="which-key-note">{note}</span>}
    </div>
  );
}
