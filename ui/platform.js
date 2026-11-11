/* Use native capabilities when available; infer the host before the first paint. */
(() => {
  "use strict";
  const host = navigator.userAgentData?.platform || navigator.platform || "";
  const inferred = /mac/i.test(host)
    ? "macos"
    : /win/i.test(host)
      ? "windows"
      : /linux/i.test(host)
        ? "linux"
        : "unknown";
  const defaults = (os) => ({
    os,
    desktopInput: os !== "unknown",
    xinput: os === "windows" || os === "linux",
    screenColor: ["windows", "macos", "linux"].includes(os),
    systemAudioBridge: os === "windows" || os === "linux" || os === "macos",
    updates: os === "windows" || os === "macos",
  });
  const platform = {
    ...defaults(inferred),
    get primaryLabel() {
      return this.os === "macos" ? "Command" : "Ctrl";
    },
    get shortcutLabel() {
      return this.os === "macos" ? "⌘ K" : "Ctrl K";
    },
    primaryModifier(event) {
      return this.os === "macos" ? event.metaKey : event.ctrlKey;
    },
    keyLabel(vk, fallback) {
      if (vk === 91 || vk === 92)
        return this.os === "macos"
          ? "Command"
          : this.os === "windows"
            ? "Windows key"
            : "Super";
      if (vk === 18 && this.os === "macos") return "Option";
      if (vk === 17 && this.os === "macos") return "Control";
      return fallback;
    },
    get appPathPlaceholder() {
      return this.os === "macos"
        ? "/Applications/App.app"
        : this.os === "windows"
          ? "C:\\Program Files\\App\\App.exe"
          : "/usr/bin/app";
    },
    apply() {
      document.body.dataset.platform = this.os;
      document
        .querySelectorAll("[data-feature]")
        .forEach((el) => (el.hidden = !this[el.dataset.feature]));
      document
        .querySelectorAll("[data-platform-only]")
        .forEach(
          (el) =>
            (el.hidden = !el.dataset.platformOnly.split(" ").includes(this.os)),
        );
    },
    update(capabilities) {
      if (!capabilities?.os) return;
      Object.assign(this, defaults(capabilities.os), capabilities);
      this.apply();
      document.dispatchEvent(
        new CustomEvent("controller-platform", { detail: this }),
      );
    },
  };

  /* Shared UI helpers (in-app confirm, desktop-only notice, aria setter). */
  const css = document.createElement("style");
  css.textContent = `
.ui-confirm-backdrop{position:fixed;inset:0;z-index:10000;background:rgba(8,14,22,.62);display:grid;place-items:center;padding:16px}
.ui-confirm{max-width:420px;width:100%;background:var(--stage-mid,#192e42);color:var(--cream,#f0f8f7);border:1px solid var(--teal-deep,#176f7b);border-radius:10px;padding:18px;box-shadow:0 18px 50px rgba(0,0,0,.5)}
.ui-confirm h3{margin:0 0 8px;font-size:1rem}.ui-confirm p{margin:0 0 14px;color:var(--parchment-dim,#d5e3df)}
.ui-confirm-actions{display:flex;gap:8px;justify-content:flex-end;flex-wrap:wrap}
.ui-confirm-actions button{font:inherit;padding:7px 14px;border-radius:6px;border:1px solid var(--line,#a8bec1);background:transparent;color:inherit;cursor:pointer}
.ui-confirm-actions button.ui-primary{background:var(--teal,#54c6c5);color:#06222a;border-color:var(--teal,#54c6c5)}
.ui-confirm-actions button.ui-danger{background:var(--danger,#c45c3e);color:#fff;border-color:var(--danger,#c45c3e)}
.ui-note{color:var(--gold-soft,#ffe0ad);font-size:.85rem}`;
  document.head.append(css);
  platform.confirm = (message, options = {}) =>
    new Promise((resolve) => {
      // One confirm at a time: a second request cancels the first.
      const open = document.querySelector(".ui-confirm-backdrop");
      if (open) open.dispatchEvent(new Event("ui-confirm-cancel"));
      const previous = document.activeElement;
      const backdrop = document.createElement("div");
      backdrop.className = "ui-confirm-backdrop";
      const box = document.createElement("div");
      box.className = "ui-confirm";
      box.setAttribute("role", "alertdialog");
      box.setAttribute("aria-modal", "true");
      const title = document.createElement("h3");
      title.textContent = options.title || "Are you sure?";
      title.id = "ui-confirm-title";
      box.setAttribute("aria-labelledby", title.id);
      const body = document.createElement("p");
      body.textContent = message;
      const actions = document.createElement("div");
      actions.className = "ui-confirm-actions";
      const cancel = document.createElement("button");
      cancel.type = "button";
      cancel.textContent = options.cancelLabel || "Cancel";
      const ok = document.createElement("button");
      ok.type = "button";
      ok.className = options.danger === false ? "ui-primary" : "ui-danger";
      ok.textContent = options.confirmLabel || "Confirm";
      actions.append(cancel, ok);
      box.append(title, body, actions);
      backdrop.append(box);
      let done = false;
      const finish = (value) => {
        if (done) return;
        done = true;
        document.removeEventListener("keydown", onKey, true);
        backdrop.remove();
        try { previous?.focus?.(); } catch {}
        resolve(value);
      };
      const onKey = (event) => {
        if (!backdrop.isConnected) { finish(false); return; }
        if (event.key === "Escape") { event.preventDefault(); event.stopPropagation(); finish(false); }
        else if (event.key === "Tab") {
          if (!box.contains(document.activeElement)) return;
          event.preventDefault();
          (document.activeElement === ok ? cancel : ok).focus();
        }
      };
      document.addEventListener("keydown", onKey, true);
      backdrop.addEventListener("ui-confirm-cancel", () => finish(false));
      cancel.addEventListener("click", () => finish(false));
      ok.addEventListener("click", () => finish(true));
      backdrop.addEventListener("pointerdown", (event) => { if (event.target === backdrop) finish(false); });
      (document.querySelector("dialog[open]") || document.body).append(backdrop);
      cancel.focus();
    });
  platform.undoToast = (message, onUndo, ms = 8000) => {
    if (window.controllerStudio?.undoToast) return window.controllerStudio.undoToast(message, onUndo);
    document.getElementById("ui-undo-toast")?.remove();
    const bar = document.createElement("div");
    bar.id = "ui-undo-toast";
    bar.setAttribute("role", "status");
    bar.style.cssText =
      "position:fixed;left:50%;bottom:24px;transform:translateX(-50%);z-index:9999;display:flex;gap:12px;align-items:center;padding:10px 14px;border-radius:8px;background:var(--stage-lift,#254158);color:var(--cream,#f0f8f7);border:1px solid var(--teal-deep,#176f7b);box-shadow:0 8px 30px rgba(0,0,0,.4)";
    const text = document.createElement("span");
    text.textContent = message;
    const undo = document.createElement("button");
    undo.type = "button";
    undo.textContent = "Undo";
    undo.style.cssText = "font:inherit;padding:4px 10px;border-radius:6px;border:1px solid var(--teal,#54c6c5);background:transparent;color:var(--teal-glow,#9be9e4);cursor:pointer";
    undo.addEventListener("click", () => { bar.remove(); onUndo(); });
    bar.append(text, undo);
    document.body.append(bar);
    setTimeout(() => bar.remove(), ms);
  };
  platform.desktopOnlyMessage = (what) =>
    `${what || "This"} is available in the desktop app only.`;
  platform.setAttrs = (node, attrs = {}) => {
    for (const [key, value] of Object.entries(attrs)) node.setAttribute(key, value);
    return node;
  };
  platform.store = {
    get(key, fallback) {
      try { const v = localStorage.getItem(key); return v === null ? fallback : JSON.parse(v); } catch { return fallback; }
    },
    set(key, value) { try { localStorage.setItem(key, JSON.stringify(value)); } catch {} },
  };
  platform.download = (name, text, type = "application/json") => {
    const url = URL.createObjectURL(new Blob([text], { type }));
    const a = document.createElement("a");
    a.href = url; a.download = name; document.body.append(a); a.click(); a.remove();
    setTimeout(() => URL.revokeObjectURL(url), 2000);
  };
  platform.pickJson = (maxBytes = 2 * 1024 * 1024) =>
    new Promise((resolve) => {
      const input = document.createElement("input");
      input.type = "file"; input.accept = ".json,application/json";
      // null = picker cancelled, undefined = unreadable or too large.
      input.addEventListener("cancel", () => resolve(null));
      input.addEventListener("change", async () => {
        const file = input.files?.[0];
        if (!file) return resolve(null);
        if (file.size > maxBytes) return resolve(undefined);
        try { resolve(JSON.parse(await file.text())); } catch { resolve(undefined); }
      });
      input.click();
    });
  /* Section tagging (declarative convention read by ui/ui-kit.js; harmless when the kit is absent). */
  platform.section = (el, id, level, title, hint, opts = {}) => {
    if (!el) return el;
    if (!el.dataset.uiSection) el.dataset.uiSection = id;
    if (!el.dataset.level) el.dataset.level = level;
    if (!el.dataset.title) el.dataset.title = title;
    if (hint && !el.dataset.hint) el.dataset.hint = hint;
    if (opts.keywords && !el.dataset.keywords) el.dataset.keywords = opts.keywords;
    if (opts.open !== undefined && !el.dataset.defaultOpen) el.dataset.defaultOpen = String(!!opts.open);
    return el;
  };
  /* Disabled-state explanation: kit hint when present, plus title and aria-describedby as fallback. */
  platform.why = (el, reason) => {
    if (!el) return el;
    const old = el.dataset.whyText;
    if ((reason || undefined) === old) return el;
    try { window.controllerUI?.why?.(el, reason || null); } catch {}
    if (reason) {
      el.title = reason;
      el.dataset.whyText = reason;
      if (!el.getAttribute("aria-describedby")) {
        const id = `${el.id || "ctl"}-why`;
        let note = document.getElementById(id);
        if (!note) { note = document.createElement("span"); note.id = id; note.hidden = true; note.className = "ui-why-fallback"; el.after(note); }
        note.textContent = reason;
        el.setAttribute("aria-describedby", id);
        el.dataset.whyOwn = "1";
      } else if (el.dataset.whyOwn) document.getElementById(el.getAttribute("aria-describedby"))?.replaceChildren(reason);
    } else if (old !== undefined) {
      if (el.title === old) el.removeAttribute("title");
      delete el.dataset.whyText;
      if (el.dataset.whyOwn) { document.getElementById(el.getAttribute("aria-describedby"))?.remove(); el.removeAttribute("aria-describedby"); delete el.dataset.whyOwn; }
    }
    return el;
  };
  /* gate(el, blocked, reason): sets disabled and explains why when blocked. */
  platform.gate = (el, blocked, reason) => {
    if (!el) return el;
    el.disabled = !!blocked;
    platform.why(el, blocked && reason ? reason : null);
    return el;
  };
  // Static panels owned by the panel modules (the markup lives in index.html, which the shell owns).
  const tagStatic = () => {
    const q = (sel) => document.querySelector(sel);
    let missing = 0;
    const section = (...args) => { if (!platform.section(...args)) missing++; };
    section(q("#lights .telemetry-panel"), "lights.forza", "tool", "Race telemetry (Forza)", "Light and grip cues from Forza Data Out", { keywords: "forza telemetry data out udp race rpm gear lights racing horizon motorsport" });
    section(q("#lights .script-panel"), "lights.rules", "tool", "Telemetry rules", "Script a light or cue from race data", { keywords: "script rhai telemetry rules code custom light cue" });
    section(q("#diagnostics .stick-checks"), "diag.drift", "advanced", "Drift and travel checks", "Center drift and circular travel", { keywords: "drift stick center circularity travel check deadzone measure" });
    section(q("#diagnostics #guided-test"), "diag.guided", "basic", "Guided button and stick test", "Press everything once", { open: true, keywords: "guided test buttons sticks triggers checklist press" });
    return missing === 0;
  };
  // Some panels (the script editor) are built by app.js after this file runs, so wait for them.
  if (!tagStatic()) {
    const watch = new MutationObserver(() => { if (tagStatic()) watch.disconnect(); });
    watch.observe(document.documentElement, { childList: true, subtree: true });
    setTimeout(() => watch.disconnect(), 15000);
  }
  window.controllerPlatform = platform;
  platform.apply();
})();
