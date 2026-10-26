import {
  accessible,
  actionsAt,
  plainText,
  sameRole,
  type Buffer,
  type Presented,
  type Rendering,
  type Role,
} from "./view.ts";

export interface Piece {
  text: string;
  role: Role;
  at: number;
  actions: string[];
}

export function classOf(role: Role): string {
  if (typeof role === "string") return `role-${role}`;
  if ("token" in role) return `role-token token-${role.token.kind}`;
  if ("claim" in role) return `role-claim claim-${role.claim.role}`;
  return `role-mark role-mark-${role.mark.held ? role.mark.mark : "missing"}`;
}

function characters(text: string): string[] {
  return Array.from(text);
}

// @implements REQ-VIEW.structure_over_text
export function pieces(buffer: Buffer): Piece[][] {
  const all = characters(buffer.text);
  const lines: Piece[][] = [];
  let current: Piece[] = [];
  let at = 0;

  const roleAt = (offset: number): Role => {
    for (const span of buffer.spans) {
      if (span.start <= offset && offset < span.stop) return span.role;
    }
    return "plain";
  };

  while (at <= all.length) {
    if (at === all.length || all[at] === "\n") {
      lines.push(current);
      current = [];
      at += 1;
      if (at > all.length) break;
      continue;
    }
    const role = roleAt(at);
    const actions = actionsAt(buffer, at);
    const start = at;
    let text = "";
    while (
      at < all.length &&
      all[at] !== "\n" &&
      sameRole(roleAt(at), role) &&
      sameActions(actionsAt(buffer, at), actions)
    ) {
      text += all[at];
      at += 1;
    }
    current.push({ text, role, at: start, actions });
  }
  return lines;
}

function sameActions(a: string[], b: string[]): boolean {
  return a.length === b.length && a.every((name, i) => name === b[i]);
}

// @implements REQ-VIEW.frontend_is_checkable
// @implements REQ-VIEW.text_is_the_content
// @implements REQ-VIEW.frontend_adds_nothing
export function render(buffer: Buffer): Rendering {
  const offered: [number, string][] = [];
  const seen = new Set<number>();
  for (const line of pieces(buffer)) {
    for (const piece of line) {
      if (piece.actions.length === 0 || seen.has(piece.at)) continue;
      seen.add(piece.at);
      for (const action of piece.actions) offered.push([piece.at, action]);
    }
  }
  return { lines: plainText(buffer), offered };
}

// @implements REQ-VIEW.presentation_may_be_symbolic
const EMBLEMS: Record<string, string> = {
  "screen.station.project": "📁",
  "screen.station.trace": "🔗",
  "screen.station.sandbox": "🧪",
  "screen.station.design": "🕸",
  "screen.station.history": "🌳",
  "screen.station.findings": "🔍",
  "screen.station.definitions": "⚙",
};

export function symbolFor(actions: string[]): string | null {
  for (const action of actions) {
    const emblem = EMBLEMS[action];
    if (emblem) return emblem;
  }
  return null;
}

export interface Shown {
  piece: Piece;
  presented: Presented;
}

// @implements REQ-VIEW.presentation_may_be_symbolic
export function shown(buffer: Buffer, mislabel = false): Shown[][] {
  return pieces(buffer).map((line) =>
    line.map((piece) => {
      const painted = symbolFor(piece.actions) ?? piece.text;
      return { piece, presented: { painted, name: mislabel ? painted : piece.text } };
    }),
  );
}

export function presented(buffer: Buffer, mislabel = false): Presented[][] {
  return shown(buffer, mislabel).map((line) => line.map((each) => each.presented));
}

// @implements REQ-VIEW.screen_is_readable
export function screenText(buffer: Buffer, mislabel = false): string {
  return presented(buffer, mislabel).map(accessible).join("\n");
}
