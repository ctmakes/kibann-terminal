import type { InputEvent } from "../proto";

export interface ImeSink {
  send(e: InputEvent): void;
  preedit(text: string): void;
}

/**
 * Composition (IME, dead keys on WebKit) and text that arrives without a
 * keydown we handled, such as the emoji picker or dictation.
 */
export class ImeInput {
  composing = false;
  private lastCommit = "";
  private lastCommitAt = 0;
  private readonly offs: Array<() => void> = [];

  constructor(
    readonly target: HTMLTextAreaElement,
    private readonly sink: ImeSink,
  ) {
    const on = <K extends keyof HTMLElementEventMap>(type: K, fn: (e: HTMLElementEventMap[K]) => void) => {
      target.addEventListener(type, fn);
      this.offs.push(() => target.removeEventListener(type, fn));
    };
    on("compositionstart", () => {
      this.composing = true;
      this.sink.preedit("");
    });
    on("compositionupdate", (e) => this.sink.preedit(e.data ?? ""));
    on("compositionend", (e) => {
      this.composing = false;
      this.sink.preedit("");
      const data = e.data ?? "";
      if (data) {
        this.sink.send({ kind: "text", data });
        this.lastCommit = data;
        this.lastCommitAt = performance.now();
      }
      this.clear();
    });
    on("input", (e) => this.onInput(e as InputEvent_));
    on("blur", () => {
      if (this.composing) {
        this.composing = false;
        this.sink.preedit("");
      }
    });
  }

  private onInput(e: InputEvent_): void {
    if (e.isComposing || this.composing) return;
    if (e.inputType === "insertText" || e.inputType === "insertReplacementText") {
      const data = e.data ?? this.target.value;
      // Some engines repeat the committed composition as an input event.
      const dup = data === this.lastCommit && performance.now() - this.lastCommitAt < 100;
      if (data && !dup) this.sink.send({ kind: "text", data });
    }
    this.clear();
  }

  private clear(): void {
    this.target.value = "";
  }

  dispose(): void {
    for (const off of this.offs) off();
  }
}

type InputEvent_ = globalThis.InputEvent;
