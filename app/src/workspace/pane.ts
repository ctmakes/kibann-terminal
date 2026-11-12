// The header above each pane's terminal: where it is (directory, or repo,
// worktree and branch), what changed, and the agent dot. Always there, so
// moving between directories never resizes the terminal.

import type { GitOp, PaneDir } from "../proto";

export const HEADER_H = 20;

/** Every pane's place, by pane id (JSON object keys are strings). */
export type GitMap = Record<string, PaneDir>;

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

const COLORS: Record<PartKind, string> = {
  place: "inherit",
  worktree: "inherit",
  branch: "inherit",
  op: "#f0883e",
  conflicts: "#f85149",
  changed: "#d29922",
  sync: "inherit",
};

export class PaneHeader {
  readonly el: HTMLDivElement;
  private readonly text: HTMLDivElement;
  private last = "";

  constructor(dot: HTMLElement) {
    this.el = document.createElement("div");
    this.el.className = "pane-header";
    Object.assign(this.el.style, {
      position: "absolute",
      left: "0",
      right: "0",
      top: "0",
      height: `${HEADER_H}px`,
      display: "flex",
      alignItems: "center",
      gap: "8px",
      padding: "0 8px",
      boxSizing: "border-box",
      background: "#141414",
      color: "#7d7d7d",
      font: "11px -apple-system, system-ui, sans-serif",
      whiteSpace: "nowrap",
      overflow: "hidden",
    });
    this.text = document.createElement("div");
    this.text.className = "pane-place";
    Object.assign(this.text.style, {
      flex: "1 1 auto",
      minWidth: "0",
      display: "flex",
      gap: "8px",
      overflow: "hidden",
    });
    this.el.append(this.text, dot);
  }

  setActive(active: boolean): void {
    this.el.style.color = active ? "#c8c8c8" : "#7d7d7d";
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
        // The branch gives way first when narrow, then the place.
        const shrink = p.kind === "branch" ? "1" : p.kind === "place" || p.kind === "worktree" ? "0.5" : "0";
        Object.assign(s.style, {
          flex: `0 ${shrink} auto`,
          minWidth: shrink === "0" ? "auto" : "2em",
          overflow: "hidden",
          textOverflow: "ellipsis",
          color: COLORS[p.kind],
        });
        return s;
      }),
    );
    this.el.title = place ? (place.git ? `${place.git.root}` : place.dir) : "";
  }
}
