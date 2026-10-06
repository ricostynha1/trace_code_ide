// The web frontend's reading of a buffer.
//
// A frontend has to split a buffer's text into lines to draw it, and it has to
// know what can be done at a position to offer it. Those are the same two
// questions the core answers, so this is a second implementation of them — and
// a second implementation is exactly the thing that drifts.
//
// So it is not trusted. Every function here is compared against the Lean model,
// case for case, by the same differential machinery that checks the Rust core
// (`REQ-DRT-TS`). It is not compared against the Rust: two implementations
// agree perfectly when both are wrong.
//
// What is deliberately absent is any producer. Building a buffer here would
// make this a second answer to what is on screen, which is the failure
// `REQ-SHOW.core_produces` exists to prevent. Buffers arrive from the core.
//
// There are no `@` annotations in this file. TraceLean has no TypeScript
// grammar, so it cannot say *which declaration* a claim here is about, and a
// claim that cannot be placed is worse than one that is made elsewhere (see the
// rejected alternative in ADR-0008). The claims about this file are made where
// they can be checked: `.tracelean/drt.json` names `web/src/view.ts::plainText`
// and its two neighbours as implementations of `REQ-VIEW`, and
// `crates/core/tests/differential_typescript.rs` is what drives them.

// An evidence level, as the core spells it.
export type Grade = "L1" | "L2" | "L3" | "L4";

// What a region of text *is*. Never how it looks.
//
// `level` carries which grade it is, so a frontend colours L1 and L4 apart
// without reading the characters — a frontend that parsed the text to decide
// how to draw it would be computing its own view of the buffer.
export type Role =
  | "plain"
  | "path"
  | "entry"
  | "heading"
  | "requirement"
  | { level: { grade: Grade } }
  | "added"
  | "removed"
  | { token: { kind: string } }
  | { claim: { role: string } };

// Whether two roles are the same. Written out because two of them are objects
// and `===` on objects compares identity, which would make every character of a
// level or a token its own region.
export function sameRole(a: Role, b: Role): boolean {
  if (typeof a === "string" || typeof b === "string") return a === b;
  if ("level" in a && "level" in b) return a.level.grade === b.level.grade;
  if ("token" in a && "token" in b) return a.token.kind === b.token.kind;
  if ("claim" in a && "claim" in b) return a.claim.role === b.claim.role;
  return false;
}

export type BufferKind =
  | { file: { path: string } }
  | { directory: { path: string } }
  | { review: { target: string } }
  | { menu: { title: string } }
  | { record: { title: string } };

export interface Span {
  start: number;
  stop: number;
  role: Role;
  actions: string[];
}

export interface Buffer {
  id: string;
  kind: BufferKind;
  text: string;
  spans: Span[];
}

export interface Rendering {
  lines: string[];
  offered: [number, string][];
}

// A region of the screen, in characters. The page reports its size in these,
// because only it knows what its font does, and the core answers in the same
// unit — so a pane's box and the pane the core laid out are the same rectangle.
export interface Rect {
  left: number;
  top: number;
  width: number;
  height: number;
}

// One pane, as the editor hands it over.
export interface Pane {
  pane: string;
  at: Rect;
  buffer: Buffer;
  focused: boolean;
  // The buffer line the window starts at; a gutter numbers from it. Absent
  // from an editor that predates it, which numbers from the top.
  top?: number;
  // The claims marked beside the window's lines: buffer line, the role's
  // letter, the requirement it opens.
  chips?: [number, string, string][];
}

export type Breach =
  | { lineDiffers: { line: number; shown: string; expected: string } }
  | { lineMissing: { line: number; expected: string } }
  | { lineExtra: { line: number; shown: string } }
  | { actionInvented: { offset: number; action: string } }
  | { actionDropped: { offset: number; action: string } };

// The rendering every frontend has to agree with: the buffer's text, as lines.
//
// A frontend with a richer medium draws more than this. What it may not do is
// show something else.
//
// Realises REQ-VIEW.rendering_is_total; bound in `.tracelean/drt.json`.
export function plainText(buffer: Buffer): string[] {
  return buffer.text.split("\n");
}

// What can be done at a position: the actions of every span covering it, in
// span order, without repeats.
//
// Offsets are characters and not code units. A `String.length` here would put
// the frontend a position out of step with the core the moment anything is
// outside the basic plane, which is the kind of disagreement that shows up as
// one wrong button and no error.
//
// Realises REQ-VIEW.affordances_named; bound in `.tracelean/drt.json`.
export function actionsAt(buffer: Buffer, offset: number): string[] {
  const seen: string[] = [];
  for (const span of buffer.spans) {
    if (span.start <= offset && offset < span.stop) {
      for (const action of span.actions) {
        if (!seen.includes(action)) seen.push(action);
      }
    }
  }
  return seen;
}

// Every action any span of a buffer names, sorted and without repeats.
//
// Realises REQ-VIEW.frontend_adds_nothing.
export function declaredActions(buffer: Buffer): string[] {
  const seen: string[] = [];
  for (const span of buffer.spans) {
    for (const action of span.actions) {
      if (!seen.includes(action)) seen.push(action);
    }
  }
  // The model sorts by code point, which is what a plain comparison gives.
  return seen.sort((a, b) => (a < b ? -1 : a > b ? 1 : 0));
}

// A region as this frontend put it on the screen: the glyphs it painted, and
// the name those glyphs carry.
//
// The name is what a screen reader announces and what the capture harness
// reads off the page. Painting is this medium's freedom; the name is its
// obligation.
export interface Presented {
  painted: string;
  name: string;
}

// What a row of presented regions reads as: the names, in order.
//
// The glyphs are not consulted, which is the whole content of the clause.
//
// Realises REQ-VIEW.presentation_may_be_symbolic; bound in `.tracelean/drt.json`.
export function accessible(row: Presented[]): string {
  return row.map((piece) => piece.name).join("");
}

function lineBreaches(shown: string[], expected: string[]): Breach[] {
  const out: Breach[] = [];
  const most = Math.max(shown.length, expected.length);
  for (let line = 0; line < most; line += 1) {
    if (line >= expected.length) {
      out.push({ lineExtra: { line, shown: shown[line] } });
    } else if (line >= shown.length) {
      out.push({ lineMissing: { line, expected: expected[line] } });
    } else if (shown[line] !== expected[line]) {
      out.push({ lineDiffers: { line, shown: shown[line], expected: expected[line] } });
    }
  }
  return out;
}

// Everything a frontend got wrong about a buffer.
//
// The frontend carries this so that it can check itself in a browser, where no
// harness reaches. The same function checked against the same model is the
// reason its answer means anything.
//
// Realises REQ-VIEW.frontend_adds_nothing; bound in `.tracelean/drt.json`.
export function conformance(buffer: Buffer, rendering: Rendering): Breach[] {
  const out: Breach[] = lineBreaches(rendering.lines, plainText(buffer));
  for (const [offset, action] of rendering.offered) {
    if (!actionsAt(buffer, offset).includes(action)) {
      out.push({ actionInvented: { offset, action } });
    }
  }
  const positions: number[] = [];
  for (const [offset] of rendering.offered) {
    if (!positions.includes(offset)) positions.push(offset);
  }
  for (const offset of positions) {
    for (const action of actionsAt(buffer, offset)) {
      const offered = rendering.offered.some(
        (pair) => pair[0] === offset && pair[1] === action,
      );
      if (!offered) out.push({ actionDropped: { offset, action } });
    }
  }
  return out;
}
