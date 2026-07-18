/** Myth which-key entry (core Keymap::bindings_for_state). */
export interface KeyBindingInfo {
  key: string;
  target: string;
  kind: string;
}

/** Translate a key event to the keymap's key names ("C-Space", "S-ArrowUp", "f"). */
export function mythKeyName(e: { key: string; ctrlKey: boolean; altKey: boolean; shiftKey: boolean }): string | null {
  if (e.key === "Control" || e.key === "Shift" || e.key === "Alt" || e.key === "Meta") return null;
  let base = e.key === " " ? "Space" : e.key;
  if (base.length === 1) base = base.toLowerCase();
  let prefix = "";
  if (e.ctrlKey) prefix += "C-";
  if (e.altKey) prefix += "A-";
  if (e.shiftKey && base.length > 1) prefix += "S-";
  return prefix + base;
}
