// Drawing a buffer in a web view.
//
// The counterpart of the terminal frontend's `draw` module, and the same rule:
// this is the only place the web frontend decides what the screen says, so the
// answer it gives a conformance harness and the pixels a person sees come from
// one function.
//
// A richer medium than a terminal, so it draws more: a span with an action
// becomes a button. What it may not do is draw text the buffer does not carry
// or offer an action no span declares, and neither is possible here — the text
// comes from `plainText` and the actions from `actionsAt`.
//
// Realises REQ-VIEW.text_is_the_content and REQ-VIEW.frontend_adds_nothing.
// Written out rather than annotated: TraceLean has no TypeScript grammar, so it
// cannot place a claim inside this file, and a claim that cannot be placed is
// worse than one made where it is checked (ADR-0008). What checks this file is
// `crates/core/tests/frontend_conformance_web.rs`.

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

// One piece of a line, and what it is.
export interface Piece {
  text: string;
  role: Role;
  // The character offset this piece starts at, which is what an action is
  // dispatched with.
  at: number;
  actions: string[];
}

// A role becomes a class name. The role says what a region *is*; how it looks
// is the stylesheet's business, which is the same division the terminal
// frontend makes with colours.
//
// A level gets two classes: one for being a level at all, one for its grade.
// So a stylesheet that has not been taught the grades still colours levels, and
// one that has tells L1 from L4 — which is `rendering_is_total` applied to a
// role's payload rather than to the role.
export function classOf(role: Role): string {
  if (typeof role === "string") return `role-${role}`;
  if ("token" in role) return `role-token token-${role.token.kind}`;
  if ("claim" in role) return `role-claim claim-${role.claim.role}`;
  return `role-level role-level-${role.level.grade.toLowerCase()}`;
}

// Characters, not code units: `text[i]` in JavaScript indexes UTF-16, so a
// character outside the basic plane would be sliced in half and the halves
// drawn as two replacement characters. The core counts characters, so this
// does too.
function characters(text: string): string[] {
  return Array.from(text);
}

// Split a buffer into lines of pieces, each piece carrying what can be done
// where it starts.
//
// A piece boundary falls wherever the set of actions changes, so a frontend can
// draw one button per region without deciding where the regions are.
//
// Realises REQ-VIEW.structure_over_text.
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

// What this frontend drew, in the form the conformance harness checks.
//
// The lines are the buffer's own, and an action is offered at the start of
// every piece that carries one — which is where the button goes.
//
// Realises REQ-VIEW.frontend_is_checkable.
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

// A region's emblem, chosen by an action the core declared.
//
// Keyed by action and never by text. An emblem picked by matching on strings
// this frontend recognised would be the frontend deciding what exists, which is
// the thing `one_representation` exists to prevent — and nothing could check
// that it matched correctly. An action is a name the keymap dispatches, so a
// window mapping it to a glyph is doing what the terminal does when it maps it
// to a key.
//
// An action not named here has no emblem and is drawn as its text, which is
// `rendering_is_total` for this medium.
//
// Realises REQ-VIEW.presentation_may_be_symbolic.
const EMBLEMS: Record<string, string> = {
  "screen.station.project": "📁",
  "screen.station.sandbox": "🧪",
  "screen.station.requirements": "📋",
  "screen.station.design": "🕸",
  "screen.station.history": "🌳",
};

export function symbolFor(actions: string[]): string | null {
  for (const action of actions) {
    const emblem = EMBLEMS[action];
    if (emblem) return emblem;
  }
  return null;
}

// A piece together with how this frontend presents it.
//
// One value, because the page and the harness must not be two decisions. The
// page builds its DOM from this and the harness reads `screenText`, which is
// this too — so a frontend cannot paint one thing and report another without
// the two disagreeing about a value it computed once.
export interface Shown {
  piece: Piece;
  presented: Presented;
}

// How this frontend presents a buffer: for each piece, what it paints and the
// name that painting carries.
//
// The name is always the piece's own text. The painting may be an emblem, and
// when it is, the DOM built from this puts the glyph on the page and the
// piece's text in its accessible name — so a person using a screen reader and
// the capture harness read the same thing, and both read the buffer.
//
// `mislabel` is the wrong-on-purpose mode: a frontend that names a region after
// the glyph it painted rather than after the text it stands for. It exists so
// that the harness can be seen catching it.
//
// Realises REQ-VIEW.presentation_may_be_symbolic.
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

// The text a person would read off the page: the accessible name of every
// presented piece, joined.
//
// This is what the capture harness compares against the buffer.
//
// Realises REQ-VIEW.screen_is_readable.
export function screenText(buffer: Buffer, mislabel = false): string {
  return presented(buffer, mislabel).map(accessible).join("\n");
}
