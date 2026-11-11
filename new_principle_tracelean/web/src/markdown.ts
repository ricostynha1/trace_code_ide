/**
 * Markdown drawn as formatted text. Built with DOM nodes and textContent only, so
 * nothing in the text can become markup. Presentation only: the buffer's text stays
 * the content (REQ-VIEW.text_is_the_content); this is shown beside it, never instead
 * of what is edited.
 *
 * @implements REQ-LOOK.markdown_drawn
 */

function el(tag: string, className?: string, text?: string): HTMLElement {
  const node = document.createElement(tag);
  if (className) node.className = className;
  if (text !== undefined) node.textContent = text;
  return node;
}

const INLINE = /(`[^`\n]+`)|(\*\*[^*\n]+\*\*|__[^_\n]+__)|(\*[^*\s][^*\n]*\*|_[^_\s][^_\n]*_)|(~~[^~\n]+~~)|(\[[^\]\n]+\]\([^)\s]+\))/;

export function inline(into: HTMLElement, text: string) {
  let rest = text;
  while (rest.length > 0) {
    const found = INLINE.exec(rest);
    if (!found) {
      into.appendChild(document.createTextNode(rest));
      return;
    }
    if (found.index > 0) into.appendChild(document.createTextNode(rest.slice(0, found.index)));
    const token = found[0];
    if (found[1]) into.appendChild(el("code", "md-code", token.slice(1, -1)));
    else if (found[2]) {
      const strong = el("strong");
      inline(strong, token.slice(2, -2));
      into.appendChild(strong);
    } else if (found[3]) {
      const emphasis = el("em");
      inline(emphasis, token.slice(1, -1));
      into.appendChild(emphasis);
    } else if (found[4]) {
      const struck = el("del");
      inline(struck, token.slice(2, -2));
      into.appendChild(struck);
    } else {
      const split = token.indexOf("](");
      const link = el("span", "md-link");
      inline(link, token.slice(1, split));
      link.title = token.slice(split + 2, -1);
      into.appendChild(link);
    }
    rest = rest.slice(found.index + token.length);
  }
}

const BULLET = /^(\s*)([-*+]|\d+[.)])\s+(.*)$/;
const RULE = /^\s*([-*_])(\s*\1){2,}\s*$/;

function cells(row: string): string[] {
  return row.trim().replace(/^\|/, "").replace(/\|$/, "").split("|").map((cell) => cell.trim());
}

/** Remember which line of the text a drawn block came from, so spans over the text can find it. */
function from(node: HTMLElement, line: number): HTMLElement {
  node.dataset.line = String(line);
  return node;
}

function isTableRule(row: string | undefined): boolean {
  return row !== undefined && /^\s*\|?\s*:?-{2,}:?\s*(\|\s*:?-{2,}:?\s*)*\|?\s*$/.test(row) && row.includes("-");
}

export function markdown(text: string, offset = 0): HTMLElement {
  const root = el("div", "md");
  const lines = text.replace(/\r\n?/g, "\n").split("\n");
  let at = 0;
  while (at < lines.length) {
    const line = lines[at];
    const first = at;
    if (line.trim() === "") {
      at += 1;
      continue;
    }
    const fence = /^(\s*)(```+|~~~+)\s*(\S*)/.exec(line);
    if (fence) {
      const body: string[] = [];
      at += 1;
      while (at < lines.length && !lines[at].trim().startsWith(fence[2])) {
        body.push(lines[at].startsWith(fence[1]) ? lines[at].slice(fence[1].length) : lines[at]);
        at += 1;
      }
      at += 1;
      const pre = from(el("pre", "md-pre"), offset + first);
      if (fence[3]) pre.dataset.lang = fence[3];
      pre.appendChild(el("code", undefined, body.join("\n")));
      root.appendChild(pre);
      continue;
    }
    const heading = /^(#{1,6})\s+(.*?)\s*#*\s*$/.exec(line);
    if (heading) {
      const node = from(el(`h${heading[1].length}`, "md-h"), offset + first);
      inline(node, heading[2]);
      root.appendChild(node);
      at += 1;
      continue;
    }
    if (RULE.test(line)) {
      root.appendChild(from(el("hr", "md-hr"), offset + first));
      at += 1;
      continue;
    }
    if (line.trimStart().startsWith(">")) {
      const body: string[] = [];
      while (at < lines.length && lines[at].trimStart().startsWith(">")) {
        body.push(lines[at].trimStart().replace(/^>\s?/, ""));
        at += 1;
      }
      const quote = from(el("blockquote", "md-quote"), offset + first);
      quote.appendChild(markdown(body.join("\n"), offset + first));
      root.appendChild(quote);
      continue;
    }
    if (line.includes("|") && isTableRule(lines[at + 1])) {
      const table = from(el("table", "md-table"), offset + first);
      const head = from(el("tr"), offset + first);
      for (const cell of cells(line)) {
        const th = el("th");
        inline(th, cell);
        head.appendChild(th);
      }
      table.appendChild(head);
      at += 2;
      while (at < lines.length && lines[at].includes("|") && lines[at].trim() !== "") {
        const row = from(el("tr"), offset + at);
        for (const cell of cells(lines[at])) {
          const td = el("td");
          inline(td, cell);
          row.appendChild(td);
        }
        table.appendChild(row);
        at += 1;
      }
      const wrap = el("div", "md-table-wrap");
      wrap.appendChild(table);
      root.appendChild(wrap);
      continue;
    }
    const bullet = BULLET.exec(line);
    if (bullet) {
      const ordered = /\d/.test(bullet[2]);
      const list = from(el(ordered ? "ol" : "ul", "md-list"), offset + first);
      const base = bullet[1].length;
      while (at < lines.length) {
        const item = BULLET.exec(lines[at]);
        if (!item || item[1].length < base) break;
        if (item[1].length > base) {
          // A deeper item belongs to the previous one.
          const nested: string[] = [];
          while (at < lines.length) {
            const deeper = BULLET.exec(lines[at]);
            if (!deeper || deeper[1].length <= base) break;
            nested.push(lines[at]);
            at += 1;
          }
          const last = list.lastElementChild;
          if (last) last.appendChild(markdown(nested.join("\n"), offset + at - nested.length));
          continue;
        }
        const li = from(el("li"), offset + at);
        let content = item[3];
        const task = /^\[([ xX])\]\s+(.*)$/.exec(content);
        if (task) {
          const box = el("span", `md-task${task[1] === " " ? "" : " done"}`, task[1] === " " ? "☐ " : "☑ ");
          li.appendChild(box);
          content = task[2];
        }
        inline(li, content);
        list.appendChild(li);
        at += 1;
      }
      root.appendChild(list);
      continue;
    }
    const body: string[] = [];
    // A row that opens or closes a section (▸ or ▾) is a block of its own, so that it can act.
    const disclosure = /^\s*[▸▾]\s/.test(line);
    const depth = disclosure ? Math.floor((line.length - line.trimStart().length) / 2) : 0;
    if (disclosure) {
      body.push(line.trim());
      at += 1;
    }
    while (
      !disclosure &&
      at < lines.length &&
      lines[at].trim() !== "" &&
      !/^\s*[▸▾]\s/.test(lines[at]) &&
      !/^(#{1,6}\s|\s*(```|~~~)|\s*>)/.test(lines[at]) &&
      !BULLET.test(lines[at]) &&
      !RULE.test(lines[at])
    ) {
      body.push(lines[at].trim());
      at += 1;
    }
    if (body.length === 0) {
      body.push(line);
      at += 1;
    }
    const paragraph = from(el("p", disclosure ? `md-p md-disclosure md-depth-${depth}` : "md-p"), offset + first);
    body.forEach((row, index) => {
      if (index > 0) paragraph.appendChild(document.createTextNode(" "));
      inline(paragraph, row);
    });
    root.appendChild(paragraph);
  }
  return root;
}

interface Active {
  start: number;
  stop: number;
  actions: string[];
  role?: unknown;
}

/**
 * Make the drawn markdown act where the buffer's spans say it does. A span over text
 * names an action; here it finds the block drawn from its line, and makes the words of the
 * span clickable inside it, or the whole block when the drawn words differ from the text
 * (a fold row, a list item). The text stays the content; this only reaches its actions.
 *
 * @implements REQ-VIEW.markdown_formatted_by_default
 */
export function activate(
  root: HTMLElement,
  text: string,
  spans: Active[],
  act: (action: string, at: number) => void,
  look: (role: unknown) => string = () => "",
) {
  const chars = Array.from(text);
  for (const span of spans) {
    const colour = span.role === undefined || span.role === "plain" ? "" : look(span.role);
    if (span.actions.length === 0 && !colour) continue;
    const line = chars.slice(0, span.start).filter((c) => c === "\n").length;
    const blocks = Array.from(root.querySelectorAll<HTMLElement>(`[data-line="${line}"]`));
    const block = blocks[blocks.length - 1];
    if (!block) continue;
    const words = chars.slice(span.start, span.stop).join("").trim();
    const acts = span.actions.length > 0;
    const go = (event: Event) => {
      event.stopPropagation();
      act(span.actions[0], span.start);
    };
    const walker = document.createTreeWalker(block, NodeFilter.SHOW_TEXT);
    const whole = words === (block.textContent ?? "").trim();
    let wrapped = false;
    for (let node = walker.nextNode(); node && words && !whole && !wrapped; node = walker.nextNode()) {
      const at = (node.textContent ?? "").indexOf(words);
      if (at < 0) continue;
      const after = (node as Text).splitText(at);
      after.splitText(words.length);
      const mark = el(acts ? "button" : "span", `${acts ? "md-action actionable " : ""}${colour}`.trim());
      mark.textContent = words;
      if (acts) mark.addEventListener("click", go);
      after.replaceWith(mark);
      wrapped = true;
    }
    if (!wrapped) {
      if (colour) block.classList.add(...colour.split(" "));
      if (acts) {
        block.classList.add("md-action", "actionable");
        block.tabIndex = 0;
        block.setAttribute("role", "button");
        block.addEventListener("click", go);
      }
    }
  }
}

interface Front {
  fields: [string, string][];
  clauses: [string, string][];
  lines: number;
}

/** The `---` header of a document, read as far as a card needs: its keys, and its clauses. */
function front(text: string): Front | null {
  const lines = text.replace(/\r\n?/g, "\n").split("\n");
  if (lines[0]?.trim() !== "---") return null;
  const end = lines.findIndex((line, at) => at > 0 && line.trim() === "---");
  if (end < 0) return null;
  const fields: [string, string][] = [];
  const clauses: [string, string][] = [];
  let inside = false;
  for (const line of lines.slice(1, end)) {
    const top = /^([A-Za-z_][\w-]*):\s*(.*)$/.exec(line);
    if (top) {
      inside = top[1] === "clauses";
      if (!inside) fields.push([top[1], top[2]]);
      continue;
    }
    if (!inside || line.trim() === "") continue;
    const clause = /^ {2}([A-Za-z_][\w-]*):\s*(.*)$/.exec(line);
    if (clause) clauses.push([clause[1], clause[2]]);
    else if (clauses.length > 0) clauses[clauses.length - 1][1] += ` ${line.trim()}`;
  }
  return { fields, clauses, lines: end + 1 };
}

/**
 * A document: markdown that may open with a `---` header. The page starts with its title,
 * then the header as a card (what it is, and its clauses), then the text. A header drawn as
 * a paragraph reads as one run of words.
 *
 * @implements REQ-LOOK.markdown_drawn
 */
export function document_(text: string): HTMLElement {
  const head = front(text);
  if (!head) return markdown(text);
  const rest = text.replace(/\r\n?/g, "\n").split("\n").slice(head.lines).join("\n");
  const page = markdown(rest, head.lines);
  const title = head.fields.find(([key]) => key === "title")?.[1] || head.fields.find(([key]) => key === "id")?.[1] || "";
  const card = el("div", "md-front");
  const table = el("table", "md-table md-front-table");
  for (const [key, value] of head.fields.filter(([key]) => key !== "title")) {
    const row = el("tr");
    row.append(el("th", undefined, key), el("td", undefined, value));
    table.appendChild(row);
  }
  card.appendChild(table);
  if (head.clauses.length > 0) {
    const clauses = el("table", "md-table md-front-clauses");
    for (const [name, said] of head.clauses) {
      const row = el("tr");
      const label = el("th");
      label.appendChild(el("code", "md-code", name));
      const cell = el("td");
      inline(cell, said);
      row.append(label, cell);
      clauses.appendChild(row);
    }
    card.appendChild(clauses);
  }
  const first = page.firstElementChild;
  if (first instanceof HTMLElement && first.tagName === "H1") {
    first.after(card);
  } else if (title) {
    const heading = el("h1", "md-h", title);
    page.prepend(heading, card);
  } else {
    page.prepend(card);
  }
  return page;
}
