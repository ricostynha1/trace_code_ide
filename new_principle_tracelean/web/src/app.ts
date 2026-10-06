// The web frontend's page.
//
// It draws a buffer and sends actions back. It holds no state about what is
// shown, keeps no view model, and builds no buffer: the editor answers with a
// buffer, this renders it, a click names an action, and the editor answers with
// the next buffer.
//
// The rendering comes from `render.ts`, which is the module the conformance
// harness checks — so what a person sees here is what that harness verified.
//
// Realises REQ-VIEW.one_representation and REQ-SHOW.core_produces. Written out
// rather than annotated: TraceLean has no TypeScript grammar and cannot place a
// claim inside this file (ADR-0008).

import { classOf, pieces, shown } from "./render.ts";
import type { Buffer, Pane } from "./view.ts";

// One text line's height, in pixels. It matches the stylesheet, and it has to:
// the editor lays panes out in characters, so the page converts to pixels with
// the same number the stylesheet uses or the boxes stop lining up with the rows.
const LINE = 21;

// How the page reaches the editor. In a Tauri window this is the bridge to the
// Rust core; anywhere else there is no editor, and the page says so rather than
// inventing one.
type Invoke = (command: string, args: Record<string, unknown>) => Promise<unknown>;

function bridge(): Invoke | null {
  const tauri = (globalThis as Record<string, any>).__TAURI__;
  return tauri?.core?.invoke ?? null;
}

// How big the page is, in characters. The editor lays the panes out in this
// region and windows each buffer to its pane, so the page still draws
// everything it is given.
//
// The width is measured rather than assumed: a monospace character's width
// depends on the font the browser actually loaded, and a number guessed here
// would put every pane's box a little out of step with the rows inside it.
function region(into: HTMLElement): { width: number; height: number } {
  const ruler = document.createElement("span");
  ruler.style.position = "absolute";
  ruler.style.visibility = "hidden";
  ruler.style.whiteSpace = "pre";
  ruler.textContent = "0".repeat(100);
  into.appendChild(ruler);
  const character = ruler.getBoundingClientRect().width / 100 || 8;
  ruler.remove();
  return {
    width: Math.max(1, Math.floor(into.clientWidth / character)),
    height: Math.max(1, Math.floor(into.clientHeight / LINE)),
  };
}

// Draw a buffer into an element.
//
// One `<div>` a line, one `<span>` a piece, and a `<button>` wherever a piece
// carries an action. The button's label is the piece's own text, because a
// label this frontend made up would be text the buffer does not contain.
//
// Every line and piece records the offset it starts at, so a pointer anywhere
// in the pane can be turned back into a position in the buffer — which is all
// the editor needs to place a cursor or list what can be done there.
export function draw(into: HTMLElement, buffer: Buffer, act: (action: string, at: number) => void) {
  into.textContent = "";
  let start = 0;
  for (const line of pieces(buffer)) {
    const row = document.createElement("div");
    row.className = "line";
    const length = line.reduce((sum, piece) => sum + Array.from(piece.text).length, 0);
    row.dataset.start = String(start);
    row.dataset.length = String(length);
    start += length + 1;
    if (line.length === 0) {
      // An empty line is a line: a row with nothing in it still takes space,
      // and dropping it would renumber everything below.
      row.appendChild(document.createTextNode(""));
    }
    for (const piece of line) {
      if (piece.actions.length === 0) {
        const span = document.createElement("span");
        span.className = classOf(piece.role);
        span.dataset.at = String(piece.at);
        span.textContent = piece.text;
        row.appendChild(span);
        continue;
      }
      const button = document.createElement("button");
      button.className = `${classOf(piece.role)} actionable`;
      button.dataset.at = String(piece.at);
      // What it does, for the style sheet: a toggle is drawn as one.
      button.dataset.action = piece.actions[0];
      button.textContent = piece.text;
      button.title = `${piece.actions.map((a) => hover[a] ?? a).join(" · ")}  (right-click for more)`;
      // A requirement's name says what the requirement says, when rested on.
      if (button.classList.contains("role-requirement")) {
        button.addEventListener("mouseenter", () => void sayRequirement(button, piece.text.trim()), { once: true });
      }
      button.addEventListener("click", (event) => {
        event.stopPropagation();
        act(piece.actions[0], piece.at);
      });
      row.appendChild(button);
    }
    into.appendChild(row);
  }
}

// The position in a drawn buffer under a pointer, in characters.
//
// The browser says which text node and which UTF-16 index the pointer is over;
// the piece it belongs to says where that piece starts. Past the end of a line,
// or on an empty one, the position is the line's end.
function offsetAt(event: MouseEvent): number | null {
  const doc = document as Document & {
    caretRangeFromPoint?: (x: number, y: number) => Range | null;
    caretPositionFromPoint?: (x: number, y: number) => { offsetNode: Node; offset: number } | null;
  };
  let node: Node | null = null;
  let index = 0;
  const range = doc.caretRangeFromPoint?.(event.clientX, event.clientY);
  if (range) {
    node = range.startContainer;
    index = range.startOffset;
  } else {
    const position = doc.caretPositionFromPoint?.(event.clientX, event.clientY);
    if (position) {
      node = position.offsetNode;
      index = position.offset;
    }
  }
  const target = event.target instanceof Element ? event.target : null;
  const host = node?.parentElement?.closest("[data-at]") ?? target?.closest("[data-at]");
  const row = (host ?? target)?.closest(".line") as HTMLElement | null;
  if (host instanceof HTMLElement && node && node.nodeType === Node.TEXT_NODE && host.contains(node)) {
    const characters = Array.from((node.textContent ?? "").slice(0, index)).length;
    return Number(host.dataset.at) + characters;
  }
  if (row) return Number(row.dataset.start) + Number(row.dataset.length);
  return null;
}

// A position in a drawn buffer, from a DOM position: the text node and index a
// selection or a caret reports, turned into characters from the window's start.
function positionOf(node: Node, index: number): number | null {
  const element = node.nodeType === Node.TEXT_NODE ? node.parentElement : (node as Element);
  const host = element?.closest("[data-at]");
  if (host instanceof HTMLElement && node.nodeType === Node.TEXT_NODE && host.contains(node)) {
    return Number(host.dataset.at) + Array.from((node.textContent ?? "").slice(0, index)).length;
  }
  const row = element?.closest(".line") as HTMLElement | null;
  if (!row) return null;
  // An element boundary: before its first child is the line's start.
  return index === 0 ? Number(row.dataset.start) : Number(row.dataset.start) + Number(row.dataset.length);
}

// The pointer's selection inside one file pane, as positions there; `null`
// when nothing is selected or the selection is not in a file.
function selectedInFile(): { pane: string; start: number; end: number; text: string } | null {
  const selection = window.getSelection();
  if (!selection || selection.isCollapsed || !selection.anchorNode || !selection.focusNode) return null;
  const box = selection.anchorNode.parentElement?.closest(".pane") as HTMLElement | null;
  if (!box || !box.classList.contains("kind-file") || !box.contains(selection.focusNode)) return null;
  const a = positionOf(selection.anchorNode, selection.anchorOffset);
  const b = positionOf(selection.focusNode, selection.focusOffset);
  if (a === null || b === null || a === b) return null;
  return { pane: box.dataset.pane ?? "", start: Math.min(a, b), end: Math.max(a, b), text: selection.toString() };
}

// The DOM position of a character offset in a drawn pane: the text node that
// holds it and the index inside that node.
function domAt(box: HTMLElement, offset: number): { node: Node; index: number } | null {
  for (const host of Array.from(box.querySelectorAll<HTMLElement>("[data-at]"))) {
    const start = Number(host.dataset.at);
    const text = host.firstChild;
    if (!text || text.nodeType !== Node.TEXT_NODE) continue;
    const characters = Array.from(text.textContent ?? "");
    if (offset >= start && offset <= start + characters.length) {
      return { node: text, index: characters.slice(0, offset - start).join("").length };
    }
  }
  return null;
}

// A selection the pointer drew in a file, handed to the editor, which keeps
// it from then on — through scrolling, past what is on screen — and draws it.
// Answers whether there was one.
async function adoptSelection(): Promise<boolean> {
  const chosen = selectedInFile();
  if (!chosen || !ctx) return false;
  window.getSelection()?.removeAllRanges();
  ctx.status.textContent = (await ctx.invoke("select", { pane: chosen.pane, start: chosen.start, end: chosen.end })) as string;
  return true;
}

// Draw one pane: a box where the editor put it, with the buffer drawn inside.
//
// The box is positioned in characters and line heights, which are the units the
// editor laid the pane out in. A window spends nothing on dividers — the border
// is drawn in the box's own edge by the stylesheet — where a terminal has to
// take a column from the pane, which is why the two frontends ask for different
// amounts of room and draw the same buffers.
//
// The pointer does what it does in any editor: a click places the cursor (and
// focuses the pane), a click on an affordance performs it, a right-click lists
// everything that can be done there, a wheel scrolls, and a drag selects text
// to copy. Nothing is redrawn on the press itself — a redraw between press and
// release replaces the element under the pointer, and the browser then never
// delivers the click.
// The claims beside a file's lines, in its gutter: one letter a claim, which
// opens the requirement it claims. Which lines and letters is the editor's
// (`surface::chips`); the colour of each role is the theme's.
const CLAIMED: Record<string, string> = { M: "models", I: "implements", T: "tests", D: "drt", P: "proves" };

function drawChips(box: HTMLElement, pane: Pane) {
  for (const [line, letter, requirement] of pane.chips ?? []) {
    const row = box.children[line - (pane.top ?? 0)];
    if (!(row instanceof HTMLElement)) continue;
    let holder = row.querySelector(".chips");
    if (!holder) {
      holder = document.createElement("span");
      holder.className = "chips";
      row.prepend(holder);
    }
    const chip = document.createElement("button");
    chip.className = `chip chip-${CLAIMED[letter] ?? "other"}`;
    chip.textContent = letter;
    chip.title = `${CLAIMED[letter] ?? letter} ${requirement}`;
    // Pointed at, it says what the clause it claims says.
    chip.addEventListener("mouseenter", () => void sayRequirement(chip, requirement), { once: true });
    chip.addEventListener("mousedown", (event) => event.stopPropagation());
    chip.addEventListener("click", (event) => {
      event.stopPropagation();
      void run("choose", { pane: pane.pane, offset: 0, action: "trace.requirement", target: requirement, answer: null });
    });
    holder.appendChild(chip);
  }
}

// Whether the last press and release was a drag, so the click that follows
// it is not taken as a click to place the cursor.
let dragged = false;

function drawPane(into: HTMLElement, pane: Pane) {
  const box = document.createElement("div");
  // The buffer's kind becomes a class, so a listing can look like a sidebar
  // beside a file. It says what the pane holds, as a role does; the stylesheet
  // decides what that looks like.
  const kind = Object.keys(pane.buffer.kind)[0] ?? "file";
  box.className = `pane kind-${kind}${pane.focused ? " focused" : ""}`;
  box.dataset.pane = pane.pane;
  box.style.left = `${pane.at.left}ch`;
  box.style.top = `${pane.at.top * LINE}px`;
  box.style.width = `${pane.at.width}ch`;
  box.style.height = `${pane.at.height * LINE}px`;
  draw(box, pane.buffer, (action, at) => run("act", { pane: pane.pane, action, offset: at }));
  // A file's lines are numbered in a gutter the stylesheet draws from this
  // attribute: a number is where a line is, not text the buffer holds.
  if (kind === "file") {
    Array.from(box.children).forEach((row, index) => {
      if (row instanceof HTMLElement) row.dataset.number = String((pane.top ?? 0) + index + 1);
    });
    drawChips(box, pane);
  }

  // A drag in a file ends as the editor's selection, drawn by it from then on.
  box.addEventListener("mouseup", () => {
    if (kind !== "file" || !selectedInFile()) return;
    dragged = true;
    void adoptSelection().then(() => ctx && show(ctx.invoke, ctx.into, ctx.status));
  });
  box.addEventListener("click", (event) => {
    // A drag that selected text is a selection, not a click to place.
    const selection = window.getSelection();
    if (dragged || (selection && !selection.isCollapsed)) {
      dragged = false;
      return;
    }
    // A click on a line's number selects the line.
    const line = event.target instanceof Element ? (event.target.closest(".line") as HTMLElement | null) : null;
    if (kind === "file" && line && event.clientX < line.getBoundingClientRect().left + parseFloat(getComputedStyle(line).paddingLeft)) {
      const start = Number(line.dataset.start);
      void run("select", { pane: pane.pane, start, end: start + Number(line.dataset.length) + 1 });
      return;
    }
    const at = offsetAt(event);
    // Ctrl+click: to where the name clicked is declared, as F12 goes.
    const definition = event.ctrlKey || event.metaKey;
    void run("place", { pane: pane.pane, offset: at ?? 0 }).then(() =>
      definition ? run("chord", { chord: "F12" }) : undefined
    );
  });
  box.addEventListener("contextmenu", (event) => {
    event.preventDefault();
    const at = offsetAt(event) ?? 0;
    void openContext(pane.pane, at, event.clientX, event.clientY);
  });
  let wheeling = 0;
  box.addEventListener(
    "wheel",
    (event) => {
      // Sideways (a trackpad, or Shift with the wheel) is the browser's: a
      // line longer than the pane scrolls across where it is drawn.
      if (event.shiftKey || Math.abs(event.deltaX) > Math.abs(event.deltaY)) return;
      event.preventDefault();
      wheeling += event.deltaY;
      const lines = Math.trunc(wheeling / 40);
      if (lines === 0) return;
      wheeling -= lines * 40;
      void run("scroll", { pane: pane.pane, lines });
    },
    { passive: false },
  );

  // The divider on the right edge, draggable. What it sends is a number of
  // characters, so from the editor's side a drag and a keypress are the same
  // change with different amounts.
  const handle = document.createElement("div");
  handle.className = "divider";
  handle.addEventListener("click", (event) => event.stopPropagation());
  handle.addEventListener("mousedown", (event) => {
    event.preventDefault();
    event.stopPropagation();
    const from = event.clientX;
    const character = box.getBoundingClientRect().width / Math.max(1, pane.at.width);
    const release = (up: MouseEvent) => {
      document.removeEventListener("mouseup", release);
      const moved = Math.round((up.clientX - from) / character);
      if (moved !== 0) void run("grab", { pane: pane.pane, amount: moved });
    };
    document.addEventListener("mouseup", release);
  });
  box.appendChild(handle);
  into.appendChild(box);
}

// A bar's buffer, drawn as a row of items rather than a column of lines.
//
// The rows are the buffer's own and nothing is added between them. What this
// frontend does that the terminal cannot is lay them along a row and let a
// pointer press one, which is the same affordance the span already carried.
//
// A station is painted as its emblem *instead of* its name, which is what
// `presentation_may_be_symbolic` allows and what makes a bar readable at a
// glance. The name does not disappear: it becomes the item's accessible name,
// which is what a screen reader announces and what the capture harness reads
// off the page. The glyph is marked hidden so that nothing reads it twice.
//
// The decision is `shown`'s and not this function's: the same value the capture
// harness reads through `screenText` says what to paint and what to name it, so
// the page and the report cannot come apart. Which glyph is chosen from the
// actions the core declared — never from the text. A frontend recognising its
// own strings would be a second answer to what is on screen.
function drawBar(
  into: HTMLElement | null,
  buffer: Buffer,
  station: boolean,
  act: (station: boolean, action: string, at: number) => void,
) {
  if (!into) return;
  into.textContent = "";
  for (const line of shown(buffer)) {
    for (const { piece, presented } of line) {
      const item = document.createElement(piece.actions.length ? "button" : "span");
      item.className = `${classOf(piece.role)} item`;
      if (presented.painted === presented.name) {
        item.appendChild(document.createTextNode(presented.name));
      } else {
        const mark = document.createElement("span");
        mark.className = "emblem";
        mark.setAttribute("aria-hidden", "true");
        mark.textContent = presented.painted;
        item.appendChild(mark);
        item.setAttribute("aria-label", presented.name);
      }
      if (piece.actions.length) {
        item.title = presented.name.trim();
        item.addEventListener("click", () => act(station, piece.actions[0], piece.at));
        // A middle click or the × closes a tab: shown, then closed, as
        // Ctrl+W does.
        if (!station && piece.actions[0] === "screen.show") {
          const close = () =>
            void (ctx?.invoke("act_in_bar", { station, action: "screen.show", offset: piece.at }) ?? Promise.resolve())
              .then(() => run("chord", { chord: "C-w" }));
          item.addEventListener("auxclick", (event) => {
            if (event.button !== 1) return;
            event.preventDefault();
            close();
          });
          // A right-click shows the tab, then lists what its pane offers.
          item.addEventListener("contextmenu", (event) => {
            event.preventDefault();
            event.stopPropagation();
            const [x, y] = [event.clientX, event.clientY];
            void run("act_in_bar", { station, action: "screen.show", offset: piece.at }).then(() => {
              const pane = document.querySelector<HTMLElement>(".pane.focused")?.dataset.pane;
              if (pane) void openContext(pane, 0, x, y);
            });
          });
          const cross = document.createElement("span");
          cross.className = "close";
          cross.setAttribute("aria-hidden", "true");
          cross.textContent = "×";
          cross.title = "Close (Ctrl+W)";
          cross.addEventListener("click", (event) => {
            event.stopPropagation();
            close();
          });
          item.appendChild(cross);
        }
      }
      into.appendChild(item);
    }
  }
}

// One entry of a context menu, as the core offers it.
interface Offer {
  group: string;
  label: string;
  action: string;
  target: string | null;
  asks: string | null;
  keys: string | null;
}

// The headings of a context menu's groups. Chrome rather than content: the
// entries themselves are the core's.
const GROUPS: Record<string, string> = {
  here: "Here",
  file: "This buffer",
  panes: "Panes",
  go: "Go to",
};

function closeContext() {
  document.getElementById("context")?.remove();
}

// Everything the core says can be done where the pointer is, laid along the
// bottom of the window as which-key lays out what a prefix leads to: a column
// per group, each entry its keys and then what it does, so the keys are learnt
// by reading them.
//
// An entry that needs an argument — a new name, a new path — asks for it in the
// menu itself, and the answer goes to the editor with the entry.
async function openContext(pane: string, at: number, _x: number, _y: number) {
  closeContext();
  if (!ctx) return;
  const offered = (await ctx.invoke("offers", { pane, offset: at })) as Offer[];
  document.body.appendChild(overlay(offered, pane, at));
}

// The box laid over the bottom of the window: the offers, or a question one
// of them asks.
function overlay(offered: Offer[], pane: string, at: number): HTMLElement {
  const menu = document.createElement("div");
  menu.id = "context";
  menu.className = "which-key";
  menu.setAttribute("role", "menu");
  whichKey(menu, offered, pane, at);
  return menu;
}

// Offers as which-key columns, one per group, keys first. A click chooses the
// entry where it was offered; one that asks for a name asks in a box over the
// bottom of the window.
function whichKey(into: HTMLElement, offered: Offer[], pane: string, at: number) {
  let group = "";
  let column: HTMLElement = into;
  for (const offer of offered) {
    if (offer.group !== group) {
      group = offer.group;
      column = document.createElement("div");
      column.className = "context-column";
      into.appendChild(column);
      const heading = document.createElement("div");
      heading.className = "context-group";
      heading.textContent = GROUPS[group] ?? group;
      column.appendChild(heading);
    }
    const entry = document.createElement("button");
    entry.className = "context-entry";
    entry.setAttribute("role", "menuitem");
    const keys = document.createElement("span");
    keys.className = "context-keys";
    keys.textContent = offer.keys ?? "";
    entry.appendChild(keys);
    const label = document.createElement("span");
    label.textContent = offer.label;
    entry.appendChild(label);
    entry.addEventListener("click", (event) => {
      event.stopPropagation();
      if (offer.asks) {
        let menu = document.getElementById("context");
        if (!menu) {
          menu = overlay([], pane, at);
          document.body.appendChild(menu);
        }
        ask(menu, offer, pane, at);
        return;
      }
      closeContext();
      void run("choose", { pane, offset: at, action: offer.action, target: offer.target, answer: null });
    });
    column.appendChild(entry);
  }
}

// Ask for an entry's argument inside the menu.
function ask(menu: HTMLElement, offer: Offer, pane: string, at: number) {
  menu.textContent = "";
  const question = document.createElement("div");
  question.className = "context-group";
  question.textContent = offer.asks ?? "";
  const input = document.createElement("input");
  input.className = "context-input";
  input.value = offer.target ?? "";
  const done = () => {
    const answer = input.value.trim();
    closeContext();
    if (answer) {
      void run("choose", { pane, offset: at, action: offer.action, target: offer.target, answer });
    }
  };
  input.addEventListener("keydown", (event) => {
    event.stopPropagation();
    if (event.key === "Enter") done();
    if (event.key === "Escape") closeContext();
  });
  input.addEventListener("click", (event) => event.stopPropagation());
  const ok = document.createElement("button");
  ok.className = "context-entry";
  ok.textContent = offer.label.replace(/…$/, "");
  ok.addEventListener("click", (event) => {
    event.stopPropagation();
    done();
  });
  menu.append(question, input, ok);
  input.focus();
  input.select();
}

// Lay a box of class `kind` over `length` characters of a drawn pane — the
// selection, a bracket pair. Boxes over the text rather than a change to it,
// so they go with the next redraw; a run past the window ends where it does.
function mark(box: HTMLElement, start: number, length: number, kind: string) {
  // Piece by piece of text, so a run over several lines marks the text and
  // not the rows' whole boxes, gutter and all.
  const stop = start + length;
  const rects: DOMRect[] = [];
  for (const host of Array.from(box.querySelectorAll<HTMLElement>("[data-at]"))) {
    const text = host.firstChild;
    if (!text || text.nodeType !== Node.TEXT_NODE) continue;
    const characters = Array.from(text.textContent ?? "");
    const at = Number(host.dataset.at);
    const [from, to] = [Math.max(start, at), Math.min(stop, at + characters.length)];
    if (from >= to) continue;
    const range = document.createRange();
    range.setStart(text, characters.slice(0, from - at).join("").length);
    range.setEnd(text, characters.slice(0, to - at).join("").length);
    rects.push(...Array.from(range.getClientRects()));
  }
  const origin = box.getBoundingClientRect();
  for (const rect of rects) {
    const laid = document.createElement("div");
    laid.className = kind;
    laid.style.left = `${rect.left - origin.left + box.scrollLeft}px`;
    laid.style.top = `${rect.top - origin.top}px`;
    laid.style.width = `${rect.width}px`;
    laid.style.height = `${rect.height}px`;
    box.appendChild(laid);
  }
}

// Ctrl+F: a field at the top of the page. Enter finds the next match, Shift+
// Enter the one before, Escape closes it. Ctrl+Shift+F: the same field, and
// Enter lists every line of the project holding the text. Ctrl+H: a second
// field under it, where Enter replaces every match in the file. The searching
// and replacing are the editor's.
function openFind(everywhere = false, replacing = false) {
  closeContext();
  const box = document.createElement("div");
  box.id = "context";
  box.className = "find";
  const input = document.createElement("input");
  input.className = "context-input";
  input.placeholder = everywhere ? "Find in every file" : "Find in this buffer";
  input.addEventListener("keydown", (event) => {
    event.stopPropagation();
    if (event.key === "Escape") closeContext();
    if (event.key === "Enter" && input.value && everywhere) {
      const text = input.value;
      closeContext();
      void run("search", { text });
      return;
    }
    // The editor selects what it finds, and the selection is drawn.
    if (event.key === "Enter" && input.value) {
      void run("find", { text: input.value, forward: !event.shiftKey }).then(() => input.focus());
    }
  });
  input.addEventListener("click", (event) => event.stopPropagation());
  box.addEventListener("click", (event) => event.stopPropagation());
  box.appendChild(input);
  if (replacing) {
    const swap = document.createElement("input");
    swap.className = "context-input";
    swap.placeholder = "Replace every match with (Enter)";
    swap.addEventListener("keydown", (event) => {
      event.stopPropagation();
      if (event.key === "Escape") closeContext();
      if (event.key === "Enter" && input.value) {
        const [text, with_] = [input.value, swap.value];
        closeContext();
        void run("replace", { text, with: with_ });
      }
    });
    swap.addEventListener("click", (event) => event.stopPropagation());
    box.appendChild(swap);
  }
  document.body.appendChild(box);
  input.focus();
  // What is selected on one line is what is looked for, to start with.
  void ctx?.invoke("selected", {}).then((text) => {
    if (typeof text !== "string" || !text || text.includes("\n") || input.value) return;
    input.value = text;
    input.select();
  });
}

// Ctrl+P: type a few letters of a file's path, pick from what matches. The
// matching is the editor's; Enter opens the first (or the one moved to with
// the arrows), a click opens the one clicked, Escape closes.
function openQuick() {
  openChooser("Open a file by name — name:42 or :42 for a line", async (query) => {
    const found = ((await ctx?.invoke("quick", { query })) as string[] | null) ?? [];
    return found.map((path) => ({
      label: path,
      pick: () => run("choose", { pane: "explorer", offset: 0, action: "file.open", target: path, answer: null }),
    }));
  });
}

// Ctrl+Shift+P: every action by what it is called, the editor's matches for
// what is typed; the one picked is done where the cursor is.
function openPalette() {
  openChooser("Do something — type a few words of it", async (query) => {
    const found = ((await ctx?.invoke("palette", { query })) as [string, string][] | null) ?? [];
    return found.map(([action, said]) => ({ label: said, pick: () => run("act_here", { action }) }));
  });
}

// Ctrl+Shift+O: what the file declares, by a few letters of its name; the
// one picked is gone to.
function openSymbols() {
  openChooser("Go to a name this file declares", async (query) => {
    const found = ((await ctx?.invoke("symbols", { query })) as [string, string][] | null) ?? [];
    return found.map(([target, label]) => ({
      label,
      pick: () => run("choose", { pane: "explorer", offset: 0, action: "file.open", target, answer: null }),
    }));
  });
}

// A field over a list: what is typed asks for the list again, the arrows
// move through it, Enter or a click picks, Escape closes.
function openChooser(placeholder: string, lookup: (query: string) => Promise<{ label: string; pick: () => Promise<void> }[]>) {
  closeContext();
  if (!ctx) return;
  const box = document.createElement("div");
  box.id = "context";
  box.className = "find quick";
  const input = document.createElement("input");
  input.className = "context-input";
  input.placeholder = placeholder;
  const list = document.createElement("div");
  let found: { label: string; pick: () => Promise<void> }[] = [];
  let chosen = 0;
  const open = (entry: { pick: () => Promise<void> }) => {
    closeContext();
    void entry.pick();
  };
  const draw = () => {
    list.textContent = "";
    found.forEach((one, index) => {
      const entry = document.createElement("button");
      entry.className = `context-entry${index === chosen ? " chosen" : ""}`;
      entry.textContent = one.label;
      entry.addEventListener("click", (event) => {
        event.stopPropagation();
        open(one);
      });
      list.appendChild(entry);
    });
  };
  const ask = async () => {
    found = await lookup(input.value);
    chosen = 0;
    draw();
  };
  input.addEventListener("input", () => void ask());
  input.addEventListener("keydown", (event) => {
    event.stopPropagation();
    if (event.key === "Escape") closeContext();
    if (event.key === "ArrowDown" || event.key === "ArrowUp") {
      event.preventDefault();
      const step = event.key === "ArrowDown" ? 1 : -1;
      chosen = Math.max(0, Math.min(found.length - 1, chosen + step));
      draw();
    }
    if (event.key === "Enter" && found[chosen]) open(found[chosen]);
  });
  input.addEventListener("click", (event) => event.stopPropagation());
  box.addEventListener("click", (event) => event.stopPropagation());
  box.append(input, list);
  document.body.appendChild(box);
  input.focus();
  void ask();
}

// What each action is called and the keys that reach it, from the keymap,
// for the text a pointer's hover shows.
let hover: Record<string, string> = {};

// What each requirement name says, asked of the editor once per name.
const requirementTexts = new Map<string, string | null>();

async function sayRequirement(button: HTMLElement, name: string) {
  if (!ctx) return;
  if (!requirementTexts.has(name)) {
    requirementTexts.set(name, (await ctx.invoke("requirement_text", { name })) as string | null);
  }
  const said = requirementTexts.get(name);
  if (said) button.title = `${said}\n\n${button.title}`;
}

// What the page needs to reach the editor and redraw, once it has started.
let ctx: { invoke: Invoke; into: HTMLElement; status: HTMLElement } | null = null;

// Send one command, show what the editor said, and draw the answer.
async function run(command: string, args: Record<string, unknown>) {
  if (!ctx) return;
  const said = (await ctx.invoke(command, args)) as string;
  ctx.status.textContent = said;
  await copyIfAsked(ctx.invoke);
  await show(ctx.invoke, ctx.into, ctx.status);
}

// Put on the clipboard whatever the editor asked to — the sandbox command.
// The asynchronous clipboard first; where the web view refuses it, the older
// copy of a selected text field, which every web view still honours.
async function copyIfAsked(invoke: Invoke) {
  const text = (await invoke("take_clipboard", {})) as string | null;
  if (!text) return;
  try {
    await navigator.clipboard.writeText(text);
    return;
  } catch {
    // fall through
  }
  const field = document.createElement("textarea");
  field.value = text;
  field.style.position = "fixed";
  field.style.opacity = "0";
  document.body.appendChild(field);
  field.select();
  document.execCommand("copy");
  field.remove();
}

// While an agent may be working, look again every second and a half, and
// redraw only when the editor says something changed. Not while a context
// menu is open or text is being selected: a redraw would take either away.
function poll() {
  setInterval(async () => {
    if (!ctx || document.getElementById("context")) return;
    const selection = window.getSelection();
    if (selection && !selection.isCollapsed) return;
    const changed = (await ctx.invoke("tick", {})) as string | boolean | null;
    if (!changed) return;
    if (typeof changed === "string") ctx.status.textContent = changed;
    await show(ctx.invoke, ctx.into, ctx.status);
  }, 1500);
}

async function show(invoke: Invoke, into: HTMLElement, status: HTMLElement) {
  const size = region(into);
  const panes = (await invoke("shown", size)) as Pane[];
  into.textContent = "";
  for (const pane of panes) drawPane(into, pane);

  const at = (await invoke("cursor", { height: size.height })) as [number, number] | null;
  const focused = panes.find((pane) => pane.focused);
  if (at && focused) {
    const box = into.children[panes.indexOf(focused)];
    if (box instanceof HTMLElement) {
      place(box, at[0], at[1]);
      // What the editor has selected — by Shift, a drag, a find.
      const chosen = (await invoke("selection", {})) as [number, number] | null;
      if (Array.isArray(chosen)) mark(box, chosen[0], chosen[1] - chosen[0], "selection");
      // Every place the name under the cursor is written, marked faintly.
      const named = (await invoke("occurrences", {})) as [number, number[]] | null;
      if (Array.isArray(named)) for (const start of named[1]) mark(box, start, named[0], "occurrence");
      // The bracket beside the cursor and its match, both marked.
      const pair = (await invoke("brackets", {})) as [number, number] | null;
      if (Array.isArray(pair)) for (const offset of pair) mark(box, offset, 1, "bracket");
    }
  }
  // Where the cursor is in a file, at the status line's right end.
  const position = document.getElementById("position");
  if (position) {
    const mode = (await invoke("mode", {})) as string | null;
    const where = at && focused && "file" in focused.buffer.kind
      ? `Ln ${(focused.top ?? 0) + at[0] + 1}, Col ${at[1] + 1}`
      : "";
    position.textContent = where;
    // The mode as a badge in its own colour, as the first TraceLean wore it.
    if (typeof mode === "string" && mode) {
      const badge = document.createElement("span");
      const name = mode.toLowerCase();
      badge.className = `mode-badge ${name === "normal" || name === "insert" ? name : "other"}`;
      badge.textContent = mode.toUpperCase();
      position.append(badge);
    }
    // How many findings the checker has, once the editor has counted; a
    // click lists them.
    const problems = (await invoke("problems", {})) as number | null;
    if (typeof problems === "number") {
      const count = document.createElement("button");
      count.className = `problems ${problems ? "some" : "none"}`;
      count.textContent = problems ? `⚠ ${problems}` : "✓";
      count.title = problems ? `${problems} finding(s) — click to list them` : "the checker finds nothing";
      count.addEventListener("click", (event) => {
        event.stopPropagation();
        void run("act_here", { action: "trace.findings" });
      });
      position.prepend(count);
    }
  }

  // The two bars above the panes. Buffers, so the same function draws them,
  // and their rows carry the actions a key dispatches.
  const [stations, strip] = (await invoke("bars", {})) as [Buffer, Buffer];
  const inBar = (station: boolean, action: string, at: number) =>
    void run("act_in_bar", { station, action, offset: at });
  drawBar(document.getElementById("stations"), stations, true, inBar);
  drawBar(document.getElementById("strip"), strip, false, inBar);

  // The menu the mode offers, beside the buffer rather than instead of it. It
  // is a buffer, so it is drawn by the same function — and its rows carry
  // actions, which is how a window offers what a terminal binds to a key. A row
  // is about whatever the cursor is on, so it is performed where the cursor is.
  //
  // With no menu open it is the which-key bar, there from the start and never
  // moving: what can be done where the cursor is, keys first, so the keys are
  // learnt by reading them. One height either way, so the panes above it keep
  // theirs.
  const bar = document.getElementById("menu");
  if (!bar) return;
  const offered = (await invoke("menu", {})) as Buffer | null;
  bar.hidden = false;
  bar.textContent = "";
  if (offered) {
    bar.className = "mode";
    draw(bar, offered, (action) => void run("act_here", { action }));
    return;
  }
  const here = (await invoke("offers_here", {})) as [string, number, Offer[]] | null;
  bar.className = "which-key";
  if (here) whichKey(bar, here[2], here[0], here[1]);
}

// Put a caret where the editor says the cursor is.
//
// Drawn rather than described: a `<span>` with nothing in it, positioned by the
// line and column the editor reports. It shows no text, so it cannot be text
// the buffer does not contain.
function place(into: HTMLElement, line: number, column: number) {
  const row = into.children[line];
  if (!(row instanceof HTMLElement)) return;
  const caret = document.createElement("span");
  caret.className = "caret";
  caret.style.left = `calc(${column}ch + var(--gutter, 0ch))`;
  caret.dataset.column = String(column);
  row.classList.add("active");
  row.appendChild(caret);
  // Past the pane's right edge, scroll across to it: the pane is drawn anew
  // on every change, so this is what keeps a long line's cursor in view.
  if (caret.offsetLeft + 16 > into.clientWidth) {
    into.scrollLeft = caret.offsetLeft - into.clientWidth / 2;
  }
}

// Paint with the theme: every colour in it becomes a CSS variable, named by its
// section and key — `--ui-accent`, `--roles-levelL1`, `--syntax-keyword` — and
// the stylesheet refers to nothing else. Changing a colour is editing JSON.
async function paint(invoke: Invoke) {
  const theme = (await invoke("theme", {})) as Record<string, unknown>;
  const root = document.documentElement.style;
  for (const [section, entries] of Object.entries(theme)) {
    if (typeof entries !== "object" || entries === null) continue;
    for (const [key, value] of Object.entries(entries as Record<string, unknown>)) {
      if (typeof value === "string") {
        root.setProperty(`--${section}-${key.replace(/\./g, "-")}`, value);
      }
    }
  }
  // The token rules are in the stylesheet, one per kind the core names: a
  // stylesheet written here at run time is refused by the window's content
  // policy, which left every file one colour in the window while a browser
  // without that policy showed them coloured.
}

export async function start() {
  const into = document.getElementById("buffer");
  const status = document.getElementById("status");
  if (!into || !status) return;

  const invoke = bridge();
  if (!invoke) {
    status.textContent =
      "No editor is attached. This page renders buffers; it does not produce them.";
    return;
  }
  ctx = { invoke, into, status };
  try {
    hover = ((await invoke("described", {})) as Record<string, string> | null) ?? {};
  } catch {
    hover = {};
  }
  await paint(invoke);
  await show(invoke, into, status);
  poll();
  status.textContent =
    "Click a file to open it · click into it to type · right-click anything for what you can do there";
  // A theme edited while the window was in the background shows on return.
  window.addEventListener("focus", () => {
    void paint(invoke);
  });

  // The keymap belongs to the editor, so a key is sent rather than interpreted.
  // The names are the ones the editor uses; everything else is left alone.
  //
  // These are `keymap::NAMED` written in this language, because a browser
  // spells its keys differently and something has to translate. What this may
  // not do is invent a name: a name the keymap does not use is a binding that
  // silently does nothing, which is what the space bar was here — the browser
  // calls it `" "`, the keymap calls it `Space`, and nothing said so, so the
  // leader menu could not be opened. Realises REQ-MYTH.actions_reachable;
  // written out rather than annotated, because TraceLean has no TypeScript
  // grammar to place a claim with (ADR-0008). What checks it is
  // `crates/core/tests/shipped_keymap.rs`, where the claim is annotated.
  const named = new Set([
    "Backspace",
    "Down",
    "Enter",
    "Escape",
    "Left",
    "Right",
    "Space",
    "Tab",
    "Up",
  ]);
  // The keys every editor has and a modal keymap does not spell. They are sent
  // by name to the editor, which performs the same action or cursor move a key
  // sequence would (`Editor::chord`); the page decides nothing about them.
  const chords: Record<string, string> = {
    "C-s": "C-s",
    "C-z": "C-z",
    "C-y": "C-y",
    "C-S-z": "C-S-z",
    "C-w": "C-w",
    F12: "F12",
    F1: "F1",
    Home: "Home",
    End: "End",
    PageUp: "PageUp",
    PageDown: "PageDown",
    Delete: "Delete",
    F3: "F3",
    "S-F3": "S-F3",
    "C-g": "C-g",
    "C-S-g": "C-S-g",
    "C-ArrowLeft": "C-Left",
    "C-ArrowRight": "C-Right",
    "C-Backspace": "C-Backspace",
    "C-Delete": "C-Delete",
    "C-/": "C-/",
    "A-ArrowUp": "A-Up",
    "A-ArrowDown": "A-Down",
    "A-S-ArrowDown": "A-S-Down",
    "A-S-ArrowUp": "A-S-Up",
    "A-ArrowLeft": "A-Left",
    "A-ArrowRight": "A-Right",
    "C-S-k": "C-S-k",
    "C-l": "C-l",
    "C-d": "C-d",
    "S-Tab": "S-Tab",
    "C-Tab": "C-Tab",
    "C-S-Tab": "C-S-Tab",
  };
  document.addEventListener("keydown", async (event) => {
    if (event.target instanceof HTMLInputElement) return;
    if (event.key === "Escape" && document.getElementById("context")) {
      event.preventDefault();
      closeContext();
      return;
    }
    const letter = event.key.length === 1 ? event.key.toLowerCase() : event.key;
    const held = `${event.ctrlKey || event.metaKey ? "C-" : ""}${event.altKey ? "A-" : ""}`;
    const chord = held
      ? `${held}${event.shiftKey ? "S-" : ""}${letter}`
      : event.shiftKey && (event.key === "F3" || event.key === "Tab") ? `S-${event.key}` : event.key;
    if (chord === "C-f") {
      event.preventDefault();
      openFind();
      return;
    }
    if (chord === "C-S-f") {
      event.preventDefault();
      openFind(true);
      return;
    }
    if (chord === "C-h") {
      event.preventDefault();
      openFind(false, true);
      return;
    }
    if (chord === "C-p") {
      event.preventDefault();
      openQuick();
      return;
    }
    if (chord === "C-S-p") {
      event.preventDefault();
      openPalette();
      return;
    }
    if (chord === "C-S-o") {
      event.preventDefault();
      openSymbols();
      return;
    }
    // A selection drawn with the pointer in a file becomes the editor's
    // first, so whatever the key does — type over it, delete it, indent it,
    // copy it — it does to that.
    await adoptSelection();
    // Shift with a movement selects, in the editor, which keeps the
    // selection while the window scrolls past it.
    const moving = ["ArrowLeft", "ArrowRight", "ArrowUp", "ArrowDown", "Home", "End", "PageUp", "PageDown"];
    if (event.shiftKey && !event.altKey && moving.includes(event.key)) {
      event.preventDefault();
      await run("chord", { chord: `${event.ctrlKey ? "C-" : ""}S-${event.key.replace("Arrow", "")}` });
      return;
    }
    // Select all, copy and cut are the editor's in a file; in a listing or a
    // record they stay the browser's.
    const inFile = document.querySelector(".pane.focused.kind-file") !== null;
    const browsing = !(window.getSelection()?.isCollapsed ?? true);
    if (inFile && !browsing && (chord === "C-a" || chord === "C-c" || chord === "C-x")) {
      event.preventDefault();
      await run("chord", { chord });
      return;
    }
    if (chords[chord]) {
      event.preventDefault();
      await run("chord", { chord: chords[chord] });
      return;
    }
    // Copy, cut, select-all and the rest stay the browser's.
    if (event.metaKey || event.ctrlKey || event.altKey) return;
    const pressed = event.key === " " ? "Space" : event.key;
    const key = pressed.startsWith("Arrow") ? pressed.slice(5) : pressed;
    if (key.length !== 1 && !named.has(key)) return;
    event.preventDefault();
    const said = (await invoke("press", { key })) as string;
    status.textContent = said;
    await show(invoke, into, status);
  });

  // Pasting types the clipboard's text in one change.
  document.addEventListener("paste", (event) => {
    if (event.target instanceof HTMLInputElement) return;
    const text = event.clipboardData?.getData("text/plain") ?? "";
    event.preventDefault();
    void (async () => {
      // Pasted over the selection, which the editor replaces.
      await adoptSelection();
      if (text) await run("paste", { text });
      else if (ctx) await show(ctx.invoke, ctx.into, ctx.status);
    })();
  });

  // A click anywhere outside the context menu closes it.
  document.addEventListener("click", () => closeContext());

  // A mouse's back and forward buttons go where Alt+Left and Alt+Right do.
  document.addEventListener("mouseup", (event) => {
    if (event.button !== 3 && event.button !== 4) return;
    event.preventDefault();
    void run("chord", { chord: event.button === 3 ? "A-Left" : "A-Right" });
  });

  // A window that changed size shows a different number of lines, so the
  // editor is asked again rather than the page guessing.
  window.addEventListener("resize", () => {
    void show(invoke, into, status);
  });
}

if (typeof document !== "undefined") {
  start();
}
