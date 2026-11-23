// The header above each pane's terminal: where it is (directory, or repo,
// worktree and branch), what changed, and the agent dot. Always there, so
// moving between directories never resizes the terminal.

import type { GitOp, PaneDir } from "../proto";
import "../ui.css";
import "./pane.css";

export const HEADER_H = 20;

/** Every pane's place, by pane id (JSON object keys are strings). */
export type GitMap = Record<string, PaneDir>;

/** What a pane is called: its program's title, or "shell" until it sets one. */
export function paneName(title: string | undefined): string {
  return title || "shell";
}

export type PartKind = "place" | "worktree" | "branch" | "op" | "conflicts" | "changed" | "sync";

export interface Part {
  kind: PartKind;
  text: string;
}

const OPS: Record<GitOp, string> = {
  merge: "MERGING",
  rebase: "REBASING",
  apply: "APPLYING",
  cherry_pick: "CHERRY-PICKING",
  revert: "REVERTING",
  bisect: "BISECTING",
};

/** What the header says, piece by piece. */
export function headerParts(place: PaneDir | undefined): Part[] {
  if (!place) return [];
  const g = place.git;
  if (!g) return [{ kind: "place", text: place.shown }];
  const parts: Part[] = [{ kind: "place", text: g.repo }];
  if (g.worktree) parts.push({ kind: "worktree", text: `› ${g.worktree}` });
  parts.push({ kind: "branch", text: `⎇ ${g.branch ?? g.detached ?? "?"}` });
  if (g.operation) parts.push({ kind: "op", text: OPS[g.operation] });
  if (g.conflicts > 0) parts.push({ kind: "conflicts", text: `!${g.conflicts}` });
  if (g.changed > 0) parts.push({ kind: "changed", text: `●${g.changed}` });
  const sync = [g.ahead > 0 ? `↑${g.ahead}` : "", g.behind > 0 ? `↓${g.behind}` : ""].filter(Boolean);
  if (sync.length) parts.push({ kind: "sync", text: sync.join(" ") });
  return parts;
}

/** The branch a tab shows after its title. */
export function branchOf(place: PaneDir | undefined): string | null {
  const g = place?.git;
  return g ? (g.branch ?? g.detached ?? null) : null;
}

export class PaneHeader {
  readonly el: HTMLDivElement;
  private readonly text: HTMLDivElement;
  private last = "";

  /** `onDiff`: the changed count (●N) was clicked. */
  constructor(dot: HTMLElement, onDiff?: () => void) {
    this.el = document.createElement("div");
    this.el.className = "pane-header";
    Object.assign(this.el.style, { height: `${HEADER_H}px`, color: "var(--k-muted)" });
    this.text = document.createElement("div");
    this.text.className = "pane-place";
    this.el.append(this.text, dot);
    this.el.addEventListener("mousedown", (e) => {
      if ((e.target as HTMLElement).classList.contains("pane-changed") && onDiff) {
        e.preventDefault();
        e.stopPropagation();
        onDiff();
      }
    });
  }

  setActive(active: boolean): void {
    this.el.style.color = active ? "var(--k-strong)" : "var(--k-muted)";
  }

  show(place: PaneDir | undefined): void {
    const parts = headerParts(place);
    const key = JSON.stringify(parts);
    if (key === this.last) return;
    this.last = key;
    this.text.replaceChildren(
      ...parts.map((p) => {
        const s = document.createElement("span");
        s.className = `pane-${p.kind}`;
        s.textContent = p.text;
        return s;
      }),
    );
    this.el.title = place ? (place.git ? `${place.git.root}` : place.dir) : "";
  }
}
