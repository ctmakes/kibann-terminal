// What's New: CHANGELOG.md (carried in the app), as a sheet over the
// workspace like the shortcut sheet. Help → What's New, or the notice after
// an update.

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
    el.textContent = m[1] ?? m[2]!;
    if (m[1] !== undefined) {
      Object.assign(el.style, {
        font: "11px ui-monospace, SFMono-Regular, Menlo, monospace",
        background: "var(--k-kbd)",
        borderRadius: "3px",
        padding: "0 3px",
      });
    } else {
      el.style.color = "var(--k-bright)";
    }
    out.push(el);
    at = re.lastIndex;
  }
  if (at < text.length) out.push(document.createTextNode(text.slice(at)));
  return out;
}

/** The sheet: an overlay over the workspace. */
export class ChangesSheet {
  readonly el: HTMLDivElement;
  visible = false;

  constructor() {
    this.el = document.createElement("div");
    this.el.className = "changes-sheet";
    Object.assign(this.el.style, {
      position: "absolute",
      inset: "0",
      zIndex: "30",
      display: "none",
      alignItems: "center",
      justifyContent: "center",
      background: "var(--k-scrim)",
      font: "12px -apple-system, system-ui, sans-serif",
      color: "var(--k-strong)",
    });
    // A click outside the panel closes it.
    this.el.addEventListener("mousedown", (e) => {
      if (e.target === this.el) {
        e.preventDefault();
        this.hide();
      }
    });
  }

  /** Show the changelog, with the running version marked. */
  show(changelog: string, current: string): void {
    const panel = document.createElement("div");
    Object.assign(panel.style, {
      background: "var(--k-panel)",
      border: "1px solid var(--k-divider)",
      borderRadius: "10px",
      boxShadow: "0 12px 40px var(--k-shadow)",
      padding: "16px 22px 18px",
      width: "min(640px, calc(100% - 32px))",
      maxHeight: "calc(100% - 32px)",
      overflowY: "auto",
      boxSizing: "border-box",
      lineHeight: "1.5",
      userSelect: "text",
    });
    const head = document.createElement("div");
    Object.assign(head.style, { display: "flex", alignItems: "baseline", marginBottom: "6px" });
    const title = document.createElement("div");
    title.textContent = "What's New";
    Object.assign(title.style, { flex: "1", fontSize: "14px", fontWeight: "600", color: "var(--k-bright)" });
    const hint = document.createElement("div");
    hint.textContent = "Esc to close";
    hint.style.color = "var(--k-muted)";
    head.append(title, hint);
    panel.append(head);

    const releases = parseChangelog(changelog);
    if (releases.length === 0) {
      const p = document.createElement("p");
      p.textContent = "No release notes in this build.";
      panel.append(p);
    }
    for (const r of releases) {
      const section = document.createElement("section");
      section.className = "changes-release";
      const h = document.createElement("div");
      Object.assign(h.style, { display: "flex", gap: "8px", alignItems: "baseline", margin: "14px 0 4px" });
      const v = document.createElement("span");
      v.textContent = r.version;
      Object.assign(v.style, { fontSize: "13px", fontWeight: "600", color: "var(--k-bright)" });
      const when = document.createElement("span");
      when.textContent = r.version === current ? `${r.when} · this version` : r.when;
      when.style.color = "var(--k-muted)";
      h.append(v, when);
      section.append(h);
      for (const b of r.blocks) {
        if (b.t === "list") {
          const ul = document.createElement("ul");
          Object.assign(ul.style, { margin: "4px 0", paddingLeft: "18px" });
          for (const it of b.items) {
            const li = document.createElement("li");
            li.style.margin = "2px 0";
            li.append(...inline(it));
            ul.append(li);
          }
          section.append(ul);
        } else {
          const p = document.createElement(b.t === "h" ? "div" : "p");
          p.append(...inline(b.text));
          if (b.t === "h") Object.assign(p.style, { fontWeight: "600", margin: "8px 0 2px" });
          else p.style.margin = "4px 0";
          section.append(p);
        }
      }
      panel.append(section);
    }
    this.el.replaceChildren(panel);
    this.visible = true;
    this.el.style.display = "flex";
  }

  hide(): void {
    this.visible = false;
    this.el.style.display = "none";
  }
}
