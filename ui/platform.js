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
    screenColor: os === "windows",
    systemAudioBridge: os === "windows" || os === "linux" || os === "macos",
    updates: os === "windows",
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
  window.controllerPlatform = platform;
  platform.apply();
})();
