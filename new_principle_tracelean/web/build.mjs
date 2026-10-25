import { stripTypeScriptTypes } from "node:module";
import { readdirSync, readFileSync, mkdirSync, writeFileSync, copyFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const here = dirname(fileURLToPath(import.meta.url));
const from = join(here, "src");
const into = join(here, "dist");

mkdirSync(into, { recursive: true });

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
