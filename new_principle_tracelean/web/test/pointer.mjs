// The window's pointer and keys, checked in a real browser.
//
// What a person does with a mouse is decided by the browser — which element
// was under the pointer when it was pressed and released, where a text caret
// falls, whether a right-click reaches the page — and none of that exists in a
// test that calls functions. So this loads the built page (`web/dist`) into
// headless Chrome, stands a recording bridge in for the editor, and sends real
// input events through the DevTools protocol: a click on a file row, a click
// into text, a right-click, a rename typed into the menu, Ctrl+S, a wheel.
// It checks the command each one sent the editor. What the editor then does
// with those commands is `crates/editor/tests/pointer.rs`.
//
// Run: node web/build.mjs && node web/test/pointer.mjs
// Needs Chrome or Chromium on PATH (CHROME=/path/to/chrome to choose).

import { spawn } from "node:child_process";
import { createServer } from "node:http";
import { readFileSync, mkdtempSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join, dirname } from "node:path";
import { fileURLToPath } from "node:url";

const dist = join(dirname(fileURLToPath(import.meta.url)), "..", "dist");

// The stand-in editor: canned answers, and a record of every call.
const BRIDGE = `
<script>
  const listing = "src/a.rs\\nsrc/b.rs";
  const file = "hello world\\nsecond line";
  const buffers = {
    explorer: { id: "dir:.", kind: { directory: { path: "." } }, text: listing,
      spans: [
        { start: 0, stop: 8, role: "path", actions: ["file.open"] },
        { start: 9, stop: 17, role: "path", actions: ["file.open"] } ] },
    document: { id: "file:x.rs", kind: { file: { path: "x.rs" } }, text: file,
      spans: [ { start: 0, stop: 5, role: { token: { kind: "keyword" } }, actions: [] },
        { start: 19, stop: 23, role: "requirement", actions: ["trace.requirement"] } ] },
  };
  const empty = (title) => ({ id: "menu:" + title, kind: { menu: { title } }, text: "", spans: [] });
  window.__calls = [];
  window.__TAURI__ = { core: { invoke: async (command, args) => {
    window.__calls.push([command, args]);
    switch (command) {
      case "theme": return { ui: { document: "#282c34", documentText: "#abb2bf" }, syntax: { keyword: "#c678dd" } };
      case "shown": return [
        { pane: "explorer", at: { left: 0, top: 0, width: 30, height: 10 }, buffer: buffers.explorer, focused: false },
        { pane: "document", at: { left: 30, top: 0, width: 60, height: 10 }, buffer: buffers.document, focused: true,
          top: 0, chips: [[0, "I", "REQ-A.x"]] } ];
      case "cursor": return [0, 6];
      case "bars": return [empty("stations"), { ...empty("opened"), text: "1  x.rs",
        spans: [ { start: 0, stop: 7, role: "entry", actions: ["screen.show"] } ] }];
      case "menu": return null;
      case "offers_here": return ["document", 6, [
        { group: "file", label: "Save x.rs", action: "file.save", target: null, asks: null, keys: "Ctrl+S" } ]];
      case "tick": return false;
      case "take_clipboard": return null;
      case "quick": return ["src/b.rs", "src/a.rs"];
      case "palette": return [["history.tree", "History tree — Space h t"]];
      case "symbols": return [["src/a.rs:3", "alpha  :3"]];
      case "requirement_text": return args.name + ": what it says";
      case "brackets": return [0, 4];
      case "mode": return "Insert";
      case "problems": return 3;
      case "selection": return [6, 11];
      case "selected": return "line";
      case "occurrences": return [3, [0, 6]];
      case "offers": return [
        { group: "here", label: "Open src/b.rs", action: "file.open", target: "src/b.rs", asks: null, keys: "Space f o" },
        { group: "here", label: "Rename src/b.rs…", action: "file.rename", target: "src/b.rs", asks: "New name for src/b.rs", keys: "Space f r" } ];
      default: return "ok";
    }
  } } };
</script>`;

function serve() {
  return new Promise((resolve) => {
    const server = createServer((request, response) => {
      const path = request.url === "/" ? "/index.html" : request.url.split("?")[0];
      try {
        let body = readFileSync(join(dist, path));
        if (path === "/index.html") {
          body = body.toString().replace("<script", BRIDGE + "\n<script");
        }
        const type = path.endsWith(".js") ? "text/javascript" : path.endsWith(".css") ? "text/css" : "text/html";
        // The window's policy for styles (`tauri.conf.json`: `default-src
        // 'self'`): a stylesheet the page writes at run time is refused there,
        // and so it is here. Scripts stay open for the stand-in bridge.
        response.writeHead(200, { "content-type": type, "content-security-policy": "style-src 'self'" });
        response.end(body);
      } catch {
        response.writeHead(404);
        response.end();
      }
    });
    server.listen(0, "127.0.0.1", () => resolve(server));
  });
}

const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

async function devtools(port) {
  for (let i = 0; i < 100; i++) {
    try {
      const pages = await (await fetch(`http://127.0.0.1:${port}/json/list`)).json();
      const page = pages.find((p) => p.type === "page");
      if (page) return page.webSocketDebuggerUrl;
    } catch {}
    await sleep(100);
  }
  throw new Error("Chrome did not open its DevTools port");
}

function connect(url) {
  const socket = new WebSocket(url);
  let id = 0;
  const waiting = new Map();
  socket.onmessage = (message) => {
    const reply = JSON.parse(message.data);
    if (reply.id && waiting.has(reply.id)) {
      waiting.get(reply.id)(reply);
      waiting.delete(reply.id);
    }
  };
  const send = (method, params = {}) =>
    new Promise((resolve, reject) => {
      id += 1;
      waiting.set(id, (reply) => (reply.error ? reject(new Error(JSON.stringify(reply.error))) : resolve(reply.result)));
      socket.send(JSON.stringify({ id, method, params }));
    });
  return new Promise((resolve) => (socket.onopen = () => resolve({ send, close: () => socket.close() })));
}

const failures = [];
function check(name, ok, detail) {
  console.log(`${ok ? "ok  " : "FAIL"} ${name}`);
  if (!ok) failures.push(`${name}: ${detail}`);
}

const server = await serve();
const url = `http://127.0.0.1:${server.address().port}/`;
const profile = mkdtempSync(join(tmpdir(), "tracelean-chrome-"));
const port = 9300 + Math.floor(Math.random() * 500);
const chrome = spawn(
  process.env.CHROME ?? "google-chrome",
  ["--headless=new", `--remote-debugging-port=${port}`, `--user-data-dir=${profile}`,
   "--no-first-run", "--window-size=1200,700", "about:blank"],
  { stdio: "ignore" },
);

try {
  const cdp = await connect(await devtools(port));
  await cdp.send("Page.enable");
  await cdp.send("Runtime.enable");
  await cdp.send("Page.navigate", { url });
  await sleep(1200);

  const evaluate = async (expression) =>
    (await cdp.send("Runtime.evaluate", { expression, returnByValue: true, awaitPromise: true })).result.value;
  // The buffer's text only: a gutter's chips are drawn letters, not text.
  await evaluate(`window.textOnly = { acceptNode: (n) =>
    n.parentElement?.closest(".chips") ? NodeFilter.FILTER_REJECT : NodeFilter.FILTER_ACCEPT }`);
  const calls = async () => evaluate("window.__calls");
  const reset = () => evaluate("window.__calls = []");
  const last = async (command) => (await calls()).filter(([c]) => c === command).pop()?.[1];
  const mouse = async (x, y, button = "left", clickCount = 1) => {
    await cdp.send("Input.dispatchMouseEvent", { type: "mouseMoved", x, y });
    await cdp.send("Input.dispatchMouseEvent", { type: "mousePressed", x, y, button, clickCount });
    await cdp.send("Input.dispatchMouseEvent", { type: "mouseReleased", x, y, button, clickCount });
    await sleep(250);
  };
  // The centre of a character of a pane's text, measured in the page.
  const charAt = (pane, offset) => evaluate(`(() => {
    const box = [...document.querySelectorAll(".pane")][${pane}];
    const walker = document.createTreeWalker(box, NodeFilter.SHOW_TEXT, textOnly);
    let left = ${offset};
    for (let node = walker.nextNode(); node; node = walker.nextNode()) {
      if (left < node.length) {
        const range = document.createRange();
        range.setStart(node, left); range.setEnd(node, left + 1);
        const r = range.getBoundingClientRect();
        return { x: r.left + r.width / 2, y: r.top + r.height / 2 };
      }
      left -= node.length + 0;
    }
    return null;
  })()`);

  check("the page reached the editor", (await calls()).some(([c]) => c === "shown"), JSON.stringify(await calls()));
  const bar = await evaluate(`(() => { const m = document.getElementById("menu"); const b = m.getBoundingClientRect();
    return { shown: !m.hidden && b.height > 0, text: m.textContent, bottom: window.innerHeight - b.bottom }; })()`);
  check("the which-key bar is at the bottom from the start, with the keys for what can be done here",
    bar.shown && bar.text.includes("Ctrl+S") && bar.text.includes("Save x.rs") && bar.bottom < 40, JSON.stringify(bar));

  // A click on the second row of the explorer opens that file, from that pane.
  await reset();
  const row = await evaluate(`(() => { const b = [...document.querySelectorAll(".pane")][0].querySelectorAll("button")[1].getBoundingClientRect(); return { x: b.left + 5, y: b.top + b.height / 2 }; })()`);
  await mouse(row.x, row.y);
  const act = await last("act");
  check("clicking a file row sends file.open for that row, from the explorer",
    act && act.pane === "explorer" && act.action === "file.open" && act.offset === 9, JSON.stringify(await calls()));
  check("the press itself does not redraw (which used to swallow the click)",
    !(await calls()).some(([c]) => c === "grab"), JSON.stringify(await calls()));

  // A click into the document's text places the cursor where it was pressed.
  await reset();
  const w = await charAt(1, 6);
  await mouse(w.x - 2, w.y);
  const placed = await last("place");
  check("clicking into text places the cursor at that character",
    placed && placed.pane === "document" && placed.offset === 6, JSON.stringify(await calls()));

  // Past the end of the second line lands at that line's end.
  await reset();
  const end = await charAt(1, 13);
  await mouse(end.x + 300, end.y);
  const atEnd = await last("place");
  check("clicking past the end of a line places the cursor at its end",
    atEnd && atEnd.offset === "hello world\nsecond line".length, JSON.stringify(atEnd));

  // A right-click on a row lists what can be done there.
  await reset();
  await mouse(row.x, row.y, "right");
  const asked = await last("offers");
  const entries = await evaluate(`[...document.querySelectorAll("#context .context-entry")].map(e => e.textContent)`);
  check("right-clicking a row asks the editor what can be done there",
    asked && asked.pane === "explorer" && asked.offset >= 9 && asked.offset < 17, JSON.stringify(await calls()));
  check("the context menu shows each entry with its keys",
    entries && entries.length === 2 && entries[1].includes("Rename src/b.rs") && entries[1].includes("Space f r"),
    JSON.stringify(entries));
  const docked = await evaluate(`(() => { const b = document.getElementById("context").getBoundingClientRect();
    return { left: b.left, width: b.width, gap: window.innerHeight - b.bottom, page: window.innerWidth }; })()`);
  check("the actions are laid along the bottom, as which-key lays them",
    docked && docked.left === 0 && docked.width === docked.page && docked.gap > 0 && docked.gap < 40, JSON.stringify(docked));

  // Rename asks for the new name in the menu, and sends it.
  await reset();
  const rename = await evaluate(`(() => { const b = document.querySelectorAll("#context .context-entry")[1].getBoundingClientRect(); return { x: b.left + 10, y: b.top + b.height / 2 }; })()`);
  await mouse(rename.x, rename.y);
  const input = await evaluate(`!!document.querySelector("#context input")`);
  check("an entry that needs a name asks for it", input, "no input in the menu");
  await evaluate(`(() => { const i = document.querySelector("#context input"); i.value = "src/c.rs"; })()`);
  await cdp.send("Input.dispatchKeyEvent", { type: "keyDown", key: "Enter", code: "Enter", windowsVirtualKeyCode: 13 });
  await sleep(250);
  const chosen = await last("choose");
  check("the typed name is sent with the entry and its target",
    chosen && chosen.action === "file.rename" && chosen.target === "src/b.rs" && chosen.answer === "src/c.rs",
    JSON.stringify(await calls()));
  check("the menu closes afterwards", !(await evaluate(`!!document.getElementById("context")`)), "still open");

  // Ctrl+S saves.
  await reset();
  await cdp.send("Input.dispatchKeyEvent", { type: "keyDown", key: "s", code: "KeyS", modifiers: 2, windowsVirtualKeyCode: 83 });
  await sleep(250);
  check("Ctrl+S is sent as the save chord", (await last("chord"))?.chord === "C-s", JSON.stringify(await calls()));

  // A selection drawn in a file is handed to the editor before the key that
  // follows, which then acts on it there: typed over, indented, deleted.
  const drawSelection = () => evaluate(`(() => {
    const box = [...document.querySelectorAll(".pane")][1];
    const walker = document.createTreeWalker(box, NodeFilter.SHOW_TEXT, textOnly);
    const first = walker.nextNode();
    const range = document.createRange();
    range.setStart(first, 0); range.setEnd(first, 5);
    const s = window.getSelection(); s.removeAllRanges(); s.addRange(range);
  })()`);
  await reset();
  await drawSelection();
  await cdp.send("Input.dispatchKeyEvent", { type: "keyDown", key: "X", code: "KeyX", text: "X", windowsVirtualKeyCode: 88 });
  await sleep(250);
  const adopted = await last("select");
  const order = (await calls()).map(([c]) => c);
  check("a drawn selection becomes the editor's", adopted && adopted.pane === "document" && adopted.start === 0 &&
    adopted.end === 5 && (await evaluate(`window.getSelection().isCollapsed`)), JSON.stringify(await calls()));
  check("and then the key goes to the editor", order.indexOf("press") > order.indexOf("select"), JSON.stringify(order));

  // The editor's selection is drawn.
  const shownSelection = await evaluate(`document.querySelectorAll(".pane.focused .selection").length`);
  check("the editor's selection is drawn", shownSelection > 0, JSON.stringify(shownSelection));

  // Shift with an arrow is the editor's to select with.
  await reset();
  await cdp.send("Input.dispatchKeyEvent", { type: "keyDown", key: "ArrowRight", code: "ArrowRight", modifiers: 8, windowsVirtualKeyCode: 39 });
  await cdp.send("Input.dispatchKeyEvent", { type: "keyDown", key: "End", code: "End", modifiers: 8, windowsVirtualKeyCode: 35 });
  await sleep(250);
  const extended = (await calls()).filter(([c]) => c === "chord").map(([, a]) => a.chord);
  check("Shift+Right and Shift+End select in the editor", extended.join() === "S-Right,S-End", JSON.stringify(extended));

  // Ctrl+C and Ctrl+A in a file are the editor's.
  await reset();
  await cdp.send("Input.dispatchKeyEvent", { type: "keyDown", key: "a", code: "KeyA", modifiers: 2, windowsVirtualKeyCode: 65 });
  await cdp.send("Input.dispatchKeyEvent", { type: "keyDown", key: "c", code: "KeyC", modifiers: 2, windowsVirtualKeyCode: 67 });
  await sleep(250);
  const copying = (await calls()).filter(([c]) => c === "chord").map(([, a]) => a.chord);
  check("Ctrl+A and Ctrl+C in a file go to the editor", copying.join() === "C-a,C-c", JSON.stringify(copying));

  // Ctrl+P lists matching files, and Enter opens the first.
  await reset();
  await cdp.send("Input.dispatchKeyEvent", { type: "keyDown", key: "p", code: "KeyP", modifiers: 2, windowsVirtualKeyCode: 80 });
  await sleep(250);
  const listed = await evaluate(`[...document.querySelectorAll("#context.quick .context-entry")].map(e => e.textContent)`);
  check("Ctrl+P lists the files the editor matched", listed && listed[0] === "src/b.rs", JSON.stringify(listed));
  await cdp.send("Input.dispatchKeyEvent", { type: "keyDown", key: "Enter", code: "Enter", windowsVirtualKeyCode: 13 });
  await sleep(250);
  const opened = await last("choose");
  check("Enter opens the first match", opened && opened.action === "file.open" && opened.target === "src/b.rs",
    JSON.stringify(await calls()));

  // Ctrl+Shift+P lists actions by name, and Enter does the first here.
  await reset();
  await cdp.send("Input.dispatchKeyEvent", { type: "keyDown", key: "P", code: "KeyP", modifiers: 10, windowsVirtualKeyCode: 80 });
  await sleep(250);
  const actions = await evaluate(`[...document.querySelectorAll("#context.quick .context-entry")].map(e => e.textContent)`);
  check("Ctrl+Shift+P lists actions by what they are called", actions && actions[0] === "History tree — Space h t", JSON.stringify(actions));
  await cdp.send("Input.dispatchKeyEvent", { type: "keyDown", key: "Enter", code: "Enter", windowsVirtualKeyCode: 13 });
  await sleep(250);
  const done = await last("act_here");
  check("Enter does it where the cursor is", done && done.action === "history.tree", JSON.stringify(await calls()));

  // Ctrl+Shift+O lists what the file declares, and Enter goes to it.
  await reset();
  await cdp.send("Input.dispatchKeyEvent", { type: "keyDown", key: "O", code: "KeyO", modifiers: 10, windowsVirtualKeyCode: 79 });
  await sleep(250);
  const names = await evaluate(`[...document.querySelectorAll("#context.quick .context-entry")].map(e => e.textContent)`);
  check("Ctrl+Shift+O lists the file's names", names && names[0] === "alpha  :3", JSON.stringify(names));
  await cdp.send("Input.dispatchKeyEvent", { type: "keyDown", key: "Enter", code: "Enter", windowsVirtualKeyCode: 13 });
  await sleep(250);
  const went = await last("choose");
  check("Enter goes to the name's line", went && went.action === "file.open" && went.target === "src/a.rs:3", JSON.stringify(await calls()));

  // Ctrl+F asks what to find, and Enter sends it.
  await reset();
  await cdp.send("Input.dispatchKeyEvent", { type: "keyDown", key: "f", code: "KeyF", modifiers: 2, windowsVirtualKeyCode: 70 });
  await sleep(150);
  check("Ctrl+F opens a field to type into", await evaluate(`document.activeElement?.matches("#context.find input") ?? false`), "no find field");
  const prefilled = await evaluate(`document.querySelector("#context.find input")?.value`);
  check("the field starts with what is selected", prefilled === "line", JSON.stringify(prefilled));
  await evaluate(`(() => { const i = document.querySelector("#context.find input"); i.value = "line"; })()`);
  await cdp.send("Input.dispatchKeyEvent", { type: "keyDown", key: "Enter", code: "Enter", windowsVirtualKeyCode: 13 });
  await sleep(250);
  const found = await last("find");
  check("Enter sends the text to find, forwards", found && found.text === "line" && found.forward === true,
    JSON.stringify(await calls()));
  // The editor selects what it found ([6, 11] in the stand-in), and the
  // selection is drawn over it.
  const matched = await charAt(1, 6);
  const marked = await evaluate(`(() => { const m = document.querySelector(".pane.focused .selection");
    if (!m) return null; const r = m.getBoundingClientRect(); return { left: r.left, right: r.right }; })()`);
  check("what was found is drawn as selected", marked && marked.left < matched.x && marked.right > matched.x + 10,
    JSON.stringify({ marked, matched }));
  await cdp.send("Input.dispatchKeyEvent", { type: "keyDown", key: "Escape", code: "Escape", windowsVirtualKeyCode: 27 });
  await sleep(100);

  // Ctrl+H adds a field for the replacement; Enter there replaces.
  await reset();
  await cdp.send("Input.dispatchKeyEvent", { type: "keyDown", key: "h", code: "KeyH", modifiers: 2, windowsVirtualKeyCode: 72 });
  await sleep(150);
  const fields = await evaluate(`document.querySelectorAll("#context.find input").length`);
  check("Ctrl+H asks what to find and what to put instead", fields === 2, JSON.stringify(fields));
  await evaluate(`(() => { const [a, b] = document.querySelectorAll("#context.find input"); a.value = "line"; b.value = "row"; b.focus(); })()`);
  await cdp.send("Input.dispatchKeyEvent", { type: "keyDown", key: "Enter", code: "Enter", windowsVirtualKeyCode: 13 });
  await sleep(250);
  const replaced = await last("replace");
  check("Enter in the second field replaces", replaced && replaced.text === "line" && replaced.with === "row",
    JSON.stringify(await calls()));

  // A bracket beside the cursor and its match are both marked.
  const brackets = await evaluate(`document.querySelectorAll(".pane.focused .bracket").length`);
  check("the bracket by the cursor and its match are marked", brackets === 2, JSON.stringify(brackets));
  const occurring = await evaluate(`document.querySelectorAll(".pane.focused .occurrence").length`);
  check("each place the name under the cursor is written is marked", occurring === 2, JSON.stringify(occurring));

  // The cursor's line and column show at the status line's end, and the word
  // and comment chords go to the editor by name.
  const position = await evaluate(`document.getElementById("position")?.textContent`);
  check("the status line shows the findings, mode, line and column", position === "⚠ 3Ln 1, Col 7INSERT", JSON.stringify(position));
  const badge = await evaluate(`document.querySelector("#position .mode-badge.insert")?.textContent`);
  check("the mode is a badge in its own colour", badge === "INSERT", JSON.stringify(badge));
  await reset();
  await evaluate(`document.querySelector("#position .problems").click()`);
  await sleep(200);
  const counted = await last("act_here");
  check("clicking the count lists the findings", counted && counted.action === "trace.findings", JSON.stringify(await calls()));
  await reset();
  await cdp.send("Input.dispatchKeyEvent", { type: "keyDown", key: "ArrowLeft", code: "ArrowLeft", modifiers: 2, windowsVirtualKeyCode: 37 });
  await cdp.send("Input.dispatchKeyEvent", { type: "keyDown", key: "/", code: "Slash", modifiers: 2, windowsVirtualKeyCode: 191 });
  await cdp.send("Input.dispatchKeyEvent", { type: "keyDown", key: "ArrowDown", code: "ArrowDown", modifiers: 1, windowsVirtualKeyCode: 40 });
  await cdp.send("Input.dispatchKeyEvent", { type: "keyDown", key: "Tab", code: "Tab", modifiers: 8, windowsVirtualKeyCode: 9 });
  await sleep(250);
  const sent = (await calls()).filter(([c]) => c === "chord").map(([, a]) => a.chord);
  check("Ctrl+Left, Ctrl+/, Alt+Down and Shift+Tab are sent as chords", sent.join() === "C-Left,C-/,A-Down,S-Tab",
    JSON.stringify(sent));

  // A middle click on a tab shows it and closes it.
  await reset();
  const tab = await evaluate(`(() => { const t = document.querySelector("#strip button.item"); if (!t) return null;
    const r = t.getBoundingClientRect(); return { x: r.x + r.width / 2, y: r.y + r.height / 2 }; })()`);
  if (tab) {
    await cdp.send("Input.dispatchMouseEvent", { type: "mousePressed", x: tab.x, y: tab.y, button: "middle", clickCount: 1 });
    await cdp.send("Input.dispatchMouseEvent", { type: "mouseReleased", x: tab.x, y: tab.y, button: "middle", clickCount: 1 });
    await sleep(250);
  }
  const closing = (await calls()).filter(([c]) => c === "act_in_bar" || c === "chord").map(([c, a]) => a.action ?? a.chord);
  check("a middle click on a tab shows it, then closes it", closing.join() === "screen.show,C-w", JSON.stringify(closing));
  await reset();
  await evaluate(`document.querySelector("#strip button.item .close").click()`);
  await sleep(250);
  const crossed = (await calls()).filter(([c]) => c === "act_in_bar" || c === "chord").map(([c, a]) => a.action ?? a.chord);
  check("a tab's × shows it, then closes it", crossed.join() === "screen.show,C-w", JSON.stringify(crossed));
  await reset();
  const tabAt = await evaluate(`(() => { const r = document.querySelector("#strip button.item").getBoundingClientRect();
    return { x: r.x + 10, y: r.y + r.height / 2 }; })()`);
  await cdp.send("Input.dispatchMouseEvent", { type: "mousePressed", x: tabAt.x, y: tabAt.y, button: "right", clickCount: 1 });
  await cdp.send("Input.dispatchMouseEvent", { type: "mouseReleased", x: tabAt.x, y: tabAt.y, button: "right", clickCount: 1 });
  await sleep(300);
  const tabMenu = (await calls()).map(([c, a]) => c === "act_in_bar" ? a.action : c).filter((c) => c === "screen.show" || c === "offers");
  check("a right-click on a tab shows it and lists what its pane offers",
    tabMenu.join() === "screen.show,offers" && await evaluate(`!!document.getElementById("context")`), JSON.stringify(tabMenu));
  await evaluate(`document.body.click()`);

  // Alt+Left / Alt+Right and the mouse's back and forward buttons go back
  // and forward.
  await reset();
  await cdp.send("Input.dispatchKeyEvent", { type: "keyDown", key: "ArrowLeft", code: "ArrowLeft", modifiers: 1, windowsVirtualKeyCode: 37 });
  await cdp.send("Input.dispatchKeyEvent", { type: "keyDown", key: "ArrowRight", code: "ArrowRight", modifiers: 1, windowsVirtualKeyCode: 39 });
  for (const button of ["back", "forward"]) {
    await cdp.send("Input.dispatchMouseEvent", { type: "mousePressed", x: w.x, y: w.y, button, clickCount: 1 });
    await cdp.send("Input.dispatchMouseEvent", { type: "mouseReleased", x: w.x, y: w.y, button, clickCount: 1 });
  }
  await sleep(250);
  const going = (await calls()).filter(([c]) => c === "chord").map(([, a]) => a.chord);
  check("Alt+arrows and the back/forward buttons go back and forward", going.join() === "A-Left,A-Right,A-Left,A-Right",
    JSON.stringify(going));

  // Resting on a requirement's name shows what it says.
  const named = await evaluate(`(() => { const b = document.querySelector(".pane.kind-file .role-requirement");
    if (!b) return null; const r = b.getBoundingClientRect(); return { x: r.x + r.width / 2, y: r.y + r.height / 2 }; })()`);
  if (named) {
    await cdp.send("Input.dispatchMouseEvent", { type: "mouseMoved", x: named.x, y: named.y });
    await sleep(250);
  }
  const titled = await evaluate(`document.querySelector(".pane.kind-file .role-requirement")?.title ?? null`);
  check("a requirement's name says what it says when rested on", titled?.startsWith("line: what it says"), JSON.stringify(titled));

  // A claim's chip sits in the gutter and opens its requirement, without
  // placing the cursor; the letter is not text a selection copies.
  await reset();
  const chip = await evaluate(`(() => { const c = document.querySelector(".pane.kind-file .chip"); if (!c) return null;
    const r = c.getBoundingClientRect(); return { x: r.x + r.width / 2, y: r.y + r.height / 2, text: c.textContent }; })()`);
  check("a claim is drawn as a chip", chip && chip.text === "I", JSON.stringify(chip));
  if (chip) {
    await cdp.send("Input.dispatchMouseEvent", { type: "mousePressed", x: chip.x, y: chip.y, button: "left", clickCount: 1 });
    await cdp.send("Input.dispatchMouseEvent", { type: "mouseReleased", x: chip.x, y: chip.y, button: "left", clickCount: 1 });
    await sleep(250);
    const opened = await last("choose");
    check("clicking a chip opens its requirement", opened && opened.action === "trace.requirement" && opened.target === "REQ-A.x",
      JSON.stringify(await calls()));
    check("clicking a chip places no cursor", !(await last("place")), JSON.stringify(await calls()));
  }

  // A click on a line's number selects the line.
  await reset();
  const numbered = await evaluate(`(() => { const r = document.querySelectorAll(".pane.kind-file .line")[1];
    const b = r.getBoundingClientRect(); return { x: b.left + 8, y: b.top + b.height / 2, start: Number(r.dataset.start), length: Number(r.dataset.length) }; })()`);
  await cdp.send("Input.dispatchMouseEvent", { type: "mousePressed", x: numbered.x, y: numbered.y, button: "left", clickCount: 1 });
  await cdp.send("Input.dispatchMouseEvent", { type: "mouseReleased", x: numbered.x, y: numbered.y, button: "left", clickCount: 1 });
  await sleep(250);
  const lined = await last("select");
  check("a click on a line's number selects the line",
    lined && lined.start === numbered.start && lined.end === numbered.start + numbered.length + 1 && !(await last("place")),
    JSON.stringify({ numbered, calls: await calls() }));

  // A pane whose lines fit has nothing to scroll across, so no scrollbar.
  const overflowing = await evaluate(`[...document.querySelectorAll(".pane")]
    .filter(p => [...p.children].every(r => r.classList.contains("divider") || r.scrollWidth <= p.clientWidth))
    .filter(p => p.scrollWidth > p.clientWidth).map(p => p.dataset.pane)`);
  check("a pane whose lines fit has no scrollbar", overflowing.length === 0, JSON.stringify(overflowing));

  // A keyword wears the theme's keyword colour, under the window's policy.
  const keyword = await evaluate(`(() => { const k = document.querySelector(".pane.kind-file .token-keyword");
    return k ? getComputedStyle(k).color : null; })()`);
  check("syntax is coloured from the theme in the window", keyword === "rgb(198, 120, 221)", JSON.stringify(keyword));

  // A wheel over the document scrolls it.
  await reset();
  await cdp.send("Input.dispatchMouseEvent", { type: "mouseWheel", x: w.x, y: w.y, deltaX: 0, deltaY: 120 });
  await sleep(250);
  const scrolled = await last("scroll");
  check("the wheel scrolls the pane under it", scrolled && scrolled.pane === "document" && scrolled.lines === 3,
    JSON.stringify(await calls()));

  cdp.close();
} finally {
  chrome.kill();
  server.close();
  await sleep(200);
  rmSync(profile, { recursive: true, force: true });
}

if (failures.length) {
  console.error("\n" + failures.join("\n"));
  process.exit(1);
}
console.log("\nall pointer checks passed");
