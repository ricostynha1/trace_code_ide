// The web frontend, answering the conformance harness.
//
// The same two modes the terminal frontend has, for the same reason: one says
// what was drawn and one is read off what was drawn. Both go through `render`,
// which is the module the page itself builds its DOM from — so a frontend whose
// drawing and whose reporting are two code paths fails the second check.
//
// ```text
// node web/src/frontend.ts --protocol   # answer with what it drew
// node web/src/frontend.ts --paint      # write the text a person would read
// node web/src/frontend.ts --embroider  # decorate the text it was given
// node web/src/frontend.ts --invent     # offer an action nobody declared
// node web/src/frontend.ts --mislabel   # name a region after its own glyph
// node web/src/frontend.ts --regions    # how many regions each line was cut into
// ```
//
// `--regions` is not a rendering: it is the one thing about this frontend that
// the text it draws cannot show. A frontend that compared roles by identity
// rather than by value would cut every character of a span into its own region,
// draw exactly the same text, pass every check here — and put one button under
// each letter. The count is what makes that visible.
//
// The last three draw wrongly on purpose. A conformance suite that has only
// ever seen a correct frontend establishes nothing: it would pass against a
// checker that always said yes.
//
// `--mislabel` is the one that matters to `presentation_may_be_symbolic`. A
// frontend painting an emblem is conformant only because the region it painted
// still carries the buffer's text as its name; a frontend that names the region
// after the glyph has put something on the screen nobody can trace, and the
// capture harness has to say so.
//
// Realises REQ-VIEW.frontend_is_checkable and REQ-VIEW.screen_is_readable.
// What checks it is `crates/core/tests/frontend_conformance_web.rs`, which is
// also where those claims are annotated — this file cannot carry them, because
// TraceLean has no TypeScript grammar to place them with (ADR-0008).

import { pieces, render, screenText } from "./render.ts";
import type { Buffer } from "./view.ts";

const painting = process.argv.includes("--paint");
const embroidering = process.argv.includes("--embroider");
const inventing = process.argv.includes("--invent");
const mislabelling = process.argv.includes("--mislabel");
const counting = process.argv.includes("--regions");

// What a frontend does when it decides it knows better than the buffer.
function wrongly(drawn: ReturnType<typeof render>): ReturnType<typeof render> {
  const lines = [...drawn.lines];
  if (embroidering && lines.length > 0) lines[0] = "📁 " + lines[0];
  const offered = [...drawn.offered];
  if (inventing) offered.push([0, "file.delete"]);
  return { lines, offered };
}

// The markers the capture harness reads: a clear at the front and a NUL at the
// end, so one painted screen can be told from the next. A web view has no
// escape sequences of its own; these are the harness's frame, not the page's,
// and they are built by code point because a control character written into a
// source file is a character nobody reading it can see.
const ESCAPE = String.fromCharCode(27);
const CLEAR = ESCAPE + "[2J";
const RESET = ESCAPE + "[0m";
const END = String.fromCharCode(0);

function screen(buffer: Buffer): string {
  return CLEAR + screenText(buffer, mislabelling) + "\n" + RESET + END;
}

let held = "";
process.stdin.on("data", (chunk) => {
  held += chunk;
  let at: number;
  while ((at = held.indexOf("\n")) >= 0) {
    const line = held.slice(0, at);
    held = held.slice(at + 1);
    if (line.trim() === "") continue;

    let asked: { case?: number; input?: unknown };
    try {
      asked = JSON.parse(line);
    } catch (e) {
      process.stdout.write(JSON.stringify({ case: 0, error: String(e) }) + "\n");
      continue;
    }
    const buffer = asked.input as Buffer;
    if (painting) {
      process.stdout.write(screen(buffer));
      continue;
    }
    // A throw is this case's error, not the end of the run: the harness is
    // entitled to a verdict about every buffer it sent.
    try {
      const output = counting
        ? pieces(buffer).map((line) => line.length)
        : wrongly(render(buffer));
      process.stdout.write(JSON.stringify({ case: asked.case ?? 0, output }) + "\n");
    } catch (e) {
      const message = e instanceof Error ? e.message : String(e);
      process.stdout.write(
        JSON.stringify({ case: asked.case ?? 0, error: message }) + "\n",
      );
    }
  }
});
