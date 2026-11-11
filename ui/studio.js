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
    let current = value;
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
        input.value = current;
        toast(`Enter ${min}–${max}.`);
        return;
      }
      current = v;
      change(v);
    });
    return label(name, input);
  };
  function card(title, id, meta) {
    const box = node("section", { class: "studio-card framed", id });
    if (meta)
      window.controllerPlatform?.section?.(box, meta.id, meta.level, meta.title || title, meta.hint, meta);
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
  function dialog(title, options = {}) {
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
      if (e.target === d) requestClose();
    });
    const requestClose = async () => {
      if (
        options.isDirty?.() &&
        !(await platform.confirm(
          "You have unsaved changes in this dialog. Discard them?",
          { title: "Discard changes?", confirmLabel: "Discard" },
        ))
      )
        return;
      options.onDiscard?.();
      d.close();
    };
    d.addEventListener("cancel", (e) => {
      if (options.isDirty?.()) {
        e.preventDefault();
        requestClose();
      }
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
    onlyFavorites = false,
    savedQuery = "";
  try {
    savedQuery = (localStorage.getItem("controller.profileQuery") || "").slice(0, 120);
    query = savedQuery.trim().toLowerCase();
    onlyFavorites = localStorage.getItem("controller.profileFavorites") === "true";
  } catch {}
  const search = node("input", {
    id: "profile-search",
    type: "search",
    placeholder: "Search profiles",
    maxLength: 120,
    value: savedQuery,
  });
  search.setAttribute("aria-label", "Search profiles");
  search.addEventListener("input", () => {
    query = search.value.trim().toLowerCase();
    try {
      localStorage.setItem("controller.profileQuery", search.value);
    } catch {}
    app.renderProfiles();
  });
  const filter = node("div", { class: "studio-profile-filter" });
  filter.append(
    search,
    checkbox("Favorites", onlyFavorites, (v) => {
      onlyFavorites = v;
      try {
        localStorage.setItem("controller.profileFavorites", String(v));
      } catch {}
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
      const clipIds = new Set(
        (p.actions || []).map((a) => a.timeline?.id).filter(Boolean),
      );
      const clips = (window.hapticAuthoring?.archive() || []).filter((c) =>
        clipIds.has(c.id),
      );
      if (clips.length) data.tactileClips = clips;
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
  const backupCard = card("Backups", "profile-recovery", { id: "profiles.recovery", level: "advanced", title: "Backups", hint: "Back up and restore profiles", keywords: "backup restore recovery profiles archive history undo save" });
  $(".profile-library-column").append(backupCard.box);
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
  window.controllerPlatform?.section?.(triggerToolbar, "triggers.toolbar", "basic", "Edit helpers", "Undo, redo, copy, swap, compare", { open: true, keywords: "undo redo copy swap compare reset trigger edit" });
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
  const modeName = (m) =>
    ({
      off: "Off",
      resistance: "Resistance",
      weapon: "Weapon",
      vibration: "Vibration",
      bow: "Bow (experimental)",
    })[m] || String(m);
  const diffStyle = document.createElement("style");
  diffStyle.textContent =
    ".studio-diff td{background:rgba(231,180,107,.18);font-weight:600}.studio-diff td:first-child{border-left:3px solid var(--gold,#e7b46b)}.monitor-groups{display:flex;flex-wrap:wrap;gap:10px 18px}.monitor-group{display:flex;flex-wrap:wrap;gap:6px;align-items:center}.monitor-group>small{color:var(--parchment-dim,#d5e3df);text-transform:uppercase;letter-spacing:.06em;font-size:.68rem}.rec-indicator{font:.85rem var(--mono,monospace);color:var(--danger,#c45c3e)}.rec-indicator[hidden]{display:none}.packet-inspector pre{max-height:220px;overflow:auto;background:var(--stage-mid,#192e42);padding:8px;border-radius:6px}.packet-inspector textarea{width:100%;min-height:70px}.macro-total{font:.85rem var(--mono,monospace)}.macro-total.over{color:var(--danger,#c45c3e)}.gallery-grid{display:grid;grid-template-columns:repeat(auto-fit,minmax(200px,1fr));gap:10px}.gallery-card{border:1px solid var(--line,#a8bec1);border-radius:8px;padding:10px;display:grid;gap:6px}.gallery-tags span{display:inline-block;margin-right:4px;padding:0 6px;border-radius:8px;font-size:.7rem;background:var(--stage-lift,#254158);color:var(--cream,#f0f8f7)}.curve-editor svg{width:100%;max-width:340px;touch-action:none;background:var(--stage-mid,#192e42);border-radius:6px}.palette-active{outline:2px solid var(--teal,#54c6c5)}";
  document.head.append(diffStyle);
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
          ["Mode", modeName(current.mode), modeName(saved.mode)],
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
          const differs = String(a) !== String(b);
          const row = node("tr", { class: differs ? "studio-diff" : "" });
          [differs ? `${label} (differs)` : label, a, b].forEach((v) =>
            row.append(text(String(v), "td")),
          );
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
  window.controllerPlatform?.section?.(guide, "triggers.guide", "advanced", "How trigger force and timing work", "Short explainer", { keywords: "guide help trigger force timing explain rest wall release" });
  guide.append(node("summary", { text: "How trigger force and timing work" }));
  guide.insertAdjacentHTML(
    "beforeend",
    '<div class="trigger-stage-guide"><span>Rest</span><i>Load / tension</i><b>Wall / shot</b><em>Release</em></div><p>Start sets where resistance begins. End sets the wall or break. Strength scales force; Force cap selects one of eight motor levels. Bow unloads at its high point. Weapon recoil needs space for the trigger to move back.</p><p>Burst spacing separates shots. Kick duration controls each return stroke. Motor strike uses whole-Hz timing, so its actual spacing is rounded. Grip feedback is separate from trigger force.</p>',
  );
  triggerToolbar.after(guide);
  // Effect gallery + drag-to-draw force curve editor (current trigger settings model only).
  const ramp = (a) => a.map((v) => Math.max(0, Math.min(1, v)));
  const gallery = [
    { name: "Bow draw", tags: ["bow", "tension"], t: { mode: "bow", strength: 90, forceCap: 8, curve: ramp([0,.02,.06,.13,.22,.34,.48,.65,.8,.92,1]), startZone: 0, endZone: 9, snapStrength: 75, zoneForces: [.12,.18,.25,.34,.44,.56,.69,.83,.94,.94] } },
    { name: "Gun recoil", tags: ["weapon", "shot"], t: { mode: "weapon", strength: 70, curve: ramp([0,.02,.05,.08,.14,.22,.92,.19,.2,.2,.2]), startZone: 4, endZone: 6, snapStrength: 55 } },
    { name: "Brake ABS pulse", tags: ["racing", "pulse"], t: { mode: "vibration", strength: 60, curve: ramp([.04,.21,.67,.16,.74,.22,.77,.27,.78,.31,.8]) } },
    { name: "Sticky lever", tags: ["resistance", "mechanical"], t: { mode: "resistance", strength: 65, curve: ramp([0,.3,.55,.7,.4,.35,.5,.75,.9,1,1]) } },
    { name: "Heartbeat", tags: ["vibration", "rhythm"], t: { mode: "vibration", strength: 45, curve: ramp([.04,.5,.15,.1,.6,.2,.05,.04,.04,.04,.04]) } },
  ];
  const galleryCard = card("Trigger effect gallery", "trigger-gallery", { id: "triggers.gallery", level: "tool", title: "Effect gallery", hint: "Ready-made trigger effects to try", keywords: "effect gallery presets trigger heartbeat brake abs lever vibration try" });
  const galleryStatus = text("Pick an effect, use it on a trigger, or try it on the controller first.");
  galleryStatus.setAttribute("role", "status");
  const galleryGrid = node("div", { class: "gallery-grid" });
  const sideName = (s) => (s === "left" ? "L2" : "R2");
  for (const g of gallery) {
    const c = node("div", { class: "gallery-card" });
    c.append(
      text(g.name, "strong"),
      node("div", { class: "gallery-tags" }, g.tags.map((t) => text(t, "span"))),
      node("div", { class: "studio-toolbar" }, ["left", "right"].map((side) =>
        button(`Use on ${sideName(side)}`, () => {
          // Drop per-effect fields from the previous mode so they do not leak into this one.
          const previous = clone(profile().triggers[side]);
          for (const stale of ["releasePoint", "preloadStrength", "frequencyHz"]) delete previous[stale];
          profile().triggers[side] = { ...previous, ...clone(g.t) };
          app.saveProfiles();
          app.renderTriggerControls();
          drawCurveEditor();
          galleryStatus.textContent = `${g.name} set on ${sideName(side)}. Press Apply to use it.`;
        }),
      )),
      button("Try on controller", async () => {
        if (!state.connected) {
          toast("Connect a controller to try an effect.");
          return;
        }
        const side = curveSide;
        const t = profile().triggers;
        const ok = await send({ action: "applyTriggers", left: side === "left" ? clone(g.t) : t.left, right: side === "right" ? clone(g.t) : t.right });
        galleryStatus.textContent = ok
          ? `${g.name} is active on ${sideName(side)} (temporary). Use "Re-apply my profile triggers" to go back.`
          : "The controller did not accept that effect. Check Diagnostics.";
      }),
    );
    galleryGrid.append(c);
  }
  const curveHost = node("div", { class: "curve-editor" });
  let curveSide = "right";
  const curveSideSel = select([["left", "L2"], ["right", "R2"]], curveSide, (v) => { curveSide = v; drawCurveEditor(); });
  curveSideSel.setAttribute("aria-label", "Trigger to edit");
  function drawCurveEditor() {
    const NS = "http://www.w3.org/2000/svg", svg = document.createElementNS(NS, "svg");
    svg.setAttribute("viewBox", "0 0 220 120");
    svg.setAttribute("role", "img");
    svg.setAttribute("aria-label", `${sideName(curveSide)} force curve, drag points to draw`);
    const curve = profile().triggers[curveSide].curve;
    const pt = (i) => [10 + (i / 10) * 200, 110 - curve[i] * 100];
    const line = document.createElementNS(NS, "polyline");
    line.setAttribute("fill", "none");
    line.style.stroke = "var(--teal)";
    line.setAttribute("stroke-width", "2");
    const dots = curve.map(() => document.createElementNS(NS, "circle"));
    const refresh = () => {
      line.setAttribute("points", curve.map((_, i) => pt(i).join(",")).join(" "));
      dots.forEach((d, i) => { const [x, y] = pt(i); d.setAttribute("cx", x); d.setAttribute("cy", y); d.setAttribute("aria-valuenow", String(Math.round(curve[i] * 100))); });
    };
    dots.forEach((d, i) => {
      d.setAttribute("r", 5); d.style.fill = "var(--gold)";
      d.setAttribute("tabindex", "0"); d.setAttribute("role", "slider");
      d.setAttribute("aria-orientation", "vertical"); d.setAttribute("aria-valuemin", "0"); d.setAttribute("aria-valuemax", "100");
      d.setAttribute("aria-label", `Curve point ${i + 1} of ${curve.length}`);
      d.addEventListener("keydown", (e) => {
        const step = e.shiftKey ? 0.1 : 0.02;
        const delta = e.key === "ArrowUp" || e.key === "ArrowRight" ? step : e.key === "ArrowDown" || e.key === "ArrowLeft" ? -step : 0;
        if (!delta && e.key !== "Home" && e.key !== "End") return;
        e.preventDefault(); e.stopPropagation();
        curve[i] = e.key === "Home" ? 0 : e.key === "End" ? 1 : Math.round(Math.max(0, Math.min(1, curve[i] + delta)) * 100) / 100;
        refresh(); app.saveProfiles();
      });
      d.addEventListener("blur", () => { app.renderTriggerControls(); });
    });
    svg.append(line, ...dots);
    let drag = -1;
    // Map through the SVG's own transform so letterboxing never skews the value.
    const local = (e) => {
      const m = svg.getScreenCTM();
      if (!m) { const r = svg.getBoundingClientRect(); return { x: ((e.clientX - r.left) / r.width) * 220, y: ((e.clientY - r.top) / r.height) * 120 }; }
      const p = new DOMPoint(e.clientX, e.clientY).matrixTransform(m.inverse());
      return { x: p.x, y: p.y };
    };
    const setFrom = (e) => {
      const y = local(e).y;
      curve[drag] = Math.round(Math.max(0, Math.min(1, (110 - y) / 100)) * 100) / 100;
      refresh();
    };
    svg.addEventListener("pointerdown", (e) => {
      const x = local(e).x;
      drag = Math.max(0, Math.min(10, Math.round(((x - 10) / 200) * 10)));
      svg.setPointerCapture(e.pointerId);
      setFrom(e);
    });
    svg.addEventListener("pointermove", (e) => { if (drag >= 0) setFrom(e); });
    const end = () => {
      if (drag < 0) return;
      drag = -1;
      app.saveProfiles();
      app.renderTriggerControls();
    };
    svg.addEventListener("pointerup", end);
    svg.addEventListener("pointercancel", end);
    refresh();
    curveHost.replaceChildren(svg);
  }
  galleryCard.body.append(
    galleryGrid,
    galleryStatus,
    node("div", { class: "studio-toolbar" }, [
      button("Re-apply my profile triggers", async () => {
        const t = profile().triggers;
        galleryStatus.textContent = (await send({ action: "applyTriggers", left: t.left, right: t.right })) ? "Your profile triggers are active again." : "Could not restore. Press Apply triggers.";
      }),
    ]),
    text("Draw a force curve: drag the points, then Apply."),
    label("Trigger to edit", curveSideSel),
    curveHost,
    button("Mirror curve to the other trigger", () => {
      const other = curveSide === "left" ? "right" : "left";
      profile().triggers[other].curve = clone(profile().triggers[curveSide].curve);
      app.saveProfiles();
      app.renderTriggerControls();
      toast(`${sideName(curveSide)} curve copied to ${sideName(other)}.`);
    }, "mirror-curve"),
  );
  // After the trigger controls so the gamepad focus order reaches the side tabs first.
  document.getElementById('triggers').append(galleryCard.box);
  // Reset a single control to its saved profile value, without touching other settings.
  const resetControl = button(
    "Reset to profile",
    () => {
      let target = lastTriggerControl;
      // Trigger controls are redrawn on side and profile changes; find the same control again.
      if (target && !target.isConnected && target.id) target = document.getElementById(target.id);
      if (!target || !target.isConnected) {
        toast("That control was redrawn. Click the control you want to reset, then press Reset to profile.", { error: false });
        return;
      }
      const key = controlKey(target),
        wanted = loadedValues.get(key),
        name = target.getAttribute("aria-label") || target.id || "Control";
      if (wanted === undefined || wanted === target.value) {
        toast(`${name} already matches the value saved in this profile.`);
        return;
      }
      target.value = wanted;
      target.dispatchEvent(new Event("input", { bubbles: true }));
      target.dispatchEvent(new Event("change", { bubbles: true }));
      toast(`${name} reset to ${wanted}, the value saved when this profile was opened.`);
    },
    "reset-trigger-control",
  );
  resetControl.setAttribute("aria-label", "Reset the last edited trigger range to the saved profile value");
  let lastTriggerControl = null;
  const loadedValues = new Map();
  const controlKey = (el) => `${state.profileId}|${state.side}|${el.id || el.getAttribute("aria-label")}`;
  // Seed the baseline when a profile opens, so edits made before the first focus still reset correctly.
  const seedBaseline = () => { for (const el of document.querySelectorAll("#triggers input[type=range], #triggers input[type=number]")) if (el.id || el.getAttribute("aria-label")) loadedValues.set(controlKey(el), el.value); };
  $("#triggers").addEventListener("focusin", (e) => {
    if (e.target.matches("input[type=range],input[type=number]")) {
      lastTriggerControl = e.target;
      const key = controlKey(e.target);
      if (!loadedValues.has(key)) loadedValues.set(key, e.target.value);
      resetControl.disabled = false;
      resetControl.title = `Reset ${e.target.getAttribute("aria-label") || e.target.id} to the value saved in this profile`;
    }
  });
  resetControl.disabled = true;
  triggerToolbar.append(resetControl);

  const steamCard = card("Installed Steam games", "steam-games", { id: "profiles.steam", level: "advanced", title: "Installed Steam games", hint: "Pick a game's program file", keywords: "steam games installed scan library executable exe find" });
  $(".profile-library-column").append(steamCard.box);
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
  try {
    const saved = JSON.parse(localStorage.getItem("controller.steamGames") || "[]");
    if (Array.isArray(saved))
      games = saved.filter(
        (g) => g && typeof g.name === "string" && Array.isArray(g.executables),
      );
    steamSearch.value = (localStorage.getItem("controller.steamFilter") || "").slice(0, 80);
  } catch {}
  steamCard.body.append(
    node("div", { class: "studio-toolbar" }, [
      button(
        "Find installed games",
        async () => {
          steamStatus.textContent = "Reading Steam libraries…";
          if (!native) {
            steamStatus.textContent =
              "Scanning Steam is available in the desktop app only.";
            return;
          }
          try {
            games = (await invoke("installed_steam_games")) || [];
            try {
              localStorage.setItem("controller.steamGames", JSON.stringify(games.slice(0, 300)));
            } catch {}
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
  steamSearch.addEventListener("input", () => {
    try {
      localStorage.setItem("controller.steamFilter", steamSearch.value);
    } catch {}
    renderSteam();
  });
  if (games.length) {
    steamStatus.textContent = `${games.length} games from your last scan. Find installed games to refresh.`;
    queueMicrotask(() => renderSteam());
  }
  function applySteamGame(p, game, exe) {
    p.name = [...game.name].slice(0, 40).join("");
    p.matchExe = exe.replaceAll("\\", "/").split("/").at(-1).toLowerCase();
    p.matchPath = exe;
    p.matchTitle = "";
    p.tag = "STEAM";
  }
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
          if (p.matchExe && p.matchPath !== exe.value) {
            // This profile already belongs to another game: ask before overwriting it.
            const view = dialog("Assign this game?");
            view.body.append(
              text(
                `The current profile "${p.name}" is already set up for ${p.matchExe}. Create a separate profile for ${game.name}, or overwrite this one?`,
              ),
            );
            view.footer.append(
              button("Cancel", () => view.d.close()),
              button("Overwrite this profile", async () => {
                view.d.close();
                applySteamGame(p, game, exe.value);
                app.renderAllProfileControls();
                if (await app.saveProfiles(true)) toast(`Assigned ${game.name} to ${p.name}`);
                else toast("Could not save the assignment. Check the profile for errors.");
              }),
              button(
                "Create new profile",
                async () => {
                  view.d.close();
                  const copyOf = clone(p);
                  copyOf.id = crypto.randomUUID();
                  copyOf.favorite = false;
                  applySteamGame(copyOf, game, exe.value);
                  state.profiles.push(copyOf);
                  await app.selectProfile(copyOf.id, false);
                  if (await app.saveProfiles(true)) toast(`Created a profile for ${game.name}`);
                  else {
                    toast("Could not save the new profile.");
                  }
                },
                "steam-create-profile",
              ),
            );
            view.d.showModal();
            return;
          }
          applySteamGame(p, game, exe.value);
          app.renderAllProfileControls();
          if (await app.saveProfiles(true)) toast(`Assigned ${game.name}`);
          else toast("The assignment could not be saved. Check the profile for errors.");
        }),
      );
      steamList.append(row);
    }
  }

  const shortcuts = card("Profile shortcuts", "game-shortcuts", { id: "profiles.shortcuts", level: "advanced", title: "Profile shortcuts", hint: "Switch profile with a button combination", keywords: "shortcuts combo button switch profile weapon next previous" });
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
        if (!available) {
          toast("Every button already has a CREATE-held shortcut. Change or remove one first.");
          return;
        }
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
        const forward = binding.action === "nextWeapon";
        if (weaponIndex < 0 || weaponIndex >= rows.length || rows[weaponIndex].weapon !== $("#manual-weapon").value)
          weaponIndex = rows.findIndex((r) => r.weapon === $("#manual-weapon").value);
        weaponIndex =
          weaponIndex < 0
            ? forward
              ? 0
              : rows.length - 1
            : (weaponIndex + (forward ? 1 : -1) + rows.length) % rows.length;
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
  function macroMs(p) {
    let total = 0;
    for (const s of p.steps || []) {
      if (s.type === "chord" || s.type === "click") total += (Number(s.holdMs) || 0) + 20;
      else if (s.type === "wait") total += Number(s.ms) || 0;
      else if (s.type === "scroll") total += 20;
      else if (s.type === "launch") total += 200;
    }
    return total * (Number(p.repeat) || 1);
  }
  function explainProgram(p) {
    if (!p || !Array.isArray(p.steps) || !p.steps.length) return "Add at least one step.";
    if (p.steps.length > 64) return "A macro can have at most 64 steps.";
    if (!Number.isInteger(p.repeat) || p.repeat < 1 || p.repeat > 10) return "Repeats must be a whole number from 1 to 10.";
    for (const [i, s] of p.steps.entries()) {
      const where = `Step ${i + 1}`;
      if (s.type === "chord") {
        if (!Array.isArray(s.keys) || !s.keys.length) return `${where}: choose at least one key.`;
        if (s.keys.length > 8 || new Set(s.keys).size !== s.keys.length) return `${where}: use up to 8 distinct keys.`;
        if (!s.keys.every(app.validPcKey)) return `${where}: one of the keys is not supported on this system.`;
        if (!Number.isInteger(s.holdMs) || s.holdMs < 20 || s.holdMs > 5000) return `${where}: hold time must be 20-5000 ms.`;
      } else if (s.type === "wait") {
        if (!Number.isInteger(s.ms) || s.ms < 10 || s.ms > 10000) return `${where}: wait must be 10-10000 ms.`;
      } else if (s.type === "click") {
        if (!Number.isInteger(s.holdMs) || s.holdMs < 20 || s.holdMs > 5000) return `${where}: click hold time must be 20-5000 ms.`;
      } else if (s.type === "scroll") {
        if (!Number.isInteger(s.amount) || !s.amount || Math.abs(s.amount) > 20) return `${where}: scroll notches must be a non-zero whole number up to 20.`;
      } else if (s.type === "launch") {
        if (typeof s.path !== "string" || !(/^[A-Za-z]:\\/.test(s.path) || s.path.startsWith("/"))) return `${where}: enter the full path of the app to open (for example ${platform.appPathPlaceholder}).`;
        if (s.path.length > 1024) return `${where}: the app path is too long.`;
        try { parseArgs(JSON.stringify(s.args)); } catch (e) { return `${where}: ${e.message}`; }
      } else return `${where}: unknown step type.`;
    }
    const total = macroMs(p);
    if (total > 60000) return `The macro takes about ${(total / 1000).toFixed(1)} s with repeats; the limit is 60 s. Shorten waits or lower Repeats.`;
    return "Check the steps.";
  }
  function editMacro(binding) {
    const draftKey = `controller.macroDraft.${binding.button}`;
    let program = clone(
      binding.program || { steps: [{ type: "wait", ms: 100 }], repeat: 1 },
    );
    const initial = JSON.stringify(program);
    let restored = false;
    try {
      const saved = JSON.parse(localStorage.getItem(draftKey) || "null");
      if (saved && Array.isArray(saved.steps) && saved.steps.length <= 64 && JSON.stringify(saved) !== initial) {
        program = saved;
        restored = true;
      }
    } catch {}
    const clearDraft = () => {
      try { localStorage.removeItem(draftKey); } catch {}
    };
    const view = dialog(`${app.names[binding.button]} macro`, {
        isDirty: () => JSON.stringify(program) !== initial,
        onDiscard: clearDraft,
      }),
      rows = node("div", { class: "macro-steps" }),
      totalLine = node("p", { class: "macro-total", id: "macro-total", role: "status" });
    const updateTotal = () => {
      const ms = macroMs(program);
      totalLine.textContent = `Total time: ${(ms / 1000).toFixed(2)} s of 60 s allowed (${program.steps.length} step${program.steps.length === 1 ? "" : "s"}, repeats x${program.repeat}).`;
      totalLine.classList.toggle("over", ms > 60000);
      try {
        if (JSON.stringify(program) !== initial) localStorage.setItem(draftKey, JSON.stringify(program));
        else clearDraft();
      } catch {}
    };
    view.body.addEventListener("change", () => queueMicrotask(updateTotal));
    view.body.addEventListener("input", () => queueMicrotask(updateTotal));
    view.body.append(
      text(
        "Runs in PC control while another window is focused. Focus this app, switch to Game mode, or use Stop macros to cancel.",
      ),
      ...(restored ? [text("Restored your unsaved draft from last time. Cancel to discard it.")] : []),
      number("Repeats", program.repeat, 1, 10, (v) => (program.repeat = v)),
      totalLine,
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
          ((b) => (b.setAttribute("aria-label", `Move step ${i + 1} up`), b))(
            button("↑", () => {
              if (i) {
                [program.steps[i - 1], program.steps[i]] = [
                  program.steps[i],
                  program.steps[i - 1],
                ];
                draw();
              }
            }),
          ),
          ((b) => (b.setAttribute("aria-label", `Move step ${i + 1} down`), b))(
            button("↓", () => {
              if (i < program.steps.length - 1) {
                [program.steps[i + 1], program.steps[i]] = [
                  program.steps[i],
                  program.steps[i + 1],
                ];
                draw();
              }
            }),
          ),
          ((b) => (b.setAttribute("aria-label", `Remove step ${i + 1}`), b))(
            button("Remove", () => {
              program.steps.splice(i, 1);
              draw();
            }),
          ),
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
      updateTotal();
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
      button("Cancel", () => {
        clearDraft();
        view.d.close();
      }),
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
            toast(explainProgram(program));
            return;
          }
          clearDraft();
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

  const diag = card("Integration monitor", "integration-monitor", { id: "diag.integration", level: "tool", title: "Integration monitor", hint: "Game receiver, packets, replay", keywords: "integration monitor game receiver packets replay record forza json udp platform check" });
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
    replayGeneration = 0,
    replaying = false,
    logs = [],
    lastDiagRender = 0,
    diagTimer = 0,
    packetHistory = [],
    selectedPacket = null,
    packetFilter = "",
    limitNotified = false,
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
        updateRecIndicator();
        return;
      }
      stopReplay();
      packets = [];
      recordBytes = 0;
      limitNotified = false;
      recording = true;
      send({ action: "setGameRecording", enabled: true });
      recordStart = performance.now();
      recordButton.textContent = "Stop recording";
      diagStatus.textContent =
        "Recording local game packets · limit 2,000 packets / 10 minutes";
      startRecTicker();
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
    stopReplay();
    const generation = replayGeneration;
    try {
      if (file.size > 5_000_000) throw Error("Recording exceeds 5 MB");
      const data = JSON.parse(await file.text());
      if (generation !== replayGeneration) return;
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
      if (!native) {
        toast("Replaying packets is available in the desktop app only.");
        return;
      }
      if (!profile().gameLink.enabled) {
        toast("Enable the local game receiver on Profiles before replaying.");
        return;
      }
      recording = false;
      send({ action: "setGameRecording", enabled: false });
      recordButton.textContent = "Record packets";
      updateRecIndicator();
      diagStatus.textContent = "Starting the local receiver for replay...";
      const ready = await send({ action: "setGameLink", settings: profile().gameLink });
      if (generation !== replayGeneration) return;
      if (!ready) {
        diagStatus.textContent = "The receiver could not be started, so nothing was replayed.";
        toast("Replay not started: the local receiver did not start. Check Diagnostics.");
        return;
      }
      packets = data.packets;
      startReplay();
    } catch (e) {
      if (generation !== replayGeneration) return;
      toast(`Replay failed: ${e.message}`);
    }
  });
  const recIndicator = node("span", { class: "rec-indicator", id: "recording-indicator", role: "status", hidden: true });
  let recTicker = 0;
  function updateRecIndicator() {
    recIndicator.hidden = !recording;
    if (recording) {
      const s = Math.floor((performance.now() - recordStart) / 1000);
      recIndicator.textContent = `● Recording ${String(Math.floor(s / 60)).padStart(2, "0")}:${String(s % 60).padStart(2, "0")} · ${packets.length} packet${packets.length === 1 ? "" : "s"}`;
    } else clearInterval(recTicker);
  }
  function startRecTicker() {
    clearInterval(recTicker);
    updateRecIndicator();
    recTicker = setInterval(updateRecIndicator, 1000);
  }
  const group = (name, items) => {
    const g = node("div", { class: "monitor-group" }, [node("small", { text: name }), ...items]);
    g.setAttribute("role", "group");
    g.setAttribute("aria-label", name);
    return g;
  };
  const monitorButtons = node("div", { class: "monitor-groups" }, [
    group("Receiver", [
    button(
      "Test receiver",
      async () => {
        try {
          if (typeof invoke !== "function") {
            toast("Sending a test packet is available in the desktop app only.");
            return;
          }
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
    ]),
    group("Record / export", [
    recordButton,
    recIndicator,
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
    ]),
    group("Replay", [
    button("Replay file", () => replayFile.click(), "replay-game-recording"),
    button("Stop replay", stopReplay, "stop-game-replay"),
    ]),
    group("Platform", [
    button("Check platform", platformChecks, "check-platform"),
    ]),
  ]);
  // Packet inspector: recent packets, filter, pretty JSON, and a custom packet sender.
  const inspector = node("details", { class: "packet-inspector", id: "packet-inspector" });
  window.controllerPlatform?.section?.(inspector, "diag.packet-inspector", "tool", "Packet inspector", "Inspect a game packet", { keywords: "packet inspector game json udp datagram format" });
  const inspectFilter = node("input", { type: "search", placeholder: "Filter recent packets", maxLength: 80 });
  inspectFilter.setAttribute("aria-label", "Filter recent packets");
  const inspectList = node("div", { class: "studio-list" }),
    inspectView = node("pre", { id: "packet-view", text: "Select a packet to see it formatted." }),
    custom = node("textarea", { id: "custom-packet", maxLength: 8192, placeholder: '{"events": ["my_event"]}' });
  custom.setAttribute("aria-label", "Custom packet JSON");
  inspectFilter.addEventListener("input", () => {
    packetFilter = inspectFilter.value.trim().toLowerCase();
    drawInspector();
  });
  function drawInspector() {
    inspectList.replaceChildren();
    const shown = packetHistory.filter((p) => !packetFilter || p.packet.toLowerCase().includes(packetFilter) || p.label.toLowerCase().includes(packetFilter));
    for (const p of shown.slice(0, 20)) {
      const b = button(`${p.time} · ${p.label}`, () => {
        selectedPacket = p;
        try {
          inspectView.textContent = JSON.stringify(JSON.parse(p.packet), null, 2);
        } catch {
          inspectView.textContent = p.packet;
        }
      });
      inspectList.append(b);
    }
    if (!shown.length) inspectList.append(text(packetHistory.length ? "No packets match the filter." : "Packets will appear here while the receiver is running."));
  }
  inspector.append(
    node("summary", { text: "Packet inspector" }),
    inspectFilter,
    inspectList,
    inspectView,
    label("Custom packet to send to the local receiver", custom),
    button("Send custom packet", async () => {
      try {
        if (typeof invoke !== "function") {
          toast("Sending packets is available in the desktop app only.");
          return;
        }
        const settings = profile().gameLink;
        if (!settings.enabled) {
          toast("Enable the game receiver on Profiles first.");
          return;
        }
        const raw = custom.value.trim();
        try {
          JSON.parse(raw);
        } catch (e) {
          toast(`That is not valid JSON: ${e.message}`);
          return;
        }
        if (new TextEncoder().encode(raw).length > 8192) {
          toast("Packets are limited to 8 KB.");
          return;
        }
        await send({ action: "setGameLink", settings });
        await invoke("send_game_packet", { port: settings.port, format: settings.format, packet: raw });
        diagStatus.textContent = "Custom packet sent.";
      } catch (e) {
        toast(`Could not send the packet: ${e}`);
      }
    }, "send-custom-packet"),
  );
  diag.body.append(
    summary,
    monitorButtons,
    diagStatus,
    replayFile,
    packetLog,
    inspector,
    checks,
  );
  function stopReplay() {
    replayGeneration++;
    clearTimeout(replayTimer);
    if (replaying) diagStatus.textContent = "Replay stopped.";
    replaying = false;
  }
  function startReplay() {
    stopReplay();
    const generation = replayGeneration;
    replaying = true;
    const started = performance.now(),
      port = profile().gameLink.port,
      format = profile().gameLink.format;
    let i = 0;
    async function next() {
      if (!replaying || generation !== replayGeneration) return;
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
        if (typeof invoke !== "function") throw Error("replay is available in the desktop app only");
        await invoke("send_game_packet", { port, format, packet: p.packet });
      } catch (e) {
        if (generation !== replayGeneration) return;
        stopReplay();
        diagStatus.textContent = `Replay stopped after ${i} of ${packets.length} packets: ${e}`;
        toast(`Replay stopped: ${e}`);
        return;
      }
      if (!replaying || generation !== replayGeneration) return;
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
        row.dataset.check = check.name;
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
    updateRecIndicator();
    drawInspector();
  }
  function scheduleDiagnostics() {
    if (state.currentView !== "diagnostics") return;
    const wait = 200 - (performance.now() - lastDiagRender);
    if (wait <= 0) {
      clearTimeout(diagTimer);
      diagTimer = 0;
      drawDiagnostics();
      lastDiagRender = performance.now();
    } else if (!diagTimer) {
      diagTimer = setTimeout(() => {
        diagTimer = 0;
        if (state.currentView === "diagnostics") {
          drawDiagnostics();
          lastDiagRender = performance.now();
        }
      }, wait);
    }
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
      if (e.packet) {
        packetHistory.unshift({ time: new Date().toLocaleTimeString(), label: lastPacket, packet: String(e.packet), format: e.format });
        packetHistory = packetHistory.slice(0, 50);
      }
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
          updateRecIndicator();
          const why = packets.length >= 2000 ? "2,000 packets" : atMs > 600000 ? "10 minutes" : "the size limit";
          diagStatus.textContent = `Recording stopped automatically at ${why}. ${packets.length} packets were kept; export them now.`;
          toast(`Recording stopped at ${why}. Your ${packets.length} packets were kept.`);
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
    scheduleDiagnostics();
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
  function fuzzyScore(name, q) {
    if (!q) return 1;
    const n = name.toLowerCase();
    const at = n.indexOf(q);
    if (at >= 0) return 1000 - at + (at === 0 ? 200 : 0);
    let i = 0, score = 0, streak = 0;
    for (const ch of n) {
      if (ch === q[i]) {
        streak++;
        score += 2 + streak;
        i++;
        if (i === q.length) return score;
      } else streak = 0;
    }
    return 0;
  }
  // Nav buttons carry a leading index span ("01"); the palette wants only the label text.
  function navLabel(b) {
    const span = b.querySelector("span:not(.nav-index)");
    if (span) return span.textContent.trim().replace(/\s+/g, " ");
    const copyNode = b.cloneNode(true);
    copyNode.querySelectorAll(".nav-index").forEach((n) => n.remove());
    return copyNode.textContent.trim().replace(/\s+/g, " ");
  }
  function pageLabel(view) {
    const b = $$(".nav-link").find((x) => x.dataset.view === view);
    return b ? navLabel(b) : view || "";
  }
  // Every tagged section is searchable. The kit index is preferred; a direct DOM scan fills any gap.
  function sectionIndex() {
    const out = new Map();
    const add = (e) => {
      if (!e || !e.id || out.has(e.id)) return;
      out.set(e.id, e);
    };
    try {
      const fromKit = window.controllerUI?.index?.();
      if (Array.isArray(fromKit))
        for (const e of fromKit)
          add({
            id: e.id,
            title: e.title || e.id,
            hint: e.hint || "",
            keywords: Array.isArray(e.keywords) ? e.keywords.join(" ") : e.keywords || "",
            level: e.level || "",
            page: e.page || document.querySelector(`[data-ui-section="${CSS.escape(e.id)}"]`)?.closest(".screen")?.id || "",
          });
    } catch {}
    for (const el of $$("[data-ui-section]"))
      add({
        id: el.dataset.uiSection,
        title: el.dataset.title || el.dataset.uiSection,
        hint: el.dataset.hint || "",
        keywords: el.dataset.keywords || "",
        level: el.dataset.level || "",
        page: el.closest(".screen")?.id || "",
      });
    return [...out.values()];
  }
  // Reveal a section without the kit: open ancestor <details>, pick the haptic tab, scroll, focus, highlight.
  function revealSection(id) {
    const el = document.querySelector(`[data-ui-section="${CSS.escape(id)}"]`);
    if (!el) return;
    try {
      if (window.controllerUI?.openFor) window.controllerUI.openFor(el);
    } catch {}
    // The kit opens ancestor sections without persisting them as the user's choice; only open them by hand without it.
    if (!window.controllerUI?.openFor) for (let n = el; n; n = n.parentElement) if (n.tagName === "DETAILS") n.open = true;
    if (el.closest(".haptic-tab-hidden")) {
      const group = el.closest("[data-haptic-group]")?.dataset.hapticGroup;
      ($(`#haptic-tabs [data-haptic-tab="${group || "studio"}"]:not([hidden])`) || $('#haptic-tabs [data-haptic-tab="all"]'))?.click();
    }
    setTimeout(() => {
      el.scrollIntoView({ block: "start", behavior: "auto" });
      const target = el.matches("input,select,textarea,button") ? el : el.querySelector("input:not([type=hidden]):not(:disabled),select:not(:disabled),textarea:not(:disabled),button:not(:disabled),summary");
      if (target && target.offsetParent !== null) target.focus({ preventScroll: true });
      else {
        el.setAttribute("tabindex", "-1");
        el.focus({ preventScroll: true });
      }
      el.classList.add("palette-active");
      setTimeout(() => el.classList.toggle("palette-active", false), 1600);
    }, 60);
  }
  function recentCommands() {
    try {
      const v = JSON.parse(localStorage.getItem("controller.recentCommands") || "[]");
      return Array.isArray(v) ? v.filter((x) => typeof x === "string").slice(0, 5) : [];
    } catch {
      return [];
    }
  }
  function rememberCommand(name) {
    // Section rows are searched, not "recent": only pages, profiles and actions are remembered.
    const list = [name, ...recentCommands().filter((n) => n !== name)].slice(0, 5);
    try {
      localStorage.setItem("controller.recentCommands", JSON.stringify(list));
    } catch {}
  }
  function openPalette() {
    if (document.querySelector("dialog[open]") || state.shortcutRecording)
      return;
    const opener = document.activeElement;
    let moved = false; // a page or section was opened: focus belongs to that target
    const view = dialog("Quick switch"),
      search = node("input", {
        type: "search",
        placeholder: "Search pages, settings and actions",
        class: "palette-search",
      }),
      hint = node("small", {
        class: "palette-hint",
        text: `Type to find any setting, such as Bluetooth, gyro, Forza or macros. Open this anywhere with ${platform.shortcutLabel}.`,
      }),
      rows = node("div", { class: "studio-list" });
    search.setAttribute("aria-label", "Find a command");
    view.d.addEventListener("close", () => {
      if (!moved && opener && opener.isConnected && opener !== document.body) opener.focus({ preventScroll: true });
    }, { once: true });
    const commands = [
      ...$$(".nav-link").map((b) => ({
        name: navLabel(b),
        page: true,
        run: () => app.navigate(b.dataset.view),
      })),
      ...state.profiles.map((p) => ({
        name: `Profile: ${p.name}`,
        run: () => app.selectProfile(p.id, false),
      })),
      { name: "Apply profile to controller", run: () => app.applyProfileHardware(true) },
      { name: "Save profile", run: () => $("#save-profile")?.click() },
      { name: "Undo last action", run: () => { if (!app.undoLast?.()) app.toast("Nothing to undo right now.", { error: false }); } },
      { name: "Undo trigger edit", run: () => travel(-1) },
      { name: "Redo trigger edit", run: () => travel(1) },
      {
        name: "Start game session",
        run: () => window.gameSessions?.enqueue(() => window.gameSessions.start(profile(), true)),
      },
      { name: "End game session", run: () => window.gameSessions?.enqueue(() => window.gameSessions.restore()) },
      { name: "Stop all output", run: () => window.dailyUse?.stopAll?.() },
      { name: "Stop macros", run: () => send({ action: "stopMacros" }) },
      { name: "Reset all output", run: () => $("#quick-reset").click() },
    ];
    // Sections are listed only while searching so the first view stays short.
    const sections = sectionIndex().map((e) => {
      const page = pageLabel(e.page);
      return {
        name: page && page !== e.title ? `${e.title} \u00b7 ${page}` : e.title,
        title: e.title,
        pageName: page && page !== e.title ? page : "",
        extra: [e.keywords, e.hint, page].join(" ").toLowerCase(),
        hint: e.hint,
        section: true,
        run: () => {
          if (e.page) app.navigate(e.page);
          revealSection(e.id);
        },
      };
    });
    const recents = recentCommands();
    let shown = [];
    function draw() {
      const q = search.value.trim().toLowerCase();
      shown = q
        ? commands
            .concat(sections)
            .map((c, order) => {
              let score = fuzzyScore(c.name, q);
              if (c.page && score) score += 300;
              if (c.extra) {
                const words = c.extra.split(/\s+/);
                const kw = words.some((w) => w.startsWith(q)) ? 500 : c.extra.includes(q) ? 250 : 0;
                score = Math.max(score, kw);
              }
              return { c, order, score };
            })
            .filter((x) => x.score > 0)
            .sort((a, b) => b.score - a.score || a.order - b.order)
            .slice(0, 40)
            .map((x) => x.c)
        : [
            ...recents.map((n) => commands.find((c) => c.name === n)).filter(Boolean),
            ...commands.filter((c) => !recents.includes(c.name)),
          ];
      rows.replaceChildren(
        ...shown.map((c, i) => {
          const b = button(q || !recents.includes(c.name) ? c.title || c.name : `Recent: ${c.name}`, () => {
            if (!c.section) rememberCommand(c.name);
            if (c.section || c.page) moved = true;
            view.d.close();
            c.run();
          });
          // The page a setting lives on is a muted trailing label, not part of its name.
          if (c.pageName) b.append(node("small", { class: "palette-page", text: c.pageName }));
          b.setAttribute("aria-label", c.name);
          if (c.hint) b.title = c.hint;
          b.addEventListener("keydown", (e) => {
            const buttons = [...rows.querySelectorAll("button")];
            const at = buttons.indexOf(b);
            if (e.key === "ArrowDown") {
              e.preventDefault();
              buttons[Math.min(buttons.length - 1, at + 1)]?.focus();
            } else if (e.key === "ArrowUp") {
              e.preventDefault();
              (at ? buttons[at - 1] : search).focus();
            }
          });
          return b;
        }),
      );
      if (!shown.length) rows.append(text("No matching command."));
    }
    search.addEventListener("input", draw);
    search.addEventListener("keydown", (e) => {
      // type=search clears itself on the first Escape; close in one press instead.
      if (e.key === "Escape") {
        e.preventDefault();
        e.stopPropagation();
        view.d.close();
        return;
      }
      if (e.key === "ArrowDown") {
        e.preventDefault();
        $("button", rows)?.focus();
      }
      if (e.key === "Enter") $("button", rows)?.click();
    });
    view.body.append(search, hint, rows);
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
  // Reduced motion lives inside the existing "Layout and controls" card when it exists,
  // so there is a single place for display settings.
  const layoutBody = $("#layout-preferences .studio-body");
  const prefs = layoutBody
    ? { body: layoutBody, box: null }
    : card("Display", "display-preferences");
  if (prefs.box) $(".settings-grid").append(prefs.box);
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
    loadedValues.clear();
    seedBaseline();
    if (typeof drawCurveEditor === "function") drawCurveEditor();
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
