export type EvidenceMark = "annotated" | "judged" | "tested" | "proved";

export type Role =
  | "plain"
  | "path"
  | "entry"
  | "heading"
  | "requirement"
  | { mark: { mark: EvidenceMark; held: boolean } }
  | "added"
  | "removed"
  | { token: { kind: string } }
  | { claim: { role: string } };

export function sameRole(a: Role, b: Role): boolean {
  if (typeof a === "string" || typeof b === "string") return a === b;
  if ("mark" in a && "mark" in b) return a.mark.mark === b.mark.mark && a.mark.held === b.mark.held;
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

export interface Rect {
  left: number;
  top: number;
  width: number;
  height: number;
}

export interface Pane {
  pane: string;
  at: Rect;
  buffer: Buffer;
  focused: boolean;
  top?: number;
  lines?: number;
  chips?: [number, string, string][];
  coverage?: [number, number, string][];
}

export type Breach =
  | { lineDiffers: { line: number; shown: string; expected: string } }
  | { lineMissing: { line: number; expected: string } }
  | { lineExtra: { line: number; shown: string } }
  | { actionInvented: { offset: number; action: string } }
  | { actionDropped: { offset: number; action: string } };

// @implements REQ-VIEW.rendering_is_total
export function plainText(buffer: Buffer): string[] {
  return buffer.text.split("\n");
}

// @implements REQ-VIEW.affordances_named
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

// @implements REQ-VIEW.frontend_adds_nothing
export function declaredActions(buffer: Buffer): string[] {
  const seen: string[] = [];
  for (const span of buffer.spans) {
    for (const action of span.actions) {
      if (!seen.includes(action)) seen.push(action);
    }
  }
  return seen.sort((a, b) => (a < b ? -1 : a > b ? 1 : 0));
}

export interface Presented {
  painted: string;
  name: string;
}

// @implements REQ-VIEW.presentation_may_be_symbolic
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

// @implements REQ-VIEW.frontend_adds_nothing
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
