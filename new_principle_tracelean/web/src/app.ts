import { classOf, pieces, shown } from "./render.ts";
import type { Buffer, Pane } from "./view.ts";

const LINE = 21;

type Invoke = (command: string, args: Record<string, unknown>) => Promise<unknown>;

function bridge(): Invoke | null {
  const tauri = (globalThis as Record<string, any>).__TAURI__;
  return tauri?.core?.invoke ?? null;
}

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

// @implements REQ-VIEW.one_representation
// @implements REQ-SHOW.core_produces
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
      row.appendChild(document.createTextNode(""));
    }
    for (const piece of line) {
      if (piece.actions.length === 0) {
        const span = document.createElement("span");
        span.className = classOf(piece.role);
        span.dataset.at = String(piece.at);
        span.textContent = piece.text;
        const role = piece.role;
        if (typeof role === "object" && "claim" in role && piece.text.trim().length === 1) {
          span.title = `something here ${role.claim.role} a requirement — 🔗 shows which`;
        }
        row.appendChild(span);
        continue;
      }
      const button = document.createElement("button");
      button.className = `${classOf(piece.role)} actionable`;
      button.dataset.at = String(piece.at);
      button.dataset.action = piece.actions[0];
      button.textContent = piece.text;
      button.title = `${piece.actions.map((a) => hover[a] ?? a).join(" · ")}  (right-click for more)`;
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

function positionOf(node: Node, index: number): number | null {
  const element = node.nodeType === Node.TEXT_NODE ? node.parentElement : (node as Element);
  const host = element?.closest("[data-at]");
  if (host instanceof HTMLElement && node.nodeType === Node.TEXT_NODE && host.contains(node)) {
    return Number(host.dataset.at) + Array.from((node.textContent ?? "").slice(0, index)).length;
  }
  const row = element?.closest(".line") as HTMLElement | null;
  if (!row) return null;
  return index === 0 ? Number(row.dataset.start) : Number(row.dataset.start) + Number(row.dataset.length);
}

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

async function adoptSelection(): Promise<boolean> {
  const chosen = selectedInFile();
  if (!chosen || !ctx) return false;
  window.getSelection()?.removeAllRanges();
  ctx.status.textContent = (await ctx.invoke("select", { pane: chosen.pane, start: chosen.start, end: chosen.end })) as string;
  return true;
}

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
    chip.addEventListener("mouseenter", () => void sayRequirement(chip, requirement), { once: true });
    chip.addEventListener("mousedown", (event) => event.stopPropagation());
    chip.addEventListener("click", (event) => {
      event.stopPropagation();
      void run("choose", { pane: pane.pane, offset: 0, action: "trace.requirement", target: requirement, answer: null });
    });
    holder.appendChild(chip);
  }
}

function drawCoverage(box: HTMLElement, pane: Pane) {
  for (const [line, hits, said] of pane.coverage ?? []) {
    const row = box.children[line - (pane.top ?? 0)];
    if (!(row instanceof HTMLElement)) continue;
    row.classList.add(hits > 0 ? "covered" : "uncovered");
    row.title = said;
  }
}

async function showChange(pane: string, at: number, x: number, y: number) {
  if (!ctx) return;
  const change = (await ctx.invoke("history_preview", { pane, offset: at })) as Buffer | null;
  hideChange();
  if (!change) return;
  const box = document.createElement("div");
  box.id = "change";
  box.className = "pane kind-record";
  draw(box, change, () => {});
  box.style.left = `${Math.max(8, x - 520)}px`;
  box.style.top = `${Math.min(y, window.innerHeight - 260)}px`;
  document.body.appendChild(box);
}

function hideChange() {
  document.getElementById("change")?.remove();
}

let dragged = false;

function drawScrollbar(box: HTMLElement, pane: Pane) {
  const total = pane.lines ?? 0;
  const shown = pane.at.height;
  if (total <= shown || shown <= 0) return;
  const top = pane.top ?? 0;
  const track = document.createElement("div");
  track.className = "scrollbar";
  const thumb = document.createElement("div");
  thumb.className = "scrollthumb";
  const room = () => Math.max(1, track.clientHeight || box.clientHeight);
  const fraction = Math.max(0.05, shown / total);
  thumb.style.height = `${fraction * 100}%`;
  thumb.style.top = `${(Math.min(top, total - shown) / total) * 100}%`;
  track.appendChild(thumb);
  box.appendChild(track);
  const keepAtEdge = () => (track.style.transform = `translateX(${box.scrollLeft}px)`);
  box.addEventListener("scroll", keepAtEdge);
  keepAtEdge();

  let at = top;
  const scrollTo = (wanted: number) => {
    const target = Math.round(Math.max(0, Math.min(total - 1, wanted)));
    const lines = target - at;
    if (lines === 0) return;
    at = target;
    void run("scroll", { pane: pane.pane, lines });
  };
  thumb.addEventListener("mousedown", (event) => {
    event.preventDefault();
    event.stopPropagation();
    const [from, startedAt, height] = [event.clientY, at, room()];
    const move = (moved: MouseEvent) => scrollTo(startedAt + ((moved.clientY - from) / height) * total);
    const release = () => {
      document.removeEventListener("mousemove", move);
      document.removeEventListener("mouseup", release);
      dragged = true;
    };
    document.addEventListener("mousemove", move);
    document.addEventListener("mouseup", release);
  });
  track.addEventListener("mousedown", (event) => {
    if (event.target !== track) return;
    event.preventDefault();
    event.stopPropagation();
    const place = (event.clientY - track.getBoundingClientRect().top) / room();
    scrollTo(place * total - shown / 2);
  });
  track.addEventListener("click", (event) => event.stopPropagation());
}

const drawnFrom = new WeakMap<Element, string>();
let barsDrawn = "";
let barDrawn = "";
let barCursor = 0;

function syncPanes(into: HTMLElement, panes: Pane[]) {
  panes.forEach((pane, index) => {
    const value = JSON.stringify(pane);
    const held = into.children[index];
    if (held instanceof HTMLElement && drawnFrom.get(held) === value) {
      for (const over of Array.from(held.querySelectorAll(".caret, .selection, .occurrence, .bracket"))) over.remove();
      for (const row of Array.from(held.querySelectorAll(".line.active"))) row.classList.remove("active");
      return;
    }
    drawPane(into, pane);
    const fresh = into.lastElementChild;
    if (!fresh) return;
    drawnFrom.set(fresh, value);
    if (held && held !== fresh) into.replaceChild(fresh, held);
  });
  while (into.children.length > panes.length) into.lastElementChild?.remove();
}

function drawPane(into: HTMLElement, pane: Pane) {
  const box = document.createElement("div");
  const kind = Object.keys(pane.buffer.kind)[0] ?? "file";
  box.className = `pane kind-${kind}${pane.focused ? " focused" : ""}`;
  box.dataset.pane = pane.pane;
  box.dataset.buffer = pane.buffer.id;
  box.style.left = `${pane.at.left}ch`;
  box.style.top = `${pane.at.top * LINE}px`;
  box.style.width = `${pane.at.width}ch`;
  box.style.height = `${pane.at.height * LINE}px`;
  draw(box, pane.buffer, (action, at) => run("act", { pane: pane.pane, action, offset: at }));
  if (kind === "file") {
    Array.from(box.children).forEach((row, index) => {
      if (row instanceof HTMLElement) row.dataset.number = String((pane.top ?? 0) + index + 1);
    });
    drawChips(box, pane);
    drawCoverage(box, pane);
  }
  drawScrollbar(box, pane);

  for (const node of box.querySelectorAll<HTMLElement>('button[data-action="history.jump"]')) {
    node.addEventListener("mouseenter", (event) => void showChange(pane.pane, Number(node.dataset.at), event.clientX, event.clientY));
    node.addEventListener("mouseleave", hideChange);
  }

  box.addEventListener("mouseup", () => {
    if (kind !== "file" || !selectedInFile()) return;
    dragged = true;
    void adoptSelection().then(() => ctx && show(ctx.invoke, ctx.into, ctx.status));
  });
  box.addEventListener("click", (event) => {
    const selection = window.getSelection();
    if (dragged || (selection && !selection.isCollapsed)) {
      dragged = false;
      return;
    }
    const line = event.target instanceof Element ? (event.target.closest(".line") as HTMLElement | null) : null;
    if (kind === "file" && line && event.clientX < line.getBoundingClientRect().left + parseFloat(getComputedStyle(line).paddingLeft)) {
      const start = Number(line.dataset.start);
      void run("select", { pane: pane.pane, start, end: start + Number(line.dataset.length) + 1 });
      return;
    }
    const at = offsetAt(event);
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

  const handle = document.createElement("div");
  handle.className = "divider";
  handle.addEventListener("click", (event) => event.stopPropagation());
  handle.addEventListener("mousedown", (event) => {
    event.preventDefault();
    event.stopPropagation();
    let from = event.clientX;
    const character = box.getBoundingClientRect().width / Math.max(1, pane.at.width);
    let sending = false;
    const follow = (move: MouseEvent) => {
      const moved = Math.trunc((move.clientX - from) / character);
      if (moved === 0 || sending) return;
      from += moved * character;
      sending = true;
      void run("grab", { pane: pane.pane, amount: moved }).finally(() => (sending = false));
    };
    const release = () => {
      document.removeEventListener("mousemove", follow);
      document.removeEventListener("mouseup", release);
      document.body.classList.remove("dragging");
    };
    document.body.classList.add("dragging");
    document.addEventListener("mousemove", follow);
    document.addEventListener("mouseup", release);
  });
  box.appendChild(handle);
  into.appendChild(box);
}

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
        if (!station && piece.actions[0] === "screen.show") {
          const close = () =>
            void (ctx?.invoke("act_in_bar", { station, action: "screen.show", offset: piece.at }) ?? Promise.resolve())
              .then(() => run("chord", { chord: "C-w" }));
          item.addEventListener("auxclick", (event) => {
            if (event.button !== 1) return;
            event.preventDefault();
            close();
          });
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

interface Offer {
  group: string;
  label: string;
  action: string;
  target: string | null;
  asks: string | null;
  keys: string | null;
}

const GROUPS: Record<string, string> = {
  here: "Here",
  file: "This buffer",
  panes: "Panes",
  go: "Go to",
};

function closeContext() {
  document.getElementById("context")?.remove();
}

async function openContext(pane: string, at: number, _x: number, _y: number) {
  closeContext();
  if (!ctx) return;
  const offered = (await ctx.invoke("offers", { pane, offset: at })) as Offer[];
  document.body.appendChild(overlay(offered, pane, at));
}

type Where = number | (() => number);
const whereIs = (at: Where) => (typeof at === "function" ? at() : at);

function overlay(offered: Offer[], pane: string, at: Where): HTMLElement {
  const menu = document.createElement("div");
  menu.id = "context";
  menu.className = "which-key";
  menu.setAttribute("role", "menu");
  whichKey(menu, offered, pane, at);
  return menu;
}

function whichKey(into: HTMLElement, offered: Offer[], pane: string, at: Where) {
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
      void run("choose", { pane, offset: whereIs(at), action: offer.action, target: offer.target, answer: null });
    });
    column.appendChild(entry);
  }
}

function ask(menu: HTMLElement, offer: Offer, pane: string, at: Where) {
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
      void run("choose", { pane, offset: whereIs(at), action: offer.action, target: offer.target, answer });
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

function mark(box: HTMLElement, start: number, length: number, kind: string) {
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
  void ctx?.invoke("selected", {}).then((text) => {
    if (typeof text !== "string" || !text || text.includes("\n") || input.value) return;
    input.value = text;
    input.select();
  });
}

function openQuick() {
  openChooser("Open a file by name — name:42 or :42 for a line", async (query) => {
    const found = ((await ctx?.invoke("quick", { query })) as string[] | null) ?? [];
    return found.map((path) => ({
      label: path,
      pick: () => run("choose", { pane: "explorer", offset: 0, action: "file.open", target: path, answer: null }),
    }));
  });
}

function openPalette() {
  openChooser("Do something — type a few words of it", async (query) => {
    const found = ((await ctx?.invoke("palette", { query })) as [string, string][] | null) ?? [];
    return found.map(([action, said]) => ({ label: said, pick: () => run("act_here", { action }) }));
  });
}

function openSymbols() {
  openChooser("Go to a name this file declares", async (query) => {
    const found = ((await ctx?.invoke("symbols", { query })) as [string, string][] | null) ?? [];
    return found.map(([target, label]) => ({
      label,
      pick: () => run("choose", { pane: "explorer", offset: 0, action: "file.open", target, answer: null }),
    }));
  });
}

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

let hover: Record<string, string> = {};

const requirementTexts = new Map<string, string | null>();

async function sayRequirement(button: HTMLElement, name: string) {
  if (!ctx) return;
  if (!requirementTexts.has(name)) {
    requirementTexts.set(name, (await ctx.invoke("requirement_text", { name })) as string | null);
  }
  const said = requirementTexts.get(name);
  if (said) button.title = `${said}\n\n${button.title}`;
}

let ctx: { invoke: Invoke; into: HTMLElement; status: HTMLElement } | null = null;

async function run(command: string, args: Record<string, unknown>) {
  if (!ctx) return;
  const said = (await ctx.invoke(command, args)) as string;
  ctx.status.textContent = said;
  if (said.startsWith("setting saved")) await paint(ctx.invoke);
  await copyIfAsked(ctx.invoke);
  await show(ctx.invoke, ctx.into, ctx.status);
}

async function copyIfAsked(invoke: Invoke) {
  const text = (await invoke("take_clipboard", {})) as string | null;
  if (!text) return;
  try {
    await navigator.clipboard.writeText(text);
    return;
  } catch {
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
  syncPanes(into, panes);

  const at = (await invoke("cursor", { height: size.height })) as [number, number] | null;
  const focused = panes.find((pane) => pane.focused);
  if (at && focused) {
    const box = into.children[panes.indexOf(focused)];
    if (box instanceof HTMLElement) {
      place(box, at[0], at[1]);
      const chosen = (await invoke("selection", {})) as [number, number] | null;
      if (Array.isArray(chosen)) mark(box, chosen[0], chosen[1] - chosen[0], "selection");
      const named = (await invoke("occurrences", {})) as [number, number[]] | null;
      if (Array.isArray(named)) for (const start of named[1]) mark(box, start, named[0], "occurrence");
      const pair = (await invoke("brackets", {})) as [number, number] | null;
      if (Array.isArray(pair)) for (const offset of pair) mark(box, offset, 1, "bracket");
    }
  }
  const position = document.getElementById("position");
  if (position) {
    const mode = (await invoke("mode", {})) as string | null;
    const where = at && focused && "file" in focused.buffer.kind
      ? `Ln ${(focused.top ?? 0) + at[0] + 1}, Col ${at[1] + 1}`
      : "";
    position.textContent = where;
    if (typeof mode === "string" && mode) {
      const badge = document.createElement("span");
      const name = mode.toLowerCase();
      badge.className = `mode-badge ${name === "normal" || name === "insert" ? name : "other"}`;
      badge.textContent = mode.toUpperCase();
      position.append(badge);
    }
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

  const [stations, strip] = (await invoke("bars", {})) as [Buffer, Buffer];
  const inBar = (station: boolean, action: string, at: number) =>
    void run("act_in_bar", { station, action, offset: at });
  const barsValue = JSON.stringify([stations, strip]);
  if (barsValue !== barsDrawn) {
    barsDrawn = barsValue;
    drawBar(document.getElementById("stations"), stations, true, inBar);
    drawBar(document.getElementById("strip"), strip, false, inBar);
  }

  const bar = document.getElementById("menu");
  if (!bar) return;
  const offered = (await invoke("menu", {})) as Buffer | null;
  const here = offered ? null : ((await invoke("offers_here", {})) as [string, number, Offer[]] | null);
  barCursor = here ? here[1] : 0;
  const barValue = JSON.stringify([offered, here && [here[0], here[2]]]);
  if (barValue === barDrawn && !bar.hidden) return;
  barDrawn = barValue;
  bar.hidden = false;
  bar.textContent = "";
  if (offered) {
    bar.className = "mode";
    draw(bar, offered, (action) => void run("act_here", { action }));
    return;
  }
  bar.className = "which-key";
  if (here) whichKey(bar, here[2], here[0], () => barCursor);
}

function place(into: HTMLElement, line: number, column: number) {
  const row = into.children[line];
  if (!(row instanceof HTMLElement)) return;
  const caret = document.createElement("span");
  caret.className = "caret";
  caret.style.left = `calc(${column}ch + var(--gutter, 0ch))`;
  caret.dataset.column = String(column);
  row.classList.add("active");
  row.appendChild(caret);
  if (caret.offsetLeft + 16 > into.clientWidth) {
    into.scrollLeft = caret.offsetLeft - into.clientWidth / 2;
  }
}

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
  window.addEventListener("focus", () => {
    void paint(invoke);
  });

  // @implements REQ-MYTH.actions_reachable
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
    await adoptSelection();
    const moving = ["ArrowLeft", "ArrowRight", "ArrowUp", "ArrowDown", "Home", "End", "PageUp", "PageDown"];
    if (event.shiftKey && !event.altKey && moving.includes(event.key)) {
      event.preventDefault();
      await run("chord", { chord: `${event.ctrlKey ? "C-" : ""}S-${event.key.replace("Arrow", "")}` });
      return;
    }
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
    if (event.metaKey || event.ctrlKey || event.altKey) return;
    const pressed = event.key === " " ? "Space" : event.key;
    const key = pressed.startsWith("Arrow") ? pressed.slice(5) : pressed;
    if (key.length !== 1 && !named.has(key)) return;
    event.preventDefault();
    const said = (await invoke("press", { key })) as string;
    status.textContent = said;
    if (said.startsWith("setting saved")) await paint(invoke);
    await show(invoke, into, status);
  });

  document.addEventListener("paste", (event) => {
    if (event.target instanceof HTMLInputElement) return;
    const text = event.clipboardData?.getData("text/plain") ?? "";
    event.preventDefault();
    void (async () => {
      await adoptSelection();
      if (text) await run("paste", { text });
      else if (ctx) await show(ctx.invoke, ctx.into, ctx.status);
    })();
  });

  document.addEventListener("click", () => closeContext());

  document.addEventListener("mouseup", (event) => {
    if (event.button !== 3 && event.button !== 4) return;
    event.preventDefault();
    void run("chord", { chord: event.button === 3 ? "A-Left" : "A-Right" });
  });

  window.addEventListener("resize", () => {
    void show(invoke, into, status);
  });
}

if (typeof document !== "undefined") {
  start();
}
