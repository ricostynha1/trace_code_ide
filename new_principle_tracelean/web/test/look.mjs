// A photograph of the window's page, drawn by a real editor, in headless
// Chrome — for judging how it looks without opening a window on anybody's
// desktop. The editor is `crates/editor/examples/look.rs`, answering the
// window's commands a JSON line at a time; this serves the page, carries each
// `invoke` to it, and takes the picture. Each step is a command the page
// could send, run before the shot.
//
// Run: node web/build.mjs && node web/test/look.mjs out.png [tree] [step…]
//   a step is `command` or `command={"json":"args"}`, e.g.
//   'choose={"pane":"explorer","offset":0,"action":"file.open","target":"src/celsius.rs"}'
// Needs Chrome on PATH (CHROME=/path/to/chrome to choose) and cargo.

import { spawn } from "node:child_process";
import { createServer } from "node:http";
import { mkdtempSync, readFileSync, writeFileSync } from "node:fs";
import { createInterface } from "node:readline";
import { tmpdir } from "node:os";
import { join, dirname, normalize } from "node:path";
import { fileURLToPath } from "node:url";

const here = join(dirname(fileURLToPath(import.meta.url)), "..", "..");
const dist = join(here, "web", "dist");
const [out = "look.png", tree = "demo", ...steps] = process.argv.slice(2);
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const width = Number(process.env.WIDTH ?? 1400);
const height = Number(process.env.HEIGHT ?? 800);

// The editor, one request a line.
const editor = spawn("cargo", ["run", "-q", "-p", "tracelean-editor", "--example", "look", "--", tree],
  { cwd: here, stdio: ["pipe", "pipe", "inherit"] });
const replies = createInterface({ input: editor.stdout });
const waiting = [];
replies.on("line", (line) => waiting.shift()?.(line));
let queue = Promise.resolve();
const ask = (command, args) => (queue = queue.then(() => new Promise((resolve) => {
  waiting.push(resolve);
  editor.stdin.write(JSON.stringify({ command, args }) + "\n");
})));

const BRIDGE = `<script>
  window.__TAURI__ = { core: { invoke: async (command, args) => {
    const reply = await fetch("/invoke/" + command, { method: "POST", body: JSON.stringify(args ?? {}) });
    return reply.json();
  } } };
</script>`;

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
    // The window's content policy, so what is photographed is what it allows.
    response.writeHead(200, { "content-type": type, "content-security-policy": "style-src 'self'" });
    response.end(body);
  } catch {
    response.writeHead(404);
    response.end();
  }
});
await new Promise((resolve) => server.listen(0, "127.0.0.1", resolve));
const url = `http://127.0.0.1:${server.address().port}/`;

for (const step of steps) {
  const at = step.indexOf("=");
  const [command, args] = at < 0 ? [step, "{}"] : [step.slice(0, at), step.slice(at + 1)];
  console.log(command, "→", (await ask(command, JSON.parse(args))).slice(0, 120));
}

const profile = mkdtempSync(join(tmpdir(), "tracelean-look-"));
const debug = 9800 + Math.floor(Math.random() * 150);
const chrome = spawn(process.env.CHROME ?? "google-chrome",
  ["--headless=new", `--remote-debugging-port=${debug}`, `--user-data-dir=${profile}`, "--no-first-run",
   "--hide-scrollbars", `--window-size=${width},${height}`, "about:blank"], { stdio: "ignore" });

try {
  let socketUrl;
  for (let i = 0; i < 100 && !socketUrl; i++) {
    try {
      const pages = await (await fetch(`http://127.0.0.1:${debug}/json/list`)).json();
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
  await send("Page.enable");
  await send("Emulation.setDeviceMetricsOverride", { width, height, deviceScaleFactor: 1, mobile: false });
  await send("Page.navigate", { url });
  await sleep(2500);
  const shot = await send("Page.captureScreenshot", { format: "png" });
  writeFileSync(out, Buffer.from(shot.data, "base64"));
  console.log("wrote", out);
  socket.close();
} finally {
  chrome.kill();
  editor.kill();
  server.close();
}
