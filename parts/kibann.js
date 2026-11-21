// kibann.js: what a part's page uses to talk to Kibann. Loaded before the
// page's own scripts, it defines `window.kibann`. Types: kibann.d.ts.
//
// A part runs in a sandboxed frame with no access to the app: everything
// goes through postMessage to the window around it, which checks what the
// part was allowed to do (its part.json's permissions).
(() => {
  "use strict";
  if (window.kibann) return;
  const host = window.parent;
  const listeners = new Map();
  const pending = new Map();
  let nextId = 1;
  let readyResolve;
  const ready = new Promise((r) => (readyResolve = r));
  let focusable = false;

  const send = (msg, transfer) => host.postMessage({ kibann: 1, ...msg }, "*", transfer || []);
  const emit = (type, value) => {
    for (const fn of listeners.get(type) || []) {
      try {
        fn(value);
      } catch (e) {
        console.error(e);
      }
    }
  };
  const call = (method, args) =>
    new Promise((resolve, reject) => {
      const id = nextId++;
      pending.set(id, { resolve, reject });
      send({ t: "call", id, method, args: args === undefined ? null : args });
    });

  /** The window's colors as CSS variables on this page: var(--k-bg). */
  const applyTheme = (theme) => {
    if (!theme) return;
    const root = document.documentElement;
    for (const [k, v] of Object.entries(theme.colors || {})) root.style.setProperty(`--k-${k}`, v);
    if (theme.font) root.style.setProperty("--k-font", theme.font);
    if (theme.mono) root.style.setProperty("--k-mono", theme.mono);
    root.style.colorScheme = theme.dark ? "dark" : "light";
  };

  const kibann = {
    /** Resolves once the first state arrived. */
    ready,
    /** What the screen gave this block (`"props"` in screen.json). */
    props: null,
    /** The window as of the last update (see `on("state")`). */
    state: null,
    /** What this part may do (its part.json's permissions, once allowed). */
    granted: null,
    /** This part's name. */
    name: null,

    /** Call `fn` with the state now (when there is one) and on every
     * change. Returns a function that stops it. Also `"props"`, `"show"`
     * (the block was shown or hidden: true or false) and `"theme"`. */
    on(type, fn) {
      if (!listeners.has(type)) listeners.set(type, new Set());
      listeners.get(type).add(fn);
      if (type === "state" && kibann.state) fn(kibann.state);
      if (type === "props" && kibann.props !== null) fn(kibann.props);
      return () => listeners.get(type).delete(fn);
    },

    /** Do something in the window: `kibann.run("pane.focus", {pane: 3})`.
     * Resolves with its result; rejects with why it couldn't. */
    run(action, args) {
      return call("run", { action, args: args || {} });
    },

    /** Run a program the part's permissions list, without a shell:
     * `kibann.exec("git", ["status", "--porcelain"], {cwd})`.
     * Resolves with `{code, stdout, stderr}`. */
    exec(program, args, opts) {
      return call("exec", { program, args: args || [], cwd: (opts && opts.cwd) || null, timeout: (opts && opts.timeout) || null });
    },

    /** A picture of what pane `pane` shows, `width` CSS px wide (the
     * height follows), or null. Needs the `read` permission. */
    thumbnail(pane, width, height) {
      return call("thumbnail", { pane, width: width || 320, height });
    },

    /** Draw pane `pane` into `canvas`, filling it (cropped to keep the
     * pane's shape, from the top, or from the bottom when `bottom`). */
    async drawPane(canvas, pane, opts) {
      const dpr = window.devicePixelRatio || 1;
      const w = Math.max(1, Math.round(canvas.clientWidth || canvas.width / dpr));
      const h = Math.max(1, Math.round(canvas.clientHeight || canvas.height / dpr));
      const img = await kibann.thumbnail(pane, w, h);
      if (canvas.width !== Math.round(w * dpr) || canvas.height !== Math.round(h * dpr)) {
        canvas.width = Math.round(w * dpr);
        canvas.height = Math.round(h * dpr);
      }
      const ctx = canvas.getContext("2d");
      ctx.clearRect(0, 0, canvas.width, canvas.height);
      if (!img) return false;
      ctx.drawImage(img, 0, 0, canvas.width, canvas.height);
      if (img.close) img.close();
      return true;
    },

    /** Values kept for this part between runs (JSON). */
    store: {
      get(key) {
        return call("store.get", { key });
      },
      set(key, value) {
        return call("store.set", { key, value });
      },
    },

    /** Show a short notice in the window's corner. */
    notify(text) {
      return call("run", { action: "notify", args: { text: String(text) } });
    },
  };

  window.addEventListener("message", (e) => {
    if (e.source !== host) return;
    const m = e.data;
    if (!m || m.kibann !== 1) return;
    switch (m.t) {
      case "init":
        kibann.name = m.name;
        kibann.granted = m.granted;
        focusable = !!m.focusable;
        kibann.props = m.props === undefined ? null : m.props;
        applyTheme(m.theme);
        kibann.state = m.state;
        emit("props", kibann.props);
        emit("theme", m.theme);
        emit("state", m.state);
        readyResolve();
        break;
      case "state":
        kibann.state = m.state;
        emit("state", m.state);
        break;
      case "theme":
        applyTheme(m.theme);
        emit("theme", m.theme);
        break;
      case "props":
        kibann.props = m.props;
        emit("props", m.props);
        break;
      case "show":
        emit("show", m.shown);
        break;
      case "reply": {
        const p = pending.get(m.id);
        if (!p) return;
        pending.delete(m.id);
        if (m.ok) p.resolve(m.value);
        else p.reject(new Error(m.error));
        break;
      }
    }
  });

  // Shortcuts work while a part has the keyboard: keys with Cmd or Ctrl
  // go to the window too.
  window.addEventListener(
    "keydown",
    (e) => {
      if (!(e.metaKey || e.ctrlKey) || ["Meta", "Control", "Shift", "Alt"].includes(e.key)) return;
      send({ t: "key", key: e.key, code: e.code, alt: e.altKey, ctrl: e.ctrlKey, meta: e.metaKey, shift: e.shiftKey });
    },
    true,
  );

  // A click leaves the keyboard with the terminal, unless the part takes
  // keys (part.json's "focusable") or the click was in a text field.
  window.addEventListener(
    "pointerup",
    (e) => {
      if (focusable) return;
      const t = e.target;
      if (t && t.closest && t.closest("input, textarea, select, [contenteditable]")) return;
      send({ t: "release" });
    },
    true,
  );

  window.addEventListener("error", (e) => send({ t: "error", message: String(e.message || e) }));
  window.addEventListener("unhandledrejection", (e) => send({ t: "error", message: String((e.reason && e.reason.message) || e.reason) }));

  window.kibann = kibann;
  send({ t: "hello" });
})();
