/** Which sandbox session is "active" — created most recently in this app
 * run, or explicitly selected in `SandboxPanel`. Module-scope (not React
 * state) so `AiChatPanel`'s transcript tab can default to it without prop
 * drilling, following the same pattern as `diffViewStore.ts` for state that
 * must be shared across sibling panels.
 */

interface SandboxStore {
  activeSessionId: string | null;
}

export const sandboxStore: SandboxStore = {
  activeSessionId: null,
};

export function setActiveSandboxSession(id: string | null) {
  sandboxStore.activeSessionId = id;
  window.dispatchEvent(new CustomEvent("sandbox-active-session-changed", { detail: { id } }));
}
