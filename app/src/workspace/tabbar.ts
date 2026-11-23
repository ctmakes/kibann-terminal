// The tab bar: a button per tab (its state dot, title and number, a close
// button) and "?" for the shortcut sheet. Shown only with two tabs or more.

import { tooltip } from "./tooltip";
import "../ui.css";
import "./tabbar.css";

export interface TabButton {
  readonly el: HTMLDivElement;
  /** The tab's agent state dot. */
  readonly dot: HTMLSpanElement;
  readonly label: HTMLSpanElement;
  readonly number: HTMLSpanElement;
}

/** What the bar shows of a tab. */
export interface TabLook {
  button: TabButton;
  title: string;
  active: boolean;
}

export class TabBar {
  readonly el: HTMLDivElement;
  /** "?", kept last. */
  private readonly help: HTMLDivElement;

  constructor(opts: { onHelp(): void; helpKey(): string }) {
    this.el = document.createElement("div");
    this.el.className = "tab-bar";
    // Inline, as screen.css's `.screen-block > .tab-bar` would beat a class.
    Object.assign(this.el.style, { display: "none", flex: "1 1 auto" });
    this.help = document.createElement("div");
    this.help.className = "tab-help";
    this.help.textContent = "?";
    tooltip(this.help, "Keyboard shortcuts", opts.helpKey);
    this.help.addEventListener("mousedown", (e) => {
      e.preventDefault(); // keep focus in the terminal
      opts.onHelp();
    });
    this.el.append(this.help);
  }

  /** A button for a new tab, with `dot` in it. A middle click or × closes
   * the tab, a click shows it. */
  button(dot: HTMLSpanElement, on: { activate(): void; close(): void }): TabButton {
    const el = document.createElement("div");
    el.className = "tab";
    const label = document.createElement("span");
    label.className = "tab-title";
    const x = document.createElement("span");
    x.className = "tab-close";
    x.textContent = "×";
    // Its number: Cmd+1–8 goes to it (Ctrl elsewhere).
    const number = document.createElement("span");
    number.className = "tab-number";
    el.append(x, dot, label, number);
    el.addEventListener("mousedown", (e) => {
      e.preventDefault(); // keep focus in the terminal
      if (e.button === 1 || e.target === x) on.close();
      else if (e.button === 0) on.activate();
    });
    this.el.append(el);
    return { el, dot, label, number };
  }

  /** Show `tabs`, in this order. */
  render(tabs: TabLook[]): void {
    this.el.style.display = tabs.length > 1 ? "flex" : "none";
    for (const t of tabs) this.el.append(t.button.el);
    this.el.append(this.help);
    tabs.forEach((t, i) => {
      const b = t.button;
      b.el.style.background = t.active ? "var(--k-tabActive)" : "var(--k-tab)";
      b.el.style.color = t.active ? "var(--k-bright)" : "var(--k-text)";
      b.label.textContent = t.title;
      b.el.title = t.title;
      b.number.textContent = String(i + 1);
    });
  }
}
