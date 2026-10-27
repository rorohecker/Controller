/* Profile tools, macro editor, and local integration diagnostics. */
(() => {
  "use strict";
  const platform = window.controllerPlatform;
  const app = window.controllerStudio;
  if (!app) return;
  const $ = (s, root = document) => root.querySelector(s),
    $$ = (s) => [...document.querySelectorAll(s)];
  const {
    state,
    profile,
    clone,
    toast,
    send,
    invoke,
    native,
    gameNode: node,
    gameSelect: select,
    gameField: field,
  } = app;
  const button = (text, run, id) => {
    const b = node("button", { type: "button", class: "quiet-action", text });
    if (id) b.id = id;
    b.addEventListener("click", run);
    return b;
  };
  const text = (value, tag = "p") => node(tag, { text: value });
  const label = (name, input) => field(name, input);
  const number = (name, value, min, max, change, step = 1) => {
    const input = node("input", { type: "number", value, min, max, step });
    input.setAttribute("aria-label", name);
    input.addEventListener("change", () => {
      const v = Number(input.value);
      if (
        !Number.isFinite(v) ||
        v < min ||
        v > max ||
        Math.abs(v / step - Math.round(v / step)) > 0.00001
      ) {
        input.value = value;
        toast(`Enter ${min}–${max}.`);
        return;
      }
      change(v);
    });
    return label(name, input);
  };
  function card(title, id) {
    const box = node("section", { class: "studio-card framed", id });
    box.append(node("div", { class: "panel-cap", text: title }));
    const body = node("div", { class: "studio-body" });
    box.append(body);
    return { box, body };
  }
  function download(data, name) {
    const url = URL.createObjectURL(
      new Blob([JSON.stringify(data, null, 2)], { type: "application/json" }),
    );
    const link = node("a", { href: url, download: name });
    link.click();
    setTimeout(() => URL.revokeObjectURL(url), 1000);
  }
  function dialog(title) {
    const d = node("dialog", { class: "studio-dialog" }),
      head = node("h2", { text: title }),
      body = node("div", { class: "studio-dialog-body" }),
      footer = node("div", { class: "studio-toolbar" });
    head.id = `dialog-${crypto.randomUUID()}`;
    d.setAttribute("aria-labelledby", head.id);
    d.append(head, body, footer);
    document.body.append(d);
    d.addEventListener("click", (e) => {
      if (
        e.target === d &&
        e.offsetX >= 0 &&
        e.offsetX <= d.clientWidth &&
        e.offsetY >= 0 &&
        e.offsetY <= d.clientHeight
      )
        return;
      if (e.target === d) d.close();
    });
    d.addEventListener("close", () => d.remove(), { once: true });
    return { d, body, footer };
  }
  const checkbox = (name, value, change) => {
    const input = node("input", { type: "checkbox", checked: value });
    input.addEventListener("change", () => change(input.checked));
    return label(name, input);
  };
  let query = "",
    onlyFavorites = false;
  const search = node("input", {
    id: "profile-search",
    type: "search",
    placeholder: "Search profiles",
    maxLength: 120,
  });
  search.setAttribute("aria-label", "Search profiles");
  search.addEventListener("input", () => {
    query = search.value.trim().toLowerCase();
    app.renderProfiles();
  });
  const filter = node("div", { class: "studio-profile-filter" });
  filter.append(
    search,
    checkbox("Favorites", false, (v) => {
      onlyFavorites = v;
      app.renderProfiles();
    }),
  );
  $("#profile-list").before(filter);
  const favorite = button(
    "Favorite",
    () => {
      profile().favorite = !profile().favorite;
      app.saveProfiles();
      app.renderProfiles();
      renderFavorite();
    },
    "favorite-profile",
  );
  const exportOne = button(
    "Export this profile",
    async () => {
      if (!(await app.commitProfileDraft())) return;
      const p = clone(profile()),
        ids = new Set(p.weaponBindings.map((b) => b.presetId));
      const data = {
        schemaVersion: 3,
        activeId: p.id,
        autoApply: false,
        profiles: [p],
        triggerPresets: app.getPresets().filter((p) => ids.has(p.id)),
      };
      if (native) {
        try {
          toast(`Exported to ${await invoke("export_profiles", { data })}`);
        } catch (e) {
          toast(e);
        }
      } else
        download(data, `${p.name.replace(/[^\w -]/g, "_") || "profile"}.json`);
    },
    "export-profile",
  );
  $("#export-profiles").parentElement.append(exportOne, favorite);
  function renderFavorite() {
    favorite.textContent = profile().favorite ? "★ Favorited" : "☆ Favorite";
    favorite.setAttribute("aria-pressed", String(!!profile().favorite));
  }

  // Native saves snapshot the previous archive. Browser previews keep the same bounded history.
  const backupCard = card("Profile recovery", "profile-recovery");
  $("#profiles").append(backupCard.box);
  const backupList = node("div", { id: "backup-list", class: "studio-list" }),
    backupStatus = text(
      "Backups stay on this computer. The newest 20 are kept.",
    );
  backupStatus.setAttribute("role", "status");
  backupCard.body.append(
    node("div", { class: "studio-toolbar" }, [
      button("Back up now", () => refreshBackups(true), "backup-now"),
      button("Refresh history", () => refreshBackups(false), "refresh-backups"),
    ]),
    backupStatus,
    backupList,
  );
  function browserHistory() {
    try {
      return JSON.parse(
        localStorage.getItem("controller.profileBackups") || "[]",
      );
    } catch {
      return [];
    }
  }
  function browserBackup(force = false) {
    const archive = app.archive(),
      rows = browserHistory(),
      now = Date.now();
    if (!force && rows[0] && now - rows[0].createdMs < 60000) return rows;
    if (
      rows[0] &&
      JSON.stringify(rows[0].data) === JSON.stringify(archive) &&
      !force
    )
      return rows;
    rows.unshift({
      id: `profiles-${now}.json`,
      createdMs: now,
      profiles: archive.profiles.length,
      data: archive,
    });
    localStorage.setItem(
      "controller.profileBackups",
      JSON.stringify(rows.slice(0, 20)),
    );
    return rows.slice(0, 20);
  }
  async function refreshBackups(create = false) {
    try {
      if (create && !(await app.commitProfileDraft())) return;
      const rows = native
        ? await invoke("profile_backups", { create })
        : create
          ? browserBackup(true)
          : browserHistory();
      backupList.replaceChildren();
      for (const row of rows || []) {
        const line = node("div", { class: "studio-list-row" });
        line.append(
          text(
            `${new Date(row.createdMs).toLocaleString()} · ${row.profiles} profiles`,
            "span",
          ),
          button("Restore", async () => {
            const view = dialog("Restore this backup?");
            view.body.append(
              text(
                "Current profiles will be backed up first. Controller effects will stop.",
              ),
            );
            view.footer.append(
              button("Cancel", () => view.d.close()),
              button("Restore", async () => {
                try {
                  await send({ action: "reset" });
                  const data = native
                    ? await invoke("restore_profile_backup", { id: row.id })
                    : (browserBackup(true), row.data);
                  if (await app.replaceArchive(data)) {
                    histories.clear();
                    renderProfile();
                    toast("Backup restored. Press Apply when ready.");
                    view.d.close();
                    refreshBackups();
                  }
                } catch (e) {
                  toast(`Restore failed: ${e}`);
                }
              }),
            );
            view.d.showModal();
          }),
        );
        backupList.append(line);
      }
      backupStatus.textContent = rows?.length
        ? "Choose a backup to restore."
        : "No backups yet. Use Back up now, or edit and save a profile.";
    } catch (e) {
      backupStatus.textContent = `Backups unavailable: ${e}`;
    }
  }
  function previewImport(data) {
    return new Promise((resolve) => {
      const view = dialog("Import profiles");
      view.body.append(
        text(
          `${data.profiles.length} profiles and ${data.triggerPresets?.length || 0} trigger presets. Existing profiles are kept.`,
        ),
      );
      const list = node("ul");
      data.profiles.forEach((p) =>
        list.append(
          text(
            `${p.name}${state.profiles.some((v) => v.name === p.name) ? " · a separate copy will be added" : ""}`,
            "li",
          ),
        ),
      );
      view.body.append(list);
      let accepted = false;
      view.footer.append(
        button("Cancel", () => view.d.close()),
        button(
          "Import copies",
          () => {
            accepted = true;
            view.d.close();
          },
          "confirm-import",
        ),
      );
      view.d.addEventListener("close", () => resolve(accepted), { once: true });
      view.d.showModal();
    });
  }

  const triggerToolbar = node("div", { class: "studio-toolbar trigger-tools" }),
    histories = new Map();
  let restoring = false;
  const undo = button("Undo", () => travel(-1), "trigger-undo"),
    redo = button("Redo", () => travel(1), "trigger-redo");
  triggerToolbar.append(
    undo,
    redo,
    button("Copy L2 → R2", () => copy("left", "right"), "copy-left-right"),
    button("Copy R2 → L2", () => copy("right", "left"), "copy-right-left"),
    button(
      "Swap L2 / R2",
      () => {
        const p = profile();
        [p.triggers.left, p.triggers.right] = [
          p.triggers.right,
          p.triggers.left,
        ];
        app.saveProfiles();
        app.renderTriggerControls();
      },
      "swap-triggers",
    ),
    button("Compare preset", compare, "compare-trigger-preset"),
  );
  $("#trigger-library").closest(".trigger-library")?.append(triggerToolbar);
  if (!triggerToolbar.isConnected)
    $("#triggers .page-heading").after(triggerToolbar);
  const snapshot = () =>
    clone({
      triggers: profile().triggers,
      inputEffects: profile().inputEffects,
    });
  function baseline() {
    let h = histories.get(state.profileId);
    if (!h) {
      h = { current: snapshot(), past: [], future: [], at: 0, control: null };
      histories.set(state.profileId, h);
    }
    return h;
  }
  function capture() {
    if (restoring) return;
    const h = baseline(),
      next = snapshot();
    if (JSON.stringify(h.current) === JSON.stringify(next)) return;
    const now = performance.now(),
      control = document.activeElement;
    const grouped =
      h.control === control &&
      now - h.at < 650 &&
      control?.matches("input[type=range],input[type=number]");
    if (!grouped || !h.past.length) h.past.push(h.current);
    h.past = h.past.slice(-100);
    h.current = next;
    h.future = [];
    h.at = now;
    h.control = control;
    updateHistory();
    if (!native)
      try {
        browserBackup();
      } catch (e) {
        toast(`Backup failed: ${e}`);
      }
  }
  function updateHistory() {
    const h = baseline();
    undo.disabled = !h.past.length;
    redo.disabled = !h.future.length;
    undo.title = undo.disabled
      ? "No earlier trigger edit"
      : "Undo the last trigger edit";
    redo.title = redo.disabled
      ? "No edit to redo"
      : "Redo the last trigger edit";
  }
  function travel(direction) {
    const h = baseline(),
      from = direction < 0 ? h.past : h.future,
      to = direction < 0 ? h.future : h.past;
    if (!from.length) return;
    to.push(h.current);
    h.current = from.pop();
    Object.assign(profile(), clone(h.current));
    h.control = null;
    restoring = true;
    app.renderTriggerControls();
    app.saveProfiles();
    restoring = false;
    updateHistory();
  }
  function copy(from, to) {
    profile().triggers[to] = clone(profile().triggers[from]);
    app.saveProfiles();
    app.renderTriggerControls();
    toast(
      `Copied ${from === "left" ? "L2" : "R2"} to ${to === "left" ? "L2" : "R2"}. Press Apply to use it.`,
    );
  }
  function compare() {
    const presets = app.getPresets();
    if (!presets.length) {
      toast("Save a trigger preset first.");
      return;
    }
    const view = dialog("Compare trigger settings"),
      choice = select(
        presets.map((p) => [p.id, p.name]),
        $("#trigger-library").value || presets[0].id,
        draw,
      ),
      host = node("div");
    choice.setAttribute("aria-label", "Comparison preset");
    view.body.append(label("Saved preset", choice), host);
    view.footer.append(button("Close", () => view.d.close()));
    function draw() {
      const p = presets.find((p) => p.id === choice.value);
      host.replaceChildren();
      for (const side of ["left", "right"]) {
        const current = profile().triggers[side],
          saved = p.triggers[side],
          table = node("table", { class: "studio-table" });
        table.innerHTML =
          "<thead><tr><th>Setting</th><th>Current</th><th>Saved</th></tr></thead>";
        const body = node("tbody");
        for (const [label, a, b] of [
          ["Mode", current.mode, saved.mode],
          ["Strength", `${current.strength}%`, `${saved.strength}%`],
          [
            "Start zone",
            (current.startZone ?? 2) + 1,
            (saved.startZone ?? 2) + 1,
          ],
          ["End zone", (current.endZone ?? 8) + 1, (saved.endZone ?? 8) + 1],
          ["Force cap", current.forceCap ?? 6, saved.forceCap ?? 6],
          [
            "Zone forces",
            (current.zoneForces || [])
              .map((v) => Math.round(v * 100))
              .join(" / "),
            (saved.zoneForces || [])
              .map((v) => Math.round(v * 100))
              .join(" / "),
          ],
        ]) {
          const row = node("tr");
          [label, a, b].forEach((v) => row.append(text(String(v), "td")));
          body.append(row);
        }
        table.append(body);
        host.append(text(side === "left" ? "L2" : "R2", "h3"), table);
      }
    }
    draw();
    view.d.showModal();
  }
  const guide = node("details", { class: "studio-guide" });
  guide.append(node("summary", { text: "How trigger force and timing work" }));
  guide.insertAdjacentHTML(
    "beforeend",
    '<div class="trigger-stage-guide"><span>Rest</span><i>Load / tension</i><b>Wall / shot</b><em>Release</em></div><p>Start sets where resistance begins. End sets the wall or break. Strength scales force; Force cap selects one of eight motor levels. Bow unloads at its high point. Weapon recoil needs space for the trigger to move back.</p><p>Burst spacing separates shots. Kick duration controls each return stroke. Motor strike uses whole-Hz timing, so its actual spacing is rounded. Grip feedback is separate from trigger force.</p>',
  );
  triggerToolbar.after(guide);
  // Reset a single control to its saved profile value, without touching other settings.
  const resetControl = button(
    "Reset focused control",
    () => {
      const target = lastTriggerControl;
      if (!target || !target.isConnected) return;
      target.value = target.defaultValue || target.getAttribute("min") || "0";
      target.dispatchEvent(new Event("input", { bubbles: true }));
      target.dispatchEvent(new Event("change", { bubbles: true }));
      toast("Control reset to its initial value.");
    },
    "reset-trigger-control",
  );
  let lastTriggerControl = null;
  $("#triggers").addEventListener("focusin", (e) => {
    if (e.target.matches("input[type=range],input[type=number]")) {
      lastTriggerControl = e.target;
      resetControl.disabled = false;
      resetControl.title = `Reset ${e.target.getAttribute("aria-label") || e.target.id}`;
    }
  });
  resetControl.disabled = true;
  triggerToolbar.append(resetControl);

  const steamCard = card("Installed Steam games", "steam-games");
  $("#profiles").append(steamCard.box);
  const steamStatus = text(
      "Find games installed through Steam and choose the executable for this profile.",
    ),
    steamList = node("div", { class: "studio-list", id: "steam-games-list" }),
    steamSearch = node("input", {
      type: "search",
      placeholder: "Filter installed games",
    });
  steamSearch.setAttribute("aria-label", "Filter installed Steam games");
  let games = [];
  steamCard.body.append(
    node("div", { class: "studio-toolbar" }, [
      button(
        "Find installed games",
        async () => {
          steamStatus.textContent = "Reading Steam libraries…";
          try {
            games = native ? await invoke("installed_steam_games") : [];
            games = games || [];
            steamStatus.textContent = games.length
              ? `${games.length} games found. Assign an executable to the current profile.`
              : "No installed Steam games found.";
            renderSteam();
          } catch (e) {
            steamStatus.textContent = `Steam scan failed: ${e}`;
          }
        },
        "scan-steam-games",
      ),
      steamSearch,
    ]),
    steamStatus,
    steamList,
  );
  steamSearch.addEventListener("input", renderSteam);
  function renderSteam() {
    steamList.replaceChildren();
    for (const game of games.filter((g) =>
      g.name.toLowerCase().includes(steamSearch.value.toLowerCase()),
    )) {
      const row = node("div", { class: "steam-game-row" }),
        options = [
          ["", "Choose executable"],
          ...(game.executables || []).map((path) => [
            path,
            path.replaceAll("\\", "/").split("/").slice(-2).join("/"),
          ]),
        ],
        exe = select(options, "", () => {});
      exe.setAttribute("aria-label", `${game.name} executable`);
      row.append(
        text(game.name, "strong"),
        exe,
        button("Assign", async () => {
          if (!exe.value) {
            toast(
              "Choose the game executable. If none is listed, use “Use last app” after focusing the game.",
            );
            return;
          }
          const p = profile();
          p.name = [...game.name].slice(0, 40).join("");
          p.matchExe = exe.value
            .replaceAll("\\", "/")
            .split("/")
            .at(-1)
            .toLowerCase();
          p.matchPath = exe.value;
          p.matchTitle = "";
          p.tag = "STEAM";
          app.renderAllProfileControls();
          await app.saveProfiles(true);
          toast(`Assigned ${game.name}`);
        }),
      );
      steamList.append(row);
    }
  }

  const shortcuts = card("Controller profile shortcuts", "game-shortcuts");
  $("#profiles").append(shortcuts.box);
  const shortcutRows = node("div", { class: "studio-list" }),
    actions = [
      ["nextProfile", "Next profile"],
      ["previousProfile", "Previous profile"],
      ["nextWeapon", "Next weapon preset"],
      ["previousWeapon", "Previous weapon preset"],
    ];
  const shortcutButtons = app.names
    .map((name, i) => [i, name])
    .filter(([i]) => ![6, 7, 16, 18].includes(i));
  shortcuts.body.append(
    text(
      "Active in Game mode. Use a modifier to avoid taking over a normal game button. Reset stops these shortcuts until you apply a profile.",
    ),
    shortcutRows,
    button(
      "Add shortcut",
      () => {
        if (profile().gameShortcuts.length >= 12) {
          toast("12 shortcut limit.");
          return;
        }
        const used = new Set(
          profile().gameShortcuts.map((s) => `${s.button}:${s.modifier}`),
        );
        const available = shortcutButtons.find(
          ([i]) => i !== 8 && !used.has(`${i}:8`),
        );
        if (!available) return;
        profile().gameShortcuts.push({
          button: available[0],
          modifier: 8,
          action: "nextWeapon",
        });
        app.saveProfiles();
        renderShortcuts();
      },
      "add-game-shortcut",
    ),
  );
  function renderShortcuts() {
    shortcutRows.replaceChildren();
    profile().gameShortcuts.forEach((binding, index) => {
      const row = node("div", { class: "studio-shortcut-row" });
      const set = (key, value) => {
        const old = binding[key];
        binding[key] = value;
        if (
          binding.modifier === binding.button ||
          profile().gameShortcuts.some(
            (s, i) =>
              i !== index &&
              s.button === binding.button &&
              s.modifier === binding.modifier,
          )
        ) {
          binding[key] = old;
          toast("Choose a distinct button and modifier.");
          renderShortcuts();
          return;
        }
        app.saveProfiles();
      };
      row.append(
        label(
          "Button",
          select(shortcutButtons, binding.button, (v) => set("button", +v)),
        ),
        label(
          "While held",
          select(
            [["", "No modifier"], ...shortcutButtons],
            binding.modifier ?? "",
            (v) => set("modifier", v === "" ? null : +v),
          ),
        ),
        label(
          "Action",
          select(actions, binding.action, (v) => set("action", v)),
        ),
        button("Remove", () => {
          profile().gameShortcuts.splice(index, 1);
          app.saveProfiles();
          renderShortcuts();
        }),
      );
      shortcutRows.append(row);
    });
  }
  let previousButtons = [],
    shortcutBusy = false,
    weaponIndex = -1;
  async function handleShortcuts(buttons) {
    const previous = previousButtons;
    previousButtons = buttons.slice();
    if (
      state.pcMode ||
      !state.connected ||
      document.querySelector("dialog[open]") ||
      state.autoSuppressed ||
      shortcutBusy ||
      (buttons[16] > 0.5 && buttons[9] > 0.5)
    )
      return;
    const binding = profile().gameShortcuts.find(
      (s) =>
        buttons[s.button] > 0.5 &&
        !(previous[s.button] > 0.5) &&
        (s.modifier == null || buttons[s.modifier] > 0.5),
    );
    if (!binding) return;
    shortcutBusy = true;
    try {
      if (binding.action.endsWith("Profile")) {
        const at = state.profiles.findIndex((p) => p.id === state.profileId),
          next =
            (at +
              (binding.action === "nextProfile" ? 1 : -1) +
              state.profiles.length) %
            state.profiles.length;
        await app.selectProfile(state.profiles[next].id, true);
        toast(`Profile: ${profile().name}`);
      } else {
        const rows = profile().weaponBindings;
        if (!rows.length) {
          toast("Add weapon mappings to this profile first.");
          return;
        }
        weaponIndex =
          (weaponIndex +
            (binding.action === "nextWeapon" ? 1 : -1) +
            rows.length) %
          rows.length;
        $("#manual-weapon").value = rows[weaponIndex].weapon;
        await app.useWeapon(rows[weaponIndex].weapon);
      }
    } finally {
      shortcutBusy = false;
    }
  }

  function parseArgs(raw) {
    if (!raw.trim()) return [];
    let args;
    try {
      args = JSON.parse(raw);
    } catch {
      throw Error(
        'Use a JSON list, for example ["--fullscreen", "file name"].',
      );
    }
    if (
      !Array.isArray(args) ||
      args.length > 16 ||
      !args.every(
        (v) => typeof v === "string" && v.length <= 1024 && !v.includes("\0"),
      )
    )
      throw Error("Use up to 16 text arguments.");
    return args;
  }
  function renderPc() {
    // Core renders the row; this adds launch arguments.
    for (const binding of app
      .getPcBindings()
      .filter((b) => b.behavior === "launch")) {
      const selector = `[aria-label="${app.names[binding.button]} app path"]`,
        input = $(selector);
      if (!input) continue;
      const host = node("div", { class: "pc-launch-editor" });
      input.replaceWith(host);
      host.append(input);
      const args = node("input", {
        type: "text",
        maxLength: 17000,
        value: JSON.stringify(binding.launchArgs || []),
        placeholder: '["--option", "value"]',
      });
      args.setAttribute(
        "aria-label",
        `${app.names[binding.button]} app arguments`,
      );
      args.title = "Arguments are passed directly, without a shell.";
      args.addEventListener("change", () => {
        try {
          binding.launchArgs = parseArgs(args.value);
          app.savePcBindings();
        } catch (e) {
          toast(e.message);
        }
      });
      host.append(label("Arguments (JSON list)", args));
    }
  }
  $("#pc-reset-bindings").after(
    button("Stop macros", () => send({ action: "stopMacros" }), "stop-macros"),
  );
  function editMacro(binding) {
    const view = dialog(`${app.names[binding.button]} macro`),
      program = clone(
        binding.program || { steps: [{ type: "wait", ms: 100 }], repeat: 1 },
      ),
      rows = node("div", { class: "macro-steps" });
    view.body.append(
      text(
        "Runs in PC control while another window is focused. Focus this app, switch to Game mode, or use Stop macros to cancel.",
      ),
      number("Repeats", program.repeat, 1, 10, (v) => (program.repeat = v)),
      rows,
    );
    const defaults = {
      chord: () => ({
        type: "chord",
        keys: [platform.os === "macos" ? 91 : 17, 67],
        holdMs: 45,
      }),
      wait: () => ({ type: "wait", ms: 100 }),
      click: () => ({ type: "click", button: "left", holdMs: 45 }),
      scroll: () => ({ type: "scroll", amount: 1 }),
      launch: () => ({ type: "launch", path: "", args: [] }),
    };
    function draw() {
      rows.replaceChildren();
      program.steps.forEach((step, i) => {
        const row = node("div", { class: "macro-step" }),
          head = node("div", { class: "studio-toolbar" });
        head.append(
          text(`${i + 1}`, "strong"),
          select(
            [
              ["chord", "Key / shortcut"],
              ["wait", "Wait"],
              ["click", "Mouse click"],
              ["scroll", "Scroll"],
              ["launch", "Open app"],
            ],
            step.type,
            (v) => {
              program.steps[i] = defaults[v]();
              draw();
            },
          ),
          button("↑", () => {
            if (i) {
              [program.steps[i - 1], program.steps[i]] = [
                program.steps[i],
                program.steps[i - 1],
              ];
              draw();
            }
          }),
          button("↓", () => {
            if (i < program.steps.length - 1) {
              [program.steps[i + 1], program.steps[i]] = [
                program.steps[i],
                program.steps[i + 1],
              ];
              draw();
            }
          }),
          button("Remove", () => {
            program.steps.splice(i, 1);
            draw();
          }),
        );
        row.append(head);
        if (step.type === "chord") {
          const keys = node("input", {
            value: step.keys
              .map(
                (k) =>
                  app.vkChoices.find((v) => v.vk === k)?.label || `Key ${k}`,
              )
              .join(" + "),
            maxLength: 120,
          });
          keys.setAttribute("aria-label", `Step ${i + 1} keys`);
          keys.addEventListener("input", () => keys.setCustomValidity(""));
          keys.addEventListener("change", () => {
            const values = keys.value
              .split("+")
              .map((v) => v.trim())
              .map(
                (v) =>
                  app.vkChoices.find(
                    (k) => k.label.toLowerCase() === v.toLowerCase(),
                  )?.vk || Number(v.replace(/^Key\s+/i, "")),
              );
            if (
              !values.length ||
              values.length > 8 ||
              !values.every(app.validPcKey) ||
              new Set(values).size !== values.length
            ) {
              keys.setCustomValidity("Enter distinct supported key names.");
              toast(
                `Use distinct key names separated by +, such as ${platform.primaryLabel} + Shift + K.`,
              );
              return;
            }
            keys.setCustomValidity("");
            step.keys = values;
          });
          row.append(
            label("Keys, separated by +", keys),
            number(
              "Hold (ms)",
              step.holdMs,
              20,
              5000,
              (v) => (step.holdMs = v),
            ),
          );
        }
        if (step.type === "wait")
          row.append(
            number("Wait (ms)", step.ms, 10, 10000, (v) => (step.ms = v)),
          );
        if (step.type === "click")
          row.append(
            label(
              "Mouse button",
              select(
                [
                  ["left", "Left"],
                  ["right", "Right"],
                ],
                step.button,
                (v) => (step.button = v),
              ),
            ),
            number(
              "Hold (ms)",
              step.holdMs,
              20,
              5000,
              (v) => (step.holdMs = v),
            ),
          );
        if (step.type === "scroll")
          row.append(
            number(
              "Notches (positive = up)",
              step.amount,
              -20,
              20,
              (v) => (step.amount = v),
            ),
          );
        if (step.type === "launch") {
          const path = node("input", {
              value: step.path,
              maxLength: 1024,
              placeholder: platform.appPathPlaceholder,
            }),
            args = node("input", {
              value: JSON.stringify(step.args),
              maxLength: 17000,
            });
          path.addEventListener("input", () => (step.path = path.value));
          args.addEventListener("change", () => {
            try {
              step.args = parseArgs(args.value);
            } catch (e) {
              toast(e.message);
            }
          });
          row.append(
            label("Application path", path),
            label("Arguments (JSON list)", args),
          );
        }
        rows.append(row);
      });
    }
    view.body.append(
      button(
        "Add step",
        () => {
          if (program.steps.length >= 64) {
            toast("64 step limit.");
            return;
          }
          program.steps.push(defaults.wait());
          draw();
        },
        "add-macro-step",
      ),
    );
    view.footer.append(
      button("Cancel", () => view.d.close()),
      button(
        "Save macro",
        () => {
          if (!program.steps.length) {
            toast("Add at least one step.");
            return;
          }
          const invalid = [...view.d.querySelectorAll("input,select")].find(
            (input) => !input.checkValidity(),
          );
          if (invalid) {
            invalid.reportValidity();
            return;
          }
          if (!validProgram(program)) {
            toast("Check the steps. Macros must finish within 60 seconds.");
            return;
          }
          binding.program = program;
          app.savePcBindings();
          app.renderPcBindings();
          view.d.close();
        },
        "save-macro",
      ),
    );
    draw();
    view.d.showModal();
  }
  function validProgram(p) {
    if (
      !p ||
      !Array.isArray(p.steps) ||
      p.steps.length < 1 ||
      p.steps.length > 64 ||
      !Number.isInteger(p.repeat) ||
      p.repeat < 1 ||
      p.repeat > 10
    )
      return false;
    let total = 0;
    for (const s of p.steps) {
      if (
        s.type === "chord" &&
        Array.isArray(s.keys) &&
        s.keys.length &&
        s.keys.length <= 8 &&
        new Set(s.keys).size === s.keys.length &&
        s.keys.every(app.validPcKey) &&
        Number.isInteger(s.holdMs) &&
        s.holdMs >= 20 &&
        s.holdMs <= 5000
      )
        total += s.holdMs + 20;
      else if (
        s.type === "wait" &&
        Number.isInteger(s.ms) &&
        s.ms >= 10 &&
        s.ms <= 10000
      )
        total += s.ms;
      else if (
        s.type === "click" &&
        ["left", "right"].includes(s.button) &&
        Number.isInteger(s.holdMs) &&
        s.holdMs >= 20 &&
        s.holdMs <= 5000
      )
        total += s.holdMs + 20;
      else if (
        s.type === "scroll" &&
        Number.isInteger(s.amount) &&
        s.amount &&
        Math.abs(s.amount) <= 20
      )
        total += 20;
      else if (
        s.type === "launch" &&
        typeof s.path === "string" &&
        s.path.length <= 1024 &&
        (/^[A-Za-z]:\\/.test(s.path) || s.path.startsWith("/")) &&
        !s.path.includes("\0")
      ) {
        try {
          parseArgs(JSON.stringify(s.args));
        } catch {
          return false;
        }
        total += 200;
      } else return false;
    }
    return total * p.repeat <= 60000;
  }

  const diag = card("Integration monitor", "integration-monitor");
  $("#diagnostics").append(diag.box);
  const summary = node("dl", { class: "studio-status-grid" }),
    diagStatus = text("No game packets received."),
    checks = node("div", { class: "studio-list" }),
    packetLog = node("ol", { class: "studio-packet-log" });
  let lastPacket = "",
    lastError = "",
    received = 0,
    recording = false,
    recordStart = 0,
    recordBytes = 0,
    packets = [],
    replayTimer = null,
    replaying = false,
    logs = [],
    lastDiagRender = 0,
    lastAction = "No output applied this session",
    receiverMessage = "Off";
  const recordButton = button(
    "Record packets",
    () => {
      if (recording) {
        recording = false;
        send({ action: "setGameRecording", enabled: false });
        recordButton.textContent = "Record packets";
        diagStatus.textContent = `Recorded ${packets.length} packets.`;
        return;
      }
      stopReplay();
      packets = [];
      recordBytes = 0;
      recording = true;
      send({ action: "setGameRecording", enabled: true });
      recordStart = performance.now();
      recordButton.textContent = "Stop recording";
      diagStatus.textContent =
        "Recording local game packets · limit 2,000 packets / 10 minutes";
    },
    "record-game-packets",
  );
  const replayFile = node("input", {
    type: "file",
    accept: ".json,application/json",
    hidden: true,
  });
  replayFile.addEventListener("change", async () => {
    const file = replayFile.files[0];
    replayFile.value = "";
    if (!file) return;
    try {
      if (file.size > 5_000_000) throw Error("Recording exceeds 5 MB");
      const data = JSON.parse(await file.text());
      if (
        data.schemaVersion !== 1 ||
        !Array.isArray(data.packets) ||
        !data.packets.length ||
        data.packets.length > 2000
      )
        throw Error("Invalid recording");
      let previous = -1;
      for (const p of data.packets) {
        if (
          !Number.isFinite(p.atMs) ||
          p.atMs < previous ||
          p.atMs > 600000 ||
          !["json", "dsx"].includes(p.format) ||
          typeof p.packet !== "string" ||
          new TextEncoder().encode(p.packet).length > 8192
        )
          throw Error("Invalid packet or timestamp");
        JSON.parse(p.packet);
        previous = p.atMs;
      }
      if (!profile().gameLink.enabled) {
        toast("Enable the local game receiver on Profiles before replaying.");
        return;
      }
      recording = false;
      send({ action: "setGameRecording", enabled: false });
      recordButton.textContent = "Record packets";
      packets = data.packets;
      startReplay();
    } catch (e) {
      toast(`Replay failed: ${e.message}`);
    }
  });
  const monitorButtons = node("div", { class: "studio-toolbar" }, [
    button(
      "Test receiver",
      async () => {
        try {
          const settings = profile().gameLink;
          if (!settings.enabled) {
            toast("Enable the game receiver on Profiles first.");
            return;
          }
          await send({ action: "setGameLink", settings });
          const packet =
            settings.format === "dsx"
              ? JSON.stringify({ instructions: [{ type: 0, parameters: [0] }] })
              : JSON.stringify({ events: ["controller_receiver_test"] });
          await invoke("send_game_packet", {
            port: settings.port,
            format: settings.format,
            packet,
          });
          diagStatus.textContent =
            "Test packet sent. Waiting for the local receiver…";
        } catch (e) {
          toast(`Receiver test failed: ${e}`);
        }
      },
      "test-game-receiver",
    ),
    recordButton,
    button(
      "Export recording",
      () => {
        if (!packets.length) {
          toast("Record a packet first.");
          return;
        }
        download(
          { schemaVersion: 1, packets },
          "CONTROLLER-game-recording.json",
        );
      },
      "export-game-recording",
    ),
    button("Replay file", () => replayFile.click(), "replay-game-recording"),
    button("Stop replay", stopReplay, "stop-game-replay"),
    button("Check platform", platformChecks, "check-platform"),
  ]);
  diag.body.append(
    summary,
    monitorButtons,
    diagStatus,
    replayFile,
    packetLog,
    checks,
  );
  function stopReplay() {
    clearTimeout(replayTimer);
    if (replaying) diagStatus.textContent = "Replay stopped.";
    replaying = false;
  }
  function startReplay() {
    stopReplay();
    replaying = true;
    const started = performance.now(),
      port = profile().gameLink.port,
      format = profile().gameLink.format;
    let i = 0;
    async function next() {
      if (!replaying) return;
      const p = packets[i];
      if (!p) {
        replaying = false;
        diagStatus.textContent = "Replay complete.";
        return;
      }
      if (p.format !== format) {
        stopReplay();
        toast("Recording format must match the receiver format.");
        return;
      }
      try {
        await invoke("send_game_packet", { port, format, packet: p.packet });
      } catch (e) {
        stopReplay();
        toast(`Replay stopped: ${e}`);
        return;
      }
      if (!replaying) return;
      i++;
      diagStatus.textContent = `Replaying ${i} / ${packets.length}`;
      replayTimer = setTimeout(
        next,
        Math.max(0, (packets[i]?.atMs || 0) - (performance.now() - started)),
      );
    }
    replayTimer = setTimeout(next, packets[0].atMs);
  }
  async function platformChecks() {
    try {
      const result = native
        ? await invoke("platform_diagnostics")
        : {
            checks: [
              {
                name: "Browser preview",
                detail: "Open the desktop app to check device permissions.",
                ok: true,
              },
            ],
          };
      checks.replaceChildren();
      for (const check of result?.checks || []) {
        const row = node("div", {
          class: "studio-check" + (check.ok ? "" : " is-warn"),
        });
        row.append(text(check.name, "strong"), text(check.detail));
        checks.append(row);
      }
    } catch (e) {
      toast(`Platform check failed: ${e}`);
    }
  }
  function drawDiagnostics() {
    summary.replaceChildren();
    for (const [name, value] of [
      ["Selected profile", profile().name],
      ["Last output", lastAction],
      [
        "Mode",
        state.pcMode ? "PC control" : state.autoSuppressed ? "Stopped" : "Game",
      ],
      [
        "Match",
        state.autoMatchId
          ? "Foreground match"
          : state.foregroundExe || "Manual",
      ],
      ["Receiver", receiverMessage],
      ["Packets", received],
      ["Last packet", lastPacket || "None"],
      ["Last error", lastError || "None"],
    ]) {
      summary.append(text(name, "dt"), text(String(value), "dd"));
    }
    packetLog.replaceChildren(...logs.map((line) => text(line, "li")));
  }
  function event(e) {
    if (e.type === "input") handleShortcuts(e.input.buttons);
    if (e.type === "gameLinkStatus")
      receiverMessage = e.message || (e.enabled ? "Listening" : "Off");
    if (e.type === "gameAction") lastAction = `${profile().name} / ${e.name}`;
    if (e.type === "gameEvent") {
      received++;
      lastPacket = e.weapon || e.events?.join(", ") || "DSX instruction";
      logs.unshift(`${new Date().toLocaleTimeString()} · ${lastPacket}`);
      logs = logs.slice(0, 12);
      diagStatus.textContent = `Receiver working · ${received} packets`;
      if (recording && e.packet && e.format) {
        const atMs = Math.round(performance.now() - recordStart);
        if (
          packets.length >= 2000 ||
          atMs > 600000 ||
          recordBytes + new TextEncoder().encode(e.packet).length > 4000000
        ) {
          recording = false;
          send({ action: "setGameRecording", enabled: false });
          recordButton.textContent = "Record packets";
        } else {
          recordBytes += new TextEncoder().encode(e.packet).length;
          packets.push({ atMs, format: e.format, packet: e.packet });
        }
      }
    }
    if (["desktopInputError", "gameLinkError", "error"].includes(e.type)) {
      lastError = e.message;
    }
    if (
      e.type === "safetyReset" ||
      (e.type === "core" && ["stopped", "restarting"].includes(e.status)) ||
      (e.type === "device" && !e.connected && state.connected)
    ) {
      stopReplay();
      recording = false;
      recordButton.textContent = "Record packets";
      previousButtons = [];
      if (e.type === "safetyReset") {
        lastAction = "Stopped by Reset";
        receiverMessage = "Off";
      }
    }
    if (
      state.currentView === "diagnostics" &&
      performance.now() - lastDiagRender > 200
    ) {
      drawDiagnostics();
      lastDiagRender = performance.now();
    }
  }

  const scrollPositions = new Map();
  try {
    for (const [k, v] of Object.entries(
      JSON.parse(sessionStorage.getItem("controller.scrollPositions") || "{}"),
    ))
      if (Number.isFinite(v)) scrollPositions.set(k, v);
  } catch {}
  const paletteButton = button(
    platform.shortcutLabel,
    openPalette,
    "command-palette-button",
  );
  paletteButton.title = "Find a page, profile, or action";
  $(".mast-actions").append(paletteButton);
  function openPalette() {
    if (document.querySelector("dialog[open]") || state.shortcutRecording)
      return;
    const view = dialog("Quick switch"),
      search = node("input", {
        type: "search",
        placeholder: "Page, profile, or action",
        class: "palette-search",
      }),
      rows = node("div", { class: "studio-list" });
    search.setAttribute("aria-label", "Find a command");
    const commands = [
      ...$$(".nav-link").map((b) => ({
        name: b.textContent.trim().replace(/\s+/g, " "),
        run: () => app.navigate(b.dataset.view),
      })),
      ...state.profiles.map((p) => ({
        name: `Profile: ${p.name}`,
        run: () => app.selectProfile(p.id, false),
      })),
      { name: "Stop macros", run: () => send({ action: "stopMacros" }) },
      { name: "Reset all output", run: () => $("#quick-reset").click() },
    ];
    function draw() {
      rows.replaceChildren(
        ...commands
          .filter((c) =>
            c.name.toLowerCase().includes(search.value.toLowerCase()),
          )
          .map((c) =>
            button(c.name, () => {
              view.d.close();
              c.run();
            }),
          ),
      );
    }
    search.addEventListener("input", draw);
    search.addEventListener("keydown", (e) => {
      if (e.key === "ArrowDown") {
        e.preventDefault();
        $("button", rows)?.focus();
      }
      if (e.key === "Enter") $("button", rows)?.click();
    });
    view.body.append(search, rows);
    view.footer.append(button("Close", () => view.d.close()));
    draw();
    view.d.showModal();
    search.focus();
  }
  window.addEventListener("keydown", (e) => {
    if (state.shortcutRecording) return;
    if (platform.primaryModifier(e) && e.key.toLowerCase() === "k") {
      e.preventDefault();
      openPalette();
    } else if (
      platform.primaryModifier(e) &&
      state.currentView === "triggers" &&
      !e.target.matches("input,textarea,select") &&
      (platform.os === "macos" ? ["z"] : ["z", "y"]).includes(
        e.key.toLowerCase(),
      )
    ) {
      e.preventDefault();
      travel(e.key.toLowerCase() === "y" || e.shiftKey ? 1 : -1);
    }
  });
  const prefs = card("Display", "display-preferences");
  $("#settings").append(prefs.box);
  for (const [key, name, cls] of [
    ["reducedMotion", "Reduce decorative motion", "studio-reduced-motion"],
  ]) {
    let enabled = false;
    try {
      enabled = localStorage.getItem(`controller.${key}`) === "true";
    } catch {}
    document.body.classList.toggle(cls, enabled);
    prefs.body.append(
      checkbox(name, enabled, (v) => {
        document.body.classList.toggle(cls, v);
        try {
          localStorage.setItem(`controller.${key}`, String(v));
        } catch {}
      }),
    );
  }
  function renderProfile() {
    baseline();
    updateHistory();
    renderFavorite();
    renderShortcuts();
    drawDiagnostics();
    weaponIndex = -1;
  }
  window.studioHooks = {
    capture,
    renderProfile,
    renderPc,
    editMacro,
    previewImport,
    event,
    filterProfile: (p) =>
      (!onlyFavorites || p.favorite) &&
      (!query ||
        [p.name, p.description, p.tag, p.matchExe].some((v) =>
          String(v || "")
            .toLowerCase()
            .includes(query),
        )),
    beforeNavigate() {
      scrollPositions.set(state.currentView, window.scrollY);
      try {
        sessionStorage.setItem(
          "controller.scrollPositions",
          JSON.stringify(Object.fromEntries(scrollPositions)),
        );
      } catch {}
    },
    scrollTop: (view) => scrollPositions.get(view) || 0,
    navigate(view) {
      if (view === "diagnostics") drawDiagnostics();
    },
    command(c) {
      if (c.action === "applyTriggers")
        lastAction = `${profile().name} · L2 ${c.left.mode} / R2 ${c.right.mode}`;
      if (c.action === "reset") {
        lastAction = "Stopped by Reset";
        stopReplay();
      }
      if (c.action === "setGameActions" && c.enabled)
        lastAction = `${profile().name} · action rules armed`;
    },
    loaded() {
      histories.clear();
      renderProfile();
    },
    validProgram,
    platform() {
      paletteButton.textContent = platform.shortcutLabel;
      paletteButton.title = `${platform.primaryLabel} + K: find a page, profile, or action`;
    },
  };
  app.renderProfiles();
  renderProfile();
  app.renderPcBindings();
  window.studioHooks.platform();
})();
