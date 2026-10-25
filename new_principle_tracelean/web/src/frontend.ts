import { pieces, render, screenText } from "./render.ts";
import type { Buffer } from "./view.ts";

const painting = process.argv.includes("--paint");
const embroidering = process.argv.includes("--embroider");
const inventing = process.argv.includes("--invent");
const mislabelling = process.argv.includes("--mislabel");
const counting = process.argv.includes("--regions");

function wrongly(drawn: ReturnType<typeof render>): ReturnType<typeof render> {
  const lines = [...drawn.lines];
  if (embroidering && lines.length > 0) lines[0] = "📁 " + lines[0];
  const offered = [...drawn.offered];
  if (inventing) offered.push([0, "file.delete"]);
  return { lines, offered };
}

// @implements REQ-VIEW.frontend_is_checkable
// @implements REQ-VIEW.screen_is_readable
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
