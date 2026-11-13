// A tooltip with a label and its shortcut ("Collapse sidebar  ⇧⌘B"), shown
// shortly after the pointer rests on an element, below it or above. One
// element serves them all.

const DELAY_MS = 450;

let tip: HTMLDivElement | null = null;
let timer = 0;

function tipEl(): HTMLDivElement {
  if (tip) return tip;
  tip = document.createElement("div");
  tip.className = "tooltip";
  Object.assign(tip.style, {
    position: "fixed",
    zIndex: "40",
    display: "none",
    gap: "10px",
    alignItems: "baseline",
    padding: "4px 8px",
    borderRadius: "6px",
    background: "var(--k-panel)",
    border: "1px solid var(--k-divider)",
    boxShadow: "0 4px 14px var(--k-shadow)",
    color: "var(--k-strong)",
    font: "12px -apple-system, system-ui, sans-serif",
    whiteSpace: "nowrap",
    pointerEvents: "none",
  });
  document.body.append(tip);
  return tip;
}

function hide(): void {
  clearTimeout(timer);
  if (tip) tip.style.display = "none";
}

/** Show `label` (and `key`, dimmer) by `el` while the pointer rests on
 * it: below it, or `above` (as for a bar at the bottom), or above anyway
 * when there's no room below. `label` may be a function, for a label that
 * follows state. */
export function tooltip(
  el: HTMLElement,
  label: string | (() => string),
  key: string | (() => string) = "",
  above = false,
): void {
  el.addEventListener("mouseenter", () => {
    clearTimeout(timer);
    timer = window.setTimeout(() => {
      const t = tipEl();
      const name = document.createElement("span");
      name.textContent = typeof label === "function" ? label() : label;
      const k = document.createElement("span");
      k.textContent = typeof key === "function" ? key() : key;
      k.style.color = "var(--k-muted)";
      t.replaceChildren(name, ...(k.textContent ? [k] : []));
      t.style.display = "flex";
      const r = el.getBoundingClientRect();
      const w = t.offsetWidth;
      t.style.left = `${Math.max(4, Math.min(window.innerWidth - w - 4, r.left))}px`;
      const h = t.offsetHeight;
      const up = above || r.bottom + 6 + h > window.innerHeight - 4;
      t.style.top = `${up ? r.top - h - 6 : r.bottom + 6}px`;
    }, DELAY_MS);
  });
  el.addEventListener("mouseleave", hide);
  el.addEventListener("mousedown", hide);
}
