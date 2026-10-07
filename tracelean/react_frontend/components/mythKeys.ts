/** Myth which-key entry (core Keymap::bindings_for_state). */
export interface KeyBindingInfo {
  key: string;
  target: string;
  kind: string;
  /** Human label, when core knows one ("Who wrote this line"). */
  title?: string;
  /** Menu grouping, for modes whose entries come from a provider. */
  group?: string;
  /** Payload passed through ActionCtx.args when this entry is chosen. */
  args?: unknown;
}

/** Translate a key event to the keymap's key names ("C-Space", "S-ArrowUp", "f"). */
export function mythKeyName(e: { key: string; ctrlKey: boolean; altKey: boolean; shiftKey: boolean }): string | null {
  if (e.key === "Control" || e.key === "Shift" || e.key === "Alt" || e.key === "Meta") return null;
  let base = e.key === " " ? "Space" : e.key;
  // Single characters keep the case the keyboard actually produced: lowercasing
  // them unconditionally made every uppercase binding unreachable (Trace mode's
  // `M` for the coverage map arrived as `m`, which is "go to tests").
  if (base.length === 1 && !e.shiftKey) base = base.toLowerCase();
  let prefix = "";
  if (e.ctrlKey) prefix += "C-";
  if (e.altKey) prefix += "A-";
  // Named keys (ArrowUp, Enter) need the modifier spelled out; printable ones
  // already carry it in the character itself.
  if (e.shiftKey && base.length > 1) prefix += "S-";
  return prefix + base;
}
