import { drawOverview, showing, type Design, type Node as DesignNode } from "./overview.ts";
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
        cardOn(button, piece.text.trim());
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
    cardOn(chip, requirement);
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

// Wheel ticks arrive faster than a draw. They are added up and sent as one scroll per
// frame, only the panes are redrawn while the wheel turns, and everything else (bars,
// what can be done here, the counts) is drawn once, when it stops.
//
// @implements REQ-LOOK.scroll_is_one_draw_per_frame
const wheel = { pane: "", lines: 0, busy: false, settle: 0 as ReturnType<typeof setTimeout> | 0 };

async function pumpWheel() {
  if (wheel.busy || !ctx) return;
  wheel.busy = true;
  try {
    while (wheel.lines !== 0) {
      const lines = wheel.lines;
      wheel.lines = 0;
      await ctx.invoke("scroll", { pane: wheel.pane, lines });
      await show(ctx.invoke, ctx.into, ctx.status, true);
      await new Promise((next) => requestAnimationFrame(next));
    }
  } finally {
    wheel.busy = false;
  }
  if (wheel.settle) clearTimeout(wheel.settle);
  wheel.settle = setTimeout(() => {
    wheel.settle = 0;
    if (ctx && !wheel.busy) void show(ctx.invoke, ctx.into, ctx.status);
  }, 140);
}

let treeMarks: Record<string, string> | null = null;

/** The meaning of each mark of the file tree, asked of the editor once. */
async function loadTreeMarks(invoke: Invoke) {
  try {
    const said = (await invoke("tree_marks", {})) as [string, string][] | null;
    treeMarks = Array.isArray(said) ? Object.fromEntries(said) : {};
  } catch {
    treeMarks = {};
  }
}

// @implements REQ-SHOW.tree_marks_explained
export function explainMarks(box: HTMLElement, marks: Record<string, string> | null = treeMarks) {
  if (!marks) return;
  for (const span of box.querySelectorAll<HTMLElement>(".line > span")) {
    const text = (span.textContent ?? "").trim();
    if (Array.from(text).length === 1 && marks[text]) {
      span.title = marks[text];
      span.classList.add("explained");
    }
  }
  for (const row of box.querySelectorAll<HTMLElement>(".line")) {
    const name = row.querySelector<HTMLElement>("button.actionable");
    const lead = row.firstChild;
    if (!name || !lead || lead === name || !(lead instanceof HTMLElement)) continue;
    const arrow = (lead.textContent ?? "").trim();
    if (marks[arrow] && !lead.title) lead.title = marks[arrow];
  }
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
  if (kind === "directory") explainMarks(box);

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
    event.stopPropagation();
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
      wheel.lines = wheel.pane === pane.pane ? wheel.lines + lines : lines;
      wheel.pane = pane.pane;
      void pumpWheel();
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

/** A drawn icon per station: the shapes inside a 24×24 box, stroked in the text colour. */
const STATION_ICONS: Record<string, string> = {
  project: '<path d="M3 7a2 2 0 0 1 2-2h4l2 2h8a2 2 0 0 1 2 2v8a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2z"/>',
  trace: '<path d="M10 14a4 4 0 0 0 5.7 0l3-3a4 4 0 0 0-5.7-5.7l-1 1"/><path d="M14 10a4 4 0 0 0-5.7 0l-3 3a4 4 0 0 0 5.7 5.7l1-1"/>',
  sandbox: '<rect x="3" y="4" width="18" height="16" rx="2"/><path d="M7 9l3 3-3 3M13 15h4"/>',
  design: '<rect x="9" y="3" width="6" height="5" rx="1"/><rect x="3" y="16" width="6" height="5" rx="1"/><rect x="15" y="16" width="6" height="5" rx="1"/><path d="M12 8v4M6 16v-4h12v4"/>',
  history: '<path d="M4 12a8 8 0 1 0 2.5-5.8L4 8.5"/><path d="M4 4v4.5h4.5M12 8v4.5l3 1.5"/>',
  findings: '<path d="M12 4l9 16H3z"/><path d="M12 10v4.5M12 17.2v.3"/>',
  definitions: '<path d="M4 7h9M17 7h3M4 17h3M11 17h9"/><circle cx="15" cy="7" r="2"/><circle cx="9" cy="17" r="2"/>',
  report: '<path d="M6 3h8l4 4v14H6z"/><path d="M14 3v4h4M9 12h6M9 16h6"/>',
  overview: '<path d="M4 20V4M4 20h16"/><path d="M8 16v-4M12 16V8M16 16v-6"/>',
  modules: '<rect x="4" y="4" width="7" height="7" rx="1"/><rect x="13" y="13" width="7" height="7" rx="1"/><path d="M11 7.5h5.5V13"/>',
};

/**
 * The icon of a station, or nothing when the action is not a station's.
 *
 * @implements REQ-LOOK.stations_have_icons
 */
export function stationIcon(action: string): SVGElement | null {
  const shapes = STATION_ICONS[action.replace("screen.station.", "")];
  if (!action.startsWith("screen.station.") || !shapes) return null;
  const icon = document.createElementNS("http://www.w3.org/2000/svg", "svg");
  icon.setAttribute("viewBox", "0 0 24 24");
  icon.setAttribute("class", "station-icon");
  icon.setAttribute("aria-hidden", "true");
  icon.innerHTML = shapes;
  return icon;
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
      const icon = station && piece.actions.length ? stationIcon(piece.actions[0]) : null;
      if (icon) {
        item.appendChild(icon);
        item.setAttribute("aria-label", presented.name.trim());
      } else if (presented.painted === presented.name) {
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
        if (!station && piece.actions[0].split(" ")[0] === "screen.show") {
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

// @implements REQ-LOOK.pointer_reaches_actions
async function openContext(pane: string, at: number, x: number, y: number) {
  closeContext();
  if (!ctx) return;
  const offered = (await ctx.invoke("offers", { pane, offset: at })) as Offer[];
  const menu = overlay(offered, pane, at);
  menu.className = "at-pointer";
  document.body.appendChild(menu);
  settle(menu, x, y);
}

function settle(menu: HTMLElement, x: number, y: number) {
  const box = menu.getBoundingClientRect();
  const left = Math.max(4, Math.min(x, window.innerWidth - box.width - 4));
  const top = y + box.height + 4 > window.innerHeight ? Math.max(4, y - box.height) : y;
  menu.style.left = `${left}px`;
  menu.style.top = `${top}px`;
  menu.querySelector<HTMLElement>(".context-entry")?.focus({ preventScroll: true });
}

function closeOverview() {
  hideCard();
  document.getElementById("overview")?.remove();
}

interface Card {
  id: string;
  title: string;
  refines: string[];
  clauses: { clause: string; text: string; marks: string }[];
  claims: [string, string, number][];
}

let carding = 0;

function hideCard() {
  carding += 1;
  document.getElementById("card")?.remove();
}

// @implements REQ-FIGURE.card_on_hover
async function showCard(anchor: Element, name: string) {
  if (!ctx) return;
  const mine = ++carding;
  await new Promise((wait) => setTimeout(wait, 220));
  if (mine !== carding) return;
  const card = (await ctx.invoke("card", { name })) as Card | null;
  if (mine !== carding) return;
  if (!card || typeof card !== "object" || !Array.isArray(card.clauses)) {
    if (anchor instanceof HTMLElement) void sayRequirement(anchor, name);
    return;
  }
  document.getElementById("card")?.remove();
  const box = document.createElement("aside");
  box.id = "card";
  box.setAttribute("role", "tooltip");
  const line = (className: string, text: string) => {
    const made = document.createElement("div");
    made.className = className;
    made.textContent = text;
    box.appendChild(made);
    return made;
  };
  line("card-id", card.id);
  line("card-title", card.title);
  if (card.refines.length) line("card-refines", `refines ${card.refines.join(", ")}`);
  for (const clause of card.clauses.slice(0, 6)) {
    const row = line("card-clause", "");
    const marks = document.createElement("span");
    marks.className = "card-marks";
    for (const [index, letter] of [...clause.marks].entries()) {
      const mark = document.createElement("span");
      mark.className = letter === "-" ? "mark-off" : `mark-on mark-${letter}`;
      mark.textContent = letter === "-" ? "AJTP"[index] : letter;
      marks.appendChild(mark);
    }
    const name = document.createElement("b");
    name.textContent = clause.clause;
    const said = document.createElement("span");
    said.textContent = clause.text.length > 190 ? `${clause.text.slice(0, 189)}…` : clause.text;
    row.append(marks, name, said);
  }
  if (card.clauses.length > 6) line("card-more", `… and ${card.clauses.length - 6} more clauses`);
  const roles = new Map<string, number>();
  for (const [role] of card.claims) roles.set(role, (roles.get(role) ?? 0) + 1);
  if (roles.size) line("card-claims", [...roles].map(([role, count]) => `${count} ${role}`).join(" · "));
  for (const [role, file, at] of card.claims.slice(0, 5)) line("card-file", `${role}  ${file}:${at}`);
  if (!card.claims.length) line("card-claims none", "nothing claims this yet");
  document.body.appendChild(box);
  const at = anchor.getBoundingClientRect();
  const size = box.getBoundingClientRect();
  const left = Math.max(6, Math.min(at.left, window.innerWidth - size.width - 6));
  const below = at.bottom + 8 + size.height < window.innerHeight;
  box.style.left = `${left}px`;
  box.style.top = `${below ? at.bottom + 8 : Math.max(6, at.top - size.height - 8)}px`;
}

function cardOn(anchor: Element, name: string) {
  anchor.addEventListener("mouseenter", () => void showCard(anchor, name));
  anchor.addEventListener("mouseleave", hideCard);
  anchor.addEventListener("mousedown", hideCard);
}

async function openOverview(force = false) {
  if (!ctx) return;
  if (document.getElementById("overview") && !force) return closeOverview();
  const invoke = ctx.invoke;
  const list = async <T>(command: string): Promise<T[]> => {
    try {
      const got = await invoke(command, {});
      return Array.isArray(got) ? (got as T[]) : [];
    } catch {
      return [];
    }
  };
  const [nodes] = ((await invoke("overview", {})) as [DesignNode[], number | null] | null) ?? [[], null];
  const counted = await invoke("problems", {});
  const design: Design = {
    nodes: Array.isArray(nodes) ? nodes : [],
    problems: typeof counted === "number" ? counted : null,
    runs: await list("runs"),
    impact: await list("impact"),
    modules: await list("modules"),
    steps: await list("agent_steps"),
  };
  closeOverview();
  const panel = document.createElement("section");
  panel.id = "overview";
  panel.setAttribute("aria-label", "overview");
  panel.addEventListener("click", (event) => event.stopPropagation());
  panel.addEventListener("contextmenu", (event) => event.stopPropagation());
  drawOverview(panel, design, {
    open: (id) => {
      hideCard();
      closeOverview();
      void run("choose", { pane: "explorer", offset: 0, action: "trace.requirement", target: id, answer: null });
    },
    close: closeOverview,
    copy: async () => {
      try {
        const text = await invoke("overview_markdown", {});
        if (typeof text !== "string" || !text) return false;
        await navigator.clipboard.writeText(text);
        return true;
      } catch {
        return false;
      }
    },
    act: (action) => {
      hideCard();
      void run("act_here", { action }).then(() => closeOverview());
    },
    report: async () => {
      try {
        await run("act_here", { action: "trace.report" });
        return true;
      } catch {
        return false;
      }
    },
    card: (anchor, name) => void showCard(anchor, name),
    uncard: hideCard,
  });
  document.body.appendChild(panel);
}

type Group = [string, [string, string, string][]];
let leader: Group[] = [];

function menuAt(): HTMLElement {
  closeContext();
  const menu = document.createElement("div");
  menu.id = "context";
  menu.className = "at-pointer";
  menu.setAttribute("role", "menu");
  return menu;
}

function entry(label: string, keys: string, pick: (event: MouseEvent) => void): HTMLElement {
  const item = document.createElement("button");
  item.className = "context-entry";
  item.setAttribute("role", "menuitem");
  const hint = document.createElement("span");
  hint.className = "context-keys";
  hint.textContent = keys;
  const said = document.createElement("span");
  said.textContent = label;
  item.append(hint, said);
  item.addEventListener("click", (event) => {
    event.stopPropagation();
    pick(event);
  });
  return item;
}

// @implements REQ-LOOK.menu_bar_lists_leader
function openGroup(group: Group, x: number, y: number, from?: HTMLElement) {
  const menu = menuAt();
  if (from) menu.dataset.from = group[0];
  const heading = document.createElement("div");
  heading.className = "context-group";
  heading.textContent = group[0];
  menu.appendChild(heading);
  for (const [keys, label, action] of group[1]) {
    menu.appendChild(entry(label, keys, () => {
      closeContext();
      void run("act_here", { action });
    }));
  }
  document.body.appendChild(menu);
  settle(menu, x, y);
}

function openLeader(x: number, y: number) {
  if (!leader.length) return;
  const menu = menuAt();
  for (const group of leader) {
    menu.appendChild(entry(`${group[0]} ›`, group[1][0]?.[0].split(" ").slice(0, 2).join(" ") ?? "", () => openGroup(group, x, y)));
  }
  document.body.appendChild(menu);
  settle(menu, x, y);
}

async function drawMenubar(invoke: Invoke) {
  const bar = document.getElementById("menubar");
  if (!bar) return;
  try {
    leader = ((await invoke("leader", {})) as Group[] | null) ?? [];
  } catch {
    leader = [];
  }
  if (!Array.isArray(leader)) leader = [];
  bar.textContent = "";
  for (const group of leader) {
    const item = document.createElement("button");
    item.className = "menubar-item";
    item.textContent = group[0];
    const open = () => {
      const at = item.getBoundingClientRect();
      openGroup(group, at.left, at.bottom, item);
    };
    item.addEventListener("click", (event) => {
      event.stopPropagation();
      if (document.getElementById("context")?.dataset.from === group[0]) closeContext();
      else open();
    });
    item.addEventListener("mouseenter", () => {
      const shown = document.getElementById("context")?.dataset.from;
      if (shown && shown !== group[0]) open();
    });
    bar.appendChild(item);
  }
  const tool = (text: string, title: string, name: string, pick: () => void) => {
    const button = document.createElement("button");
    button.className = `menubar-tool ${name}`;
    button.textContent = text;
    button.title = title;
    button.setAttribute("aria-label", title);
    button.addEventListener("click", (event) => {
      event.stopPropagation();
      closeContext();
      pick();
    });
    return button;
  };
  const spacer = document.createElement("span");
  spacer.className = "menubar-spacer";
  bar.append(
    tool("Overview", "The design as figures: marks, the requirement graph, where work is left", "overview", () => void openOverview()),
    spacer,
    tool("A−", "Smaller text (Ctrl -)", "zoom", () => void run("zoom", { steps: -1 })),
    tool("A+", "Bigger text (Ctrl +)", "zoom", () => void run("zoom", { steps: 1 })),
    tool("Search commands", "Search every command by a few words or letters (Ctrl+P)", "search", () => openPalette()),
    tool("Open file", "Open a file by name (Ctrl+O or Ctrl+Shift+P)", "open", () => openQuick()),
    tool("↻ Files", "Refresh: reload the files from the disk and the sandbox (F5, Space f R)", "refresh", () => void run("act_here", { action: "file.refresh" })),
    tool("Theme", "Switch theme: dark or light (Space w c)", "theme", () => void run("act_here", { action: "look.theme" })),
    tool("?", "Every key (F1)", "help", () => void run("chord", { chord: "F1" })),
  );
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
  openChooser("Do something — type a few words or letters of it", async (query) => {
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

async function show(invoke: Invoke, into: HTMLElement, status: HTMLElement, panesOnly = false) {
  const size = region(into);
  const panes = (await invoke("shown", size)) as Pane[];
  syncPanes(into, panes);

  const at = (await invoke("cursor", { height: size.height })) as [number, number] | null;
  const focused = panes.find((pane) => pane.focused);
  if (at && focused) {
    const box = into.children[panes.indexOf(focused)];
    if (box instanceof HTMLElement) {
      place(box, at[0], at[1]);
      const [chosen, named, pair] = (await Promise.all([
        invoke("selection", {}),
        invoke("occurrences", {}),
        invoke("brackets", {}),
      ])) as [[number, number] | null, [number, number[]] | null, [number, number] | null];
      if (Array.isArray(chosen)) mark(box, chosen[0], chosen[1] - chosen[0], "selection");
      if (Array.isArray(named)) for (const start of named[1]) mark(box, start, named[0], "occurrence");
      if (Array.isArray(pair)) for (const offset of pair) mark(box, offset, 1, "bracket");
    }
  }
  if (panesOnly) return;
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
    const touched = (await invoke("impact", {})) as unknown;
    if (Array.isArray(touched) && touched.length) {
      const badge = document.createElement("button");
      badge.className = "problems impact";
      badge.textContent = `Δ ${touched.length}`;
      badge.title = `your unsaved edits touch ${touched.length} clause(s) — click to see which, before saving`;
      badge.addEventListener("click", (event) => {
        event.stopPropagation();
        showing.filter = "risk";
        void openOverview(true);
      });
      position.prepend(badge);
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
  const layout = (theme.layout ?? {}) as Record<string, unknown>;
  document.body.classList.toggle("menubar-hidden", layout.menuBar === "hidden");
  document.body.classList.toggle("stations-top", layout.stations === "top");
  document.body.classList.toggle("stations-hidden", layout.stations === "hidden");
  root.colorScheme = theme.base === "light" ? "light" : "dark";
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
  await loadTreeMarks(invoke);
  await drawMenubar(invoke);
  await show(invoke, into, status);
  poll();
  status.textContent =
    "Space: every command · . or right-click: what works here · Escape: back · F1: every key";
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
    if (event.key === "Escape" && document.getElementById("overview") && !document.getElementById("context")) {
      event.preventDefault();
      closeOverview();
      return;
    }
    if ((event.ctrlKey || event.metaKey) && ["=", "+", "-", "_", "0"].includes(event.key)) {
      event.preventDefault();
      await run("zoom", { steps: event.key === "0" ? 0 : event.key === "-" || event.key === "_" ? -1 : 1 });
      return;
    }
    if (event.key === "Escape" && document.getElementById("context")) {
      event.preventDefault();
      closeContext();
      return;
    }
    const pointed = document.querySelector<HTMLElement>("#context.at-pointer");
    if (pointed && ["ArrowDown", "ArrowUp", "Enter"].includes(event.key)) {
      event.preventDefault();
      const entries = [...pointed.querySelectorAll<HTMLElement>(".context-entry")];
      const at = entries.indexOf(document.activeElement as HTMLElement);
      if (event.key === "Enter") entries[at]?.click();
      else entries[(at + (event.key === "ArrowDown" ? 1 : entries.length - 1)) % entries.length]?.focus();
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
      openPalette();
      return;
    }
    if (chord === "C-S-p" || chord === "C-o") {
      event.preventDefault();
      openQuick();
      return;
    }
    if (chord === "F5") {
      event.preventDefault();
      await run("act_here", { action: "file.refresh" });
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
  // A press anywhere outside an open menu closes it, as Escape does. Capturing, so that a
  // pane or a panel that keeps its clicks to itself cannot leave the menu standing.
  // @implements REQ-LOOK.press_outside_closes_menu
  document.addEventListener(
    "mousedown",
    (event) => {
      const menu = document.getElementById("context");
      const on = event.target instanceof Element ? event.target : null;
      if (menu && !(on && (menu.contains(on) || on.closest(".menubar-item")))) closeContext();
    },
    true,
  );

  // @implements REQ-LOOK.pointer_reaches_actions
  document.addEventListener("contextmenu", (event) => {
    if (event.target instanceof HTMLInputElement) return;
    event.preventDefault();
    const [x, y] = [event.clientX, event.clientY];
    const below = event.target instanceof Element && event.target.closest("#menu, #status, #position");
    if (!below) return openLeader(x, y);
    void (async () => {
      const here = (await invoke("offers_here", {})) as [string, number, Offer[]] | null;
      if (!here) return openLeader(x, y);
      closeContext();
      const menu = overlay(here[2], here[0], here[1]);
      menu.className = "at-pointer";
      document.body.appendChild(menu);
      settle(menu, x, y);
    })();
  });

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
