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

function isTableRule(row: string | undefined): boolean {
  return row !== undefined && /^\s*\|?\s*:?-{2,}:?\s*(\|\s*:?-{2,}:?\s*)*\|?\s*$/.test(row) && row.includes("-");
}

export function markdown(text: string): HTMLElement {
  const root = el("div", "md");
  const lines = text.replace(/\r\n?/g, "\n").split("\n");
  let at = 0;
  while (at < lines.length) {
    const line = lines[at];
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
      const pre = el("pre", "md-pre");
      if (fence[3]) pre.dataset.lang = fence[3];
      pre.appendChild(el("code", undefined, body.join("\n")));
      root.appendChild(pre);
      continue;
    }
    const heading = /^(#{1,6})\s+(.*?)\s*#*\s*$/.exec(line);
    if (heading) {
      const node = el(`h${heading[1].length}`, "md-h");
      inline(node, heading[2]);
      root.appendChild(node);
      at += 1;
      continue;
    }
    if (RULE.test(line)) {
      root.appendChild(el("hr", "md-hr"));
      at += 1;
      continue;
    }
    if (line.trimStart().startsWith(">")) {
      const body: string[] = [];
      while (at < lines.length && lines[at].trimStart().startsWith(">")) {
        body.push(lines[at].trimStart().replace(/^>\s?/, ""));
        at += 1;
      }
      const quote = el("blockquote", "md-quote");
      quote.appendChild(markdown(body.join("\n")));
      root.appendChild(quote);
      continue;
    }
    if (line.includes("|") && isTableRule(lines[at + 1])) {
      const table = el("table", "md-table");
      const head = el("tr");
      for (const cell of cells(line)) {
        const th = el("th");
        inline(th, cell);
        head.appendChild(th);
      }
      table.appendChild(head);
      at += 2;
      while (at < lines.length && lines[at].includes("|") && lines[at].trim() !== "") {
        const row = el("tr");
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
      const list = el(ordered ? "ol" : "ul", "md-list");
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
          if (last) last.appendChild(markdown(nested.join("\n")));
          continue;
        }
        const li = el("li");
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
    while (
      at < lines.length &&
      lines[at].trim() !== "" &&
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
    const paragraph = el("p", "md-p");
    body.forEach((row, index) => {
      if (index > 0) paragraph.appendChild(document.createTextNode(" "));
      inline(paragraph, row);
    });
    root.appendChild(paragraph);
  }
  return root;
}
