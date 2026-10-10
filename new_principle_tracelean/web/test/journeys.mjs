// Every journey in `tests/journeys.json`, replayed in the window's page in
// headless Chrome, with a real editor behind it (`crates/editor/examples/
// look.rs`, as `look.mjs` uses) on a fresh copy of the demo tree. The terminal
// replays the same file (`crates/tui/tests/driving.rs`); a journey that passes
// in one and fails in the other is the finding (`REQ-LOOK.journeys_replay`).
//
// Run: node web/build.mjs && node web/test/journeys.mjs
// Needs Chrome on PATH (CHROME=/path/to/chrome to choose) and cargo.

import { spawn } from "node:child_process";
import { createServer } from "node:http";
import { cpSync, mkdtempSync, readFileSync, rmSync } from "node:fs";
import { createInterface } from "node:readline";
import { tmpdir } from "node:os";
import { join, dirname, normalize, basename } from "node:path";
import { fileURLToPath } from "node:url";

const here = join(dirname(fileURLToPath(import.meta.url)), "..", "..");
const dist = join(here, "web", "dist");
const { journeys } = JSON.parse(readFileSync(join(here, "tests", "journeys.json"), "utf8"));
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const SKIP = new Set([".tracelean", ".git", "target", ".lake", "node_modules"]);

const BRIDGE = `<script>
  window.__TAURI__ = { core: { invoke: async (command, args) => {
    const reply = await fetch("/invoke/" + command, { method: "POST", body: JSON.stringify(args ?? {}) });
    return reply.json();
  } } };
</script>`;

// The editor once, built before any journey so no journey waits on cargo.
const built = spawn("cargo", ["build", "-q", "-p", "tracelean-editor", "--example", "look"], { cwd: here, stdio: "inherit" });
if ((await new Promise((r) => built.on("exit", r))) !== 0) process.exit(1);

// One editor on a fresh copy of the demo, and a server carrying the page's
// commands to it.
async function session() {
  const copy = join(mkdtempSync(join(tmpdir(), "tracelean-journey-")), "demo");
  cpSync(join(here, "demo"), copy, { recursive: true, filter: (from) => !SKIP.has(basename(from)) });
  const editor = spawn(join(here, "target", "debug", "examples", "look"), [copy], { cwd: here, stdio: ["pipe", "pipe", "inherit"] });
  const replies = createInterface({ input: editor.stdout });
  const waiting = [];
  replies.on("line", (line) => waiting.shift()?.(line));
  let queue = Promise.resolve();
  const ask = (command, args) => (queue = queue.then(() => new Promise((resolve) => {
    waiting.push(resolve);
    editor.stdin.write(JSON.stringify({ command, args }) + "\n");
  })));
  const server = createServer((request, response) => {
    if (request.method === "POST" && request.url.startsWith("/invoke/")) {
      let body = "";
      request.on("data", (chunk) => (body += chunk));
      request.on("end", async () => {
        const reply = await ask(request.url.slice("/invoke/".length), JSON.parse(body || "{}"));
        response.writeHead(200, { "content-type": "application/json" });
        response.end(reply);
      });
      return;
    }
    const path = normalize(request.url === "/" ? "/index.html" : request.url.split("?")[0]);
    try {
      let body = readFileSync(join(dist, path));
      if (path === "/index.html") body = body.toString().replace("<script", BRIDGE + "\n<script");
      const type = path.endsWith(".js") ? "text/javascript" : path.endsWith(".css") ? "text/css" : "text/html";
      response.writeHead(200, { "content-type": type, "content-security-policy": "style-src 'self'" });
      response.end(body);
    } catch {
      response.writeHead(404);
      response.end();
    }
  });
  await new Promise((resolve) => server.listen(0, "127.0.0.1", resolve));
  const close = () => {
    editor.kill();
    server.close();
    rmSync(dirname(copy), { recursive: true, force: true });
  };
  return { url: `http://127.0.0.1:${server.address().port}/`, close };
}

const profile = mkdtempSync(join(tmpdir(), "tracelean-journeys-chrome-"));
const port = 9650 + Math.floor(Math.random() * 100);
const chrome = spawn(process.env.CHROME ?? "google-chrome",
  ["--headless=new", `--remote-debugging-port=${port}`, `--user-data-dir=${profile}`, "--no-first-run",
   "--window-size=1400,800", "about:blank"], { stdio: "ignore" });

const failures = [];
try {
  let socketUrl;
  for (let i = 0; i < 100 && !socketUrl; i++) {
    try {
      const pages = await (await fetch(`http://127.0.0.1:${port}/json/list`)).json();
      socketUrl = pages.find((p) => p.type === "page")?.webSocketDebuggerUrl;
    } catch {}
    if (!socketUrl) await sleep(100);
  }
  const socket = new WebSocket(socketUrl);
  await new Promise((r) => (socket.onopen = r));
  let id = 0;
  const pending = new Map();
  socket.onmessage = (m) => {
    const reply = JSON.parse(m.data);
    if (pending.has(reply.id)) pending.get(reply.id)(reply.result);
  };
  const send = (method, params = {}) => new Promise((resolve) => {
    pending.set(++id, resolve);
    socket.send(JSON.stringify({ id, method, params }));
  });
  const visible = async () =>
    (await send("Runtime.evaluate", { expression: "document.body.innerText", returnByValue: true })).result.value;
  // A key as the keymap names it, as a keyboard sends it to the page.
  const press = async (name) => {
    const ctrl = name.startsWith("C-");
    const key = ctrl ? name.slice(2) : name === "Space" ? " " : name;
    const code = key === " " ? "Space" : key.length === 1 ? `Key${key.toUpperCase()}` : key;
    const text = !ctrl && key.length === 1 ? key : undefined;
    const base = { key, code, modifiers: ctrl ? 2 : 0, windowsVirtualKeyCode: key === "Enter" ? 13 : key === "Escape" ? 27 : key.toUpperCase().charCodeAt(0) };
    await send("Input.dispatchKeyEvent", { type: text ? "keyDown" : "rawKeyDown", ...base, text });
    await send("Input.dispatchKeyEvent", { type: "keyUp", ...base });
    await sleep(300);
  };
  await send("Page.enable");
  await send("Runtime.enable");

  for (const journey of journeys) {
    const { url, close } = await session();
    try {
      await send("Page.navigate", { url });
      await sleep(2000);
      for (const [at, step] of journey.steps.entries()) {
        if (step.keys) for (const key of step.keys) await press(key);
        else if (step.type) { await send("Input.insertText", { text: step.type }); await sleep(400); }
        else if (step.see !== undefined) {
          const seen = await visible();
          if (!seen.includes(step.see)) failures.push(`${journey.name}, step ${at}: \`${step.see}\` is not on the page:\n${seen.slice(0, 600)}`);
        } else if (step.not !== undefined) {
          const seen = await visible();
          if (seen.includes(step.not)) failures.push(`${journey.name}, step ${at}: \`${step.not}\` is on the page`);
        } else failures.push(`${journey.name}, step ${at}: a step this harness cannot take`);
      }
      console.log(`${failures.some((f) => f.startsWith(journey.name)) ? "FAIL" : "ok  "} ${journey.name}`);
    } finally {
      close();
    }
  }
  socket.close();
} finally {
  chrome.kill();
  await sleep(200);
  rmSync(profile, { recursive: true, force: true });
}

if (failures.length) {
  console.error("\n" + failures.join("\n\n"));
  process.exit(1);
}
console.log("\nevery journey replayed in the page");
