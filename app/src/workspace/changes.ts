// What's New: CHANGELOG.md (carried in the app), shown in Settings →
// Updates (also Help → What's New, and the notice after an update).

import "./changes.css";

/** A changelog section: `## [0.1.0] - 2026-11-14`. */
export interface Release {
  version: string;
  /** The date, "Unreleased", or "Included in 0.2.0". */
  when: string;
  blocks: Block[];
}

/** A paragraph, a list (each item one string), or a `###` heading. */
export type Block = { t: "p"; text: string } | { t: "list"; items: string[] } | { t: "h"; text: string };

/** The sections of a Keep a Changelog style file, newest first as written.
 * The file's own introduction (before the first section) is left out. */
export function parseChangelog(text: string): Release[] {
  const out: Release[] = [];
  let cur: Release | null = null;
  // The paragraph or list item being joined from wrapped lines.
  let para: string[] | null = null;
  let list: string[] | null = null;
  const flush = () => {
    if (!cur) return;
    if (para) cur.blocks.push({ t: "p", text: para.join(" ") });
    para = null;
  };
  const endList = () => {
    if (cur && list) cur.blocks.push({ t: "list", items: list });
    list = null;
  };
  for (const raw of text.split("\n")) {
    const line = raw.trimEnd();
    const head = /^## \[([^\]]+)\](?:\s*-\s*(.*))?$/.exec(line);
    if (head) {
      flush();
      endList();
      cur = { version: head[1]!, when: (head[2] ?? "").trim(), blocks: [] };
      out.push(cur);
      continue;
    }
    if (!cur) continue;
    if (line.trim() === "") {
      flush();
      endList();
      continue;
    }
    const sub = /^###\s+(.*)$/.exec(line);
    if (sub) {
      flush();
      endList();
      cur.blocks.push({ t: "h", text: sub[1]! });
      continue;
    }
    const bullet = /^[-*]\s+(.*)$/.exec(line);
    if (bullet) {
      flush();
      list ??= [];
      list.push(bullet[1]!);
      continue;
    }
    if (list && /^\s+/.test(raw)) {
      // A wrapped list item.
      list[list.length - 1] += " " + line.trim();
      continue;
    }
    endList();
    para ??= [];
    para.push(line.trim());
  }
  flush();
  endList();
  return out;
}

/** `code` and **bold** in a line of text, as DOM nodes (never HTML). */
export function inline(text: string): Node[] {
  const out: Node[] = [];
  const re = /`([^`]+)`|\*\*([^*]+)\*\*/g;
  let at = 0;
  for (let m = re.exec(text); m; m = re.exec(text)) {
    if (m.index > at) out.push(document.createTextNode(text.slice(at, m.index)));
    const el = document.createElement(m[1] !== undefined ? "code" : "strong");
    el.className = m[1] !== undefined ? "changes-code" : "changes-strong";
    el.textContent = m[1] ?? m[2]!;
    out.push(el);
    at = re.lastIndex;
  }
  if (at < text.length) out.push(document.createTextNode(text.slice(at)));
  return out;
}

/** The release notes, newest first, with the running version marked:
 * Settings → Updates. */
export function changesView(changelog: string, current: string): HTMLDivElement {
  const box = document.createElement("div");
  box.className = "changes";
  const releases = parseChangelog(changelog);
  if (releases.length === 0) {
    const p = document.createElement("p");
    p.textContent = "No release notes in this build.";
    box.append(p);
  }
  for (const r of releases) {
    const section = document.createElement("section");
    section.className = "changes-release";
    const h = document.createElement("div");
    h.className = "changes-head";
    const v = document.createElement("span");
    v.className = "changes-version";
    v.textContent = r.version;
    const when = document.createElement("span");
    when.className = "changes-when";
    when.textContent = r.version === current ? `${r.when} · this version` : r.when;
    h.append(v, when);
    section.append(h);
    for (const b of r.blocks) {
      if (b.t === "list") {
        const ul = document.createElement("ul");
        ul.className = "changes-list";
        for (const it of b.items) {
          const li = document.createElement("li");
          li.className = "changes-item";
          li.append(...inline(it));
          ul.append(li);
        }
        section.append(ul);
      } else {
        const p = document.createElement(b.t === "h" ? "div" : "p");
        p.className = b.t === "h" ? "changes-h" : "changes-p";
        p.append(...inline(b.text));
        section.append(p);
      }
    }
    box.append(section);
  }
  return box;
}
