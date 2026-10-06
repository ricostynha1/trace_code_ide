// Turn the frontend's TypeScript into what a browser loads.
//
// Node strips the types itself, so this needs nothing installed: no bundler, no
// package manager, no lockfile, and nothing between the source that was checked
// and the file that ships. The only change made to a module is that its
// `./x.ts` imports become `./x.js`, because a browser has no type stripper.
//
// Run: node web/build.mjs
//
// Realises REQ-DRT-TS.runs_the_shipped_source. Written out rather than
// annotated: TraceLean has no grammar for this language and cannot place a
// claim inside the file (ADR-0008).

import { stripTypeScriptTypes } from "node:module";
import { readdirSync, readFileSync, mkdirSync, writeFileSync, copyFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const here = dirname(fileURLToPath(import.meta.url));
const from = join(here, "src");
const into = join(here, "dist");

mkdirSync(into, { recursive: true });

// The entry point the harness runs is not something a browser loads: it reads
// standard input. Everything else is a module the page imports.
const skip = new Set(["frontend.ts"]);

let written = 0;
for (const name of readdirSync(from)) {
  if (!name.endsWith(".ts") || skip.has(name)) continue;
  const source = readFileSync(join(from, name), "utf8");
  const stripped = stripTypeScriptTypes(source, { mode: "strip" });
  const rewritten = stripped.replace(/(from\s+")(\.[^"]*)\.ts(")/g, "$1$2.js$3");
  writeFileSync(join(into, name.replace(/\.ts$/, ".js")), rewritten);
  written += 1;
}

for (const asset of ["index.html", "style.css"]) {
  copyFileSync(join(here, asset), join(into, asset));
}

console.log(`web: ${written} modules and 2 assets into dist/`);
