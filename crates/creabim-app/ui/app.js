// CreaBIM interface pages. The app sends each pane its state with
// CB.state({...}); the page draws it and posts what the user does back as
// JSON with window.ipc.postMessage.
(function () {
  "use strict";

  const pane = document.documentElement.dataset.pane;
  const root = document.getElementById("root");
  let S = null; // the last state
  const local = { open: {}, filter: "", stack: [] };

  function post(msg) {
    window.ipc.postMessage(JSON.stringify(msg));
  }

  function icon(name, cls) {
    const body = window.ICONS[name] || "";
    return `<svg class="i ${cls || ""}" viewBox="0 0 24 24">${body}</svg>`;
  }

  // the CreaBIM mark: filled, in the brand colour of the theme
  // (the small cut below 48 px, the full mark from 48 px up)
  function mark(full) {
    const name = full ? "creabim-mark-full" : "creabim-mark";
    return `<svg class="mark" viewBox="0 0 200 200" aria-label="CreaBIM">${window.ICONS[name]}</svg>`;
  }

  // The weaver and its nest, woven once where the logo lives (MU39, the
  // "Woven precise" motion): the branch draws, two strands weave the
  // nest, the nest fills, the bird lands; then a line or two of where it
  // is: a settings slider, a guide's page, a new plan. In the theme's
  // logo colour. `play` false draws it still (a redraw, or reduced motion).
  function woven(kind, play) {
    const ctxArt = {
      settings: `<path class="wm-ctx" d="M222 78 H318"/><circle class="wm-ctx" cx="252" cy="78" r="9"/><path class="wm-ctx" d="M222 126 H318"/><circle class="wm-ctx" cx="292" cy="126" r="9"/>`,
      guide: `<path class="wm-ctx" d="M222 70 H306 M222 100 H318 M222 130 H290 M222 160 H300"/>`,
      new: `<path class="wm-ctx" d="M222 62 H318 V160 H222 Z M270 62 V112 H318"/>`,
    }[kind] || "";
    const w = ctxArt ? 330 : 200;
    return `<svg class="woven ${play ? "play" : "still"}" viewBox="0 0 ${w} 200" aria-label="CreaBIM">` +
      `<g transform="translate(9 4)">` +
      `<path class="wm-branch" d="M26 52 H174"/>` +
      `<path class="wm-nest" d="M130 52 C162 64 172 100 166 128 C160 156 146 166 142 190 L118 190 C114 166 98 156 94 128 C88 100 98 64 130 52 Z"/>` +
      `<path class="wm-strand" d="M98 98 L164 124"/><path class="wm-strand s2" d="M96 132 L160 156"/>` +
      `<g class="wm-bird"><path d="M44.5 8.0 C51.2 2.1 63.8 3.0 67.2 12.2 C69.7 18.9 66.4 24.8 63.0 29.0 C61.3 39.9 52.9 50.0 38.6 50.8 C32.8 51.3 27.7 50.0 23.5 48.3 L8.4 56.7 L16.8 43.3 C14.3 35.7 16.0 27.3 21.8 22.3 C27.7 17.2 37.8 14.7 44.5 8.0 Z"/><path d="M65.5 10.5 L80.6 16.4 L65.5 21.4 Z"/><circle class="wm-eye" cx="61.5" cy="11.4" r="2.8"/></g>` +
      `</g>${ctxArt}</svg>`;
  }

  // A page's major sections (its ## headings) fold, as Properties' do
  // (MU39); what is folded stays folded while the page is redrawn.
  const folded = new Set();
  function foldSections(html, pageId) {
    const parts = html.split(/(?=<h2 )/);
    return parts.map((p, i) => {
      if (i === 0 && !p.startsWith("<h2 ")) return p;
      const end = p.indexOf("</h2>") + 5;
      const id = (p.match(/id="([^"]+)"/) || [])[1] || String(i);
      const key = pageId + "#" + id;
      return `<details class="hsec" data-fold="${esc(key)}"${folded.has(key) ? "" : " open"}><summary>${p.slice(0, end)}</summary>${p.slice(end)}</details>`;
    }).join("");
  }
  // A status message cut short slides to show the rest while the pointer
  // rests on it, and back (MU39).
  document.addEventListener("mouseover", (e) => {
    const t = e.target && e.target.closest && e.target.closest(".status .msg span[title], .status .hint");
    if (!t || t.classList.contains("slide")) return;
    const over = t.scrollWidth - t.clientWidth;
    if (over < 4) return;
    t.classList.add("slide");
    t.innerHTML = `<i>${esc(t.textContent)}</i>`;
    t.style.setProperty("--slide", `-${over + 8}px`);
    t.style.setProperty("--slide-t", `${Math.max(2, over / 40)}s`);
    t.addEventListener("mouseleave", () => {
      t.classList.remove("slide");
      t.textContent = t.textContent;
    }, { once: true });
  });
  document.addEventListener("toggle", (e) => {
    const d = e.target;
    if (d && d.dataset && d.dataset.fold) {
      if (d.open) folded.delete(d.dataset.fold); else folded.add(d.dataset.fold);
    }
  }, true);

  function esc(s) {
    return String(s == null ? "" : s).replace(/[&<>"']/g, (c) => ({
      "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;",
    }[c]));
  }

  function el(html) {
    const t = document.createElement("template");
    t.innerHTML = html.trim();
    return t.content.firstChild;
  }

  // Keep a text box being typed in when the state redraws.
  function keepFocus(draw) {
    const a = document.activeElement;
    const key = a && a.dataset ? a.dataset.key : null;
    const pos = a && a.selectionStart;
    const val = a && a.value;
    draw();
    if (key) {
      const b = root.querySelector(`[data-key="${CSS.escape(key)}"]`);
      if (b) {
        b.focus();
        if (b.value !== val && b.dataset.live !== "1") b.value = val;
        try { b.setSelectionRange(pos, pos); } catch (e) { /* not a text box */ }
      }
    }
  }

  // ---- controls shared by the top bar and the status bar ----

  // A control's tooltip: its name, its key, then what it does.
  function tip(c) {
    let name = c.label || c.value || "";
    if (c.kind === "menu" && c.label && c.value) name = `${c.label}: ${c.value}`;
    return name + (c.shortcut ? `  (${c.shortcut})` : "") + (c.hint ? (name ? "\n" : "") + c.hint : "");
  }

  // `full` draws a compact control with its words too (in the More menu).
  function control(c, full) {
    const compact = c.compact && !full;
    const slot = c.slot ? ` data-slot="${esc(c.slot)}"` : "";
    switch (c.kind) {
      case "number": {
        // a length comes formatted in the project's units, and what is
        // typed in it goes back as text for the app to read
        const text = c.text != null;
        return `<label class="field"${slot} title="${esc(tip(c))}"><span class="k" data-scrub="${esc(c.key)}" data-step="${c.step}" data-min="${c.min}" data-max="${c.max}">${esc(c.label)}</span>` +
          `<input class="${text ? "len" : ""}" data-key="${esc(c.key)}" data-step="${c.step}" data-min="${c.min}" data-max="${c.max}" data-v="${c.value}" data-text="${text ? 1 : 0}" value="${text ? esc(c.text) : fmt(c.value, c.step)}">` +
          (c.unit ? `<span class="u">${esc(c.unit)}</span>` : "") + `</label>`;
      }
      case "menu":
        // at its usual setting a status control says nothing but its icon:
        // its name and setting are in the tip (MU101)
        if (c.quiet && full !== true) {
          return `<button class="dd icon quiet ${S.popup === c.key ? "open" : ""}"${slot} data-popup="${esc(c.key)}" title="${esc(c.label)}: ${esc(c.value)}. ${esc(tip(c))}">` +
            icon(c.icon) + `</button>`;
        }
        if (compact) {
          return `<button class="dd icon ${S.popup === c.key ? "open" : ""}"${slot} data-popup="${esc(c.key)}" title="${esc(tip(c))}" ${c.enabled === false ? "disabled" : ""}>` +
            icon(c.icon) + `${icon("chevron-down", "chev")}</button>`;
        }
        return `<button class="dd ${c.big ? "big" : ""} ${c.svg ? "kindbtn" : ""} ${c.join ? "join-" + c.join : ""} ${S.popup === c.key ? "open" : ""}"${slot} data-popup="${esc(c.key)}" title="${esc(tip(c))}" ${c.enabled === false ? "disabled" : ""}>` +
          (c.svg ? c.svg : c.icon ? icon(c.icon) : "") + (c.label && !(c.icon && c.value && c.key === "step") ? `<span class="k">${esc(c.label)}</span>` : "") +
          `<span class="v">${esc(c.value)}</span>${icon("chevron-down", "chev")}</button>`;
      case "button":
        return `<button class="btn ${compact ? "icon" : ""}"${slot} data-act="${esc(c.id)}" title="${esc(tip(c))}">${c.icon ? icon(c.icon) : ""}${compact ? "" : esc(c.label)}</button>`;
      case "toggle":
        return `<button class="toggle ${compact ? "icon" : ""} ${c.on ? "on" : ""} ${c.join ? "join-" + c.join : ""}"${slot} data-act="${esc(c.id)}" data-keep="${full ? 1 : 0}" title="${esc(tip(c))}">${c.icon ? icon(c.icon) : ""}${compact ? "" : esc(c.label)}</button>`;
      case "check":
        return `<button class="toggle ${c.on ? "on" : ""}"${slot} data-act="${esc(c.id)}" data-keep="${full ? 1 : 0}" title="${esc(tip(c))}">${c.on ? icon("check") : ""}${esc(c.label)}</button>`;
      case "label":
        return `<span class="ctl-label"${slot}>${esc(c.label)}</span>`;
      case "seg":
        return `<span class="seg"${slot}>` + c.items.map((i) => `<button class="${i.on ? "on" : ""}${i.icon ? " ico" : ""}" data-act="${esc(i.id)}" data-keep="${full ? 1 : 0}" title="${esc(i.hint)}">${i.icon ? icon(i.icon) : esc(i.label)}</button>`).join("") + `</span>`;
      case "sep":
        return `<span class="vsep"${slot}></span>`;
      case "meta":
        return `<span class="meta"${slot}${c.hint ? ` title="${esc(c.hint)}"` : ""}>${esc(c.label)}</span>`;
    }
    return "";
  }

  function fmt(v, step) {
    const d = step >= 1 ? 0 : step >= 0.1 ? 1 : 2;
    return Number(v).toFixed(d);
  }

  function setNumber(input, v) {
    const min = parseFloat(input.dataset.min), max = parseFloat(input.dataset.max);
    if (!isNaN(min)) v = Math.max(min, v);
    if (!isNaN(max)) v = Math.min(max, v);
    input.dataset.v = v;
    if (input.dataset.text !== "1") input.value = fmt(v, parseFloat(input.dataset.step));
    post({ t: "set", key: input.dataset.key, value: v });
  }

  // ---- top pane ----

  function openKindsOnce() {
    if (!S || !S.open_kinds) { local.kindsAsked = false; return; }
    if (local.kindsAsked) return;
    const b = root.querySelector('[data-popup^="kinds:"]');
    if (!b) return;
    local.kindsAsked = true;
    const r = b.getBoundingClientRect();
    post({ t: "act", id: "kinds.opened" });
    post({ t: "popup", key: b.dataset.popup, x: r.left - 8, y: r.bottom, w: r.width, up: null, top: r.top - 8 + 10 });
  }

  function drawTop() {
    const menus = S.native_menu ? "" :
      `<button class="brand menubtn ${S.popup === S.app.key ? "open" : ""}" data-popup="${esc(S.app.key)}" data-menubar="1" title="About, updates, settings and exit"><span class="logo">${mark(true)}</span><span class="name">${esc(S.app.label)}</span>${icon("chevron-down", "chev")}</button>` +
      S.menus.map((m) => `<button class="menubtn ${S.popup === m.key ? "open" : ""}" data-popup="${esc(m.key)}" data-menubar="1">${esc(m.name)}</button>`).join("") +
      `<span class="vsep"></span>`;
    const quick = S.quick.map((q) => q === null ? `<span class="qsep"></span>` :
      `<button class="ibtn" data-act="${esc(q.id)}" title="${esc(q.label)}${q.key ? "  (" + esc(q.key) + ")" : ""}" ${q.enabled ? "" : "disabled"}>${icon(q.icon)}</button>`).join("");
    // the tabs, then Select and Align, then the tab's own buttons: icons
    // only, the name in the tip and, for the tool in use, in the strip below
    const tabs = (S.tabs || []).map((t) => `<button class="ttab ${t.on ? "on" : ""}" data-act="${esc(t.id)}">${esc(t.label)}</button>`).join("");
    const tools = S.tools.map((t) =>
      t.kind === "sep" ? `<span class="vsep"></span>` :
      t.kind === "menu" ? `<button class="tool ${t.open ? "on" : ""}" data-popup="${esc(t.key)}" title="${esc(t.label + ": " + t.hint)}">${icon(t.icon)}${icon("chevron-down", "tchev")}</button>` :
      // a tool family (tool families REV04, AMD-001 Blender style,
      // softened): the button is its last tool, the caret opens its panel
      t.kind === "family" ? `<span class="split ${t.on ? "on" : ""} ${t.open ? "open" : ""}">` +
        `<button class="tool" data-act="${esc(t.act)}" title="${esc(t.label + (t.key_hint ? "  (" + t.key_hint + ")" : "") + " · " + t.family + "\n" + t.hint)}">${icon(t.icon)}</button>` +
        `<button class="caret" data-popup="${esc(t.key)}" title="${esc(t.family)} tools">${icon("chevron-down")}</button></span>` :
      `<button class="tool ${t.active ? "on" : ""} ${t.off ? "off" : ""}" data-act="${esc(t.id)}" title="${esc(t.label + ": " + t.hint)}">${icon(t.icon)}</button>`).join("");
    const bar1 = `<div class="bar1">${menus}` +
      `<div class="quick">${quick}</div><span class="spacer"></span><div class="doc" title="${esc(S.doc.path)}${S.doc.unsaved === false ? "" : " · unsaved changes"}">${S.doc.unsaved === false ? "" : `<span class="dot"></span>`}${esc(S.doc.name)}</div>${S.right.map(control).join("")}</div>`;
    const titlebar = S.mac_titlebar;
    const html =
      (titlebar ? `<div class="mac-titlebar${titlebar.right ? " right" : ""}" style="--titlebar-inset:${Number(titlebar.inset)}px">${bar1}</div>` : bar1) +
      `<div class="bar2"><div class="ttabs">${tabs}</div><div class="tools">${tools}</div><span class="spacer"></span></div>` +
      // the tool options strip: the tool in use and what it can be set to,
      // then the active view's own settings at the right. What doesn't fit
      // goes in a More menu at the end (the app picks what, from the
      // widths measured below).
      `<div class="bar3"><div class="toolname">${S.tool.icon ? icon(S.tool.icon) : ""}<b>${esc(S.tool.label)}</b>` +
      (S.tool.help ? `<button class="qhelp ${S.popup === "toolhelp" ? "open" : ""}" data-popup="toolhelp" title="How to use the ${esc(S.tool.label)} tool (F1 opens its page)">${icon("circle-help")}</button>` : "") + `</div>` +
      (S.tool_options.length ? `<div class="options tool-opts">${shown(S.tool_options).map((c) => control(c)).join("")}</div>` +
        // Select keeps its hint after its tools: what is selected (MU39)
        (S.tool.trail ? `<div class="tool-hint" title="${esc(S.tool.hint)}">${esc(S.tool.hint || "")}</div>` : "") :
        `<div class="tool-hint" title="${esc(S.tool.hint)}">${esc(S.tool.hint || "")}</div>`) +
      `<span class="spacer"></span>` +
      (S.tool_options.length && S.options.length ? `<span class="vsep group-sep"></span>` : "") +
      `<div class="options view-opts">${shown(S.options).map((c) => control(c)).join("")}</div>` +
      (S.more ? `<button class="dd more-btn ${S.popup === "more" ? "open" : ""}" data-popup="more" title="${S.more} more ${S.more === 1 ? "option" : "options"} that don't fit here">${icon("ellipsis")}<span class="v">More</span></button>` : "") +
      `</div>` +
      // every control at its own width, out of sight, to measure
      `<div class="bar3 measure" aria-hidden="true"><div class="m-tool">${S.tool_options.map((c) => control(c)).join("")}</div>` +
      `<div class="m-view">${S.options.map((c) => control(c)).join("")}</div>` +
      `<span class="m-more"><button class="dd more-btn">${icon("ellipsis")}<span class="v">More</span></button></span></div>`;
    const cut = html.indexOf(`<div class="bar3 measure"`);
    // a row scrolled sideways stays where it was
    const scrolled = ROWS.map((sel) => { const r = root.querySelector(sel); return r ? r.scrollLeft : 0; });
    // the measured copies answer no clicks and hold no typing
    root.innerHTML = html.slice(0, cut) + html.slice(cut).replace(/ data-(key|act|popup|scrub)=/g, " data-m$1=");
    ROWS.forEach((sel, i) => {
      const r = root.querySelector(sel);
      r.scrollLeft = scrolled[i];
      r.addEventListener("scroll", () => edges(r));
      edges(r);
    });
    measureBar();
    // Keep the native hit region disabled until the matching padding is laid out.
    if (titlebar && titlebar.revision !== local.titlebar) {
      local.titlebar = titlebar.revision;
      post({ t: "mac-titlebar", revision: titlebar.revision });
    }
  }

  // The menu and tool rows. In a narrow window they scroll sideways, with
  // the wheel too, and fade at an end that has more past it.
  const ROWS = [".bar1", ".bar2"];
  function edges(row) {
    const more = row.scrollWidth - row.clientWidth;
    row.classList.toggle("more-l", more > 1 && row.scrollLeft > 1);
    row.classList.toggle("more-r", more > 1 && row.scrollLeft < more - 1);
  }
  document.addEventListener("wheel", (e) => {
    const row = pane === "top" && e.target.closest && e.target.closest(ROWS.join(","));
    if (!row || row.scrollWidth <= row.clientWidth) return;
    e.preventDefault();
    row.scrollLeft += 2 * (e.deltaX || e.deltaY);
  }, { passive: false });

  // The controls the More menu doesn't hold.
  function shown(list) {
    return list.filter((c) => !c.more);
  }

  // Tell the app how wide each control is and how much room the strip
  // has, when that changes; it answers with what goes in More.
  function measureBar() {
    const bar = root.querySelector(".bar3:not(.measure)");
    const m = root.querySelector(".bar3.measure");
    if (!bar || !m) return;
    const w = (e) => Math.ceil(e.getBoundingClientRect().width);
    const items = [];
    const take = (sel, view, list) => {
      [...m.querySelectorAll(sel + " > *")].forEach((k, i) => {
        const cs = getComputedStyle(k);
        const out = w(k) + Math.ceil(parseFloat(cs.marginLeft) + parseFloat(cs.marginRight));
        items.push({ k: (list[i] && list[i].slot) || "", w: out, v: view });
      });
    };
    take(".m-tool", false, S.tool_options);
    take(".m-view", true, S.options);
    const name = bar.querySelector(".toolname");
    const hint = bar.querySelector(".tool-hint");
    // the side padding, the gaps round the name and the spacer, the
    // divider between the groups, and room for a little of the hint
    const fixed = 20 + w(name) + 3 * 8 + 13 + (hint ? 120 : 0);
    const msg = { t: "bar", avail: bar.clientWidth - fixed, gap: 6, more: w(m.querySelector(".m-more button")) + 6, items };
    const key = JSON.stringify(msg);
    if (key !== local.bar) {
      local.bar = key;
      post(msg);
    }
  }

  window.addEventListener("resize", () => {
    if (pane !== "top" || !S) return;
    measureBar();
    ROWS.forEach((sel) => { const r = root.querySelector(sel); if (r) edges(r); });
  });

  // ---- left pane: project browser ----

  function drawLeft() {
    const f = local.filter.trim().toLowerCase();
    const anyOpen = S.groups.some((g) => (local.open[g.key] !== undefined ? local.open[g.key] : g.open));
    // folded to a rail (MU27): one icon a group; a click opens the browser
    // on that group
    if (S.folded) {
      // a click lists the group's items beside the rail, to pick one
      // without opening the browser (MU29); the wheel scrolls the rail
      let rail = `<div class="rail">`;
      for (const g of S.groups) rail += `<button class="rail-btn" data-popup="rail:${esc(g.key)}" title="${esc(g.title)} (${g.rows.length})">${icon(g.icon)}</button>`;
      root.innerHTML = rail + `</div>`;
      return;
    }
    let html = `<div class="browser"><div class="browser-head"><div class="title">${icon("layout-dashboard")}Project<span class="count">${esc(S.count)}</span>` +
      `<button class="gact fold" data-fold="${anyOpen ? 0 : 1}" title="${anyOpen ? "Collapse every group" : "Expand every group"}">${icon(anyOpen ? "chevrons-down-up" : "chevrons-up-down")}</button>` +
      `</div>` +
      `<label class="search">${icon("search")}<input data-key="filter" data-live="1" placeholder="Find views and elements" value="${esc(local.filter)}"></label></div><div class="tree">`;
    let any = false;
    for (const g of S.groups) {
      const rows = f ? g.rows.filter((r) => r.name.toLowerCase().includes(f) || (r.mark && r.mark.toLowerCase().includes(f))) : g.rows;
      if (f && rows.length === 0) continue;
      any = true;
      const open = f ? true : (local.open[g.key] !== undefined ? local.open[g.key] : g.open);
      const acts = (g.acts || []).map((a) => `<button class="gact" data-act="${esc(a.id)}" title="${esc(a.hint)}">${icon(a.icon)}</button>`).join("");
      const note = g.note ? `<span class="gnote">${esc(g.note)}</span>` : "";
      const m = local.moving;
      const moving = m && m.key === g.key && !f && performance.now() - m.at < 220 ? (m.open ? "anim-open" : "anim-close") : "";
      // the buttons wait over the count until the row is hovered, so the
      // name and the counts keep their places (Browser rows REV00 A)
      html += `<div class="group ${open ? "open" : ""} ${moving}" data-group="${esc(g.key)}">${icon("chevron-right", "chev")}${icon(g.icon)}<span class="gt">${esc(g.title)}</span>${note}${acts ? `<span class="gacts">${acts}</span>` : ""}<span class="n">${g.rows.length}</span></div>`;
      // a group shutting keeps its rows while they slide away
      if (open || moving === "anim-close") {
        // rows of a sortable group can be dragged to another place in it
        const sort = g.sortable && !f ? ` data-sort="${esc(g.key)}"` : "";
        html += `<div class="grows ${moving}"><div>`;
        // an offer the group makes (Groups: "these look like options")
        if (g.offer && !f) {
          const b = (g.offer.acts || []).map((a) => `<button class="btn${a.primary ? " pri" : ""}" data-act="${esc(a.id)}">${esc(a.label)}</button>`).join("");
          html += `<div class="goffer"><div>${esc(g.offer.text)}</div><div class="gob">${b}</div></div>`;
        }
        for (const r of rows) {
          // a row that can be hidden carries its eye at the right
          const eye = r.eye === undefined ? "" : `<button class="reye${r.eye ? "" : " off"}" data-act="${esc(r.eyeAct)}" title="${r.eye ? "Hide it in the views" : "Show it again"}">${icon(r.eye ? "eye" : "eye-off")}</button>`;
          html += `<div class="row ${r.sel ? "sel" : ""}${r.kid ? " kid" : ""}" data-id="${r.id}"${sort} title="${esc(r.hint || r.name)}${sort ? " · drag to reorder" : ""}">${icon(r.icon)}<span>${esc(r.name)}</span>${r.active ? `<span class="dot"></span>` : ""}${eye}</div>`;
        }
        html += `</div></div>`;
      }
    }
    if (!any) html += emptyState(`Nothing matches “${esc(local.filter)}”`, "Try a type name, a level or a mark.");
    html += `</div>${S.dock ? `<div class="tdock"></div>` : ""}</div>`;
    // the time strip ticks every second: when nothing else changed only
    // the strip is drawn again, so the tree keeps its place and focus
    if (local.leftHtml === html && root.querySelector(".tdock")) { drawDock(); return; }
    local.leftHtml = html;
    const tree = root.querySelector(".tree");
    const scroll = tree ? tree.scrollTop : 0;
    keepFocus(() => { root.innerHTML = html; });
    const t2 = root.querySelector(".tree");
    if (t2) t2.scrollTop = scroll;
    markCutNames();
    drawDock();
    showDrag();
  }

  // a group's name fades only where it is cut (Browser rows REV00 A)
  function markCutNames() {
    root.querySelectorAll(".group > .gt").forEach((t) => t.classList.toggle("cut", t.scrollWidth > t.clientWidth + 1));
  }
  window.addEventListener("resize", () => { if (pane === "left") markCutNames(); });

  // Project time pinned at the browser's foot (MU101, B): the clock,
  // Pause and Unpin, then the stage and its budget; the clock opens the card
  function drawDock() {
    const el = root.querySelector(".tdock"), t = S.dock;
    if (!el || !t) return;
    const go = t.manual && !t.running && t.clock === "00:00:00";
    const html = `<div class="td-l1"><button class="td-open" data-popup="time" title="Open the time card"><span class="dot ${esc(t.cls)}${t.pulse ? "" : " still"}"></span><span class="td-clk">${esc(t.clock)}</span></button>` +
      `<button class="td-b" data-act="time.pause" title="${t.running ? "Pause" : go ? "Start" : "Resume"}">${icon(t.running ? "pause" : "play")}</button>` +
      `<button class="td-b on" data-act="time.pin" title="Unpin: back to the pill in the status bar">${icon("pin")}</button></div>` +
      `<div class="td-st" title="${esc(t.line)}">${esc(t.line)}</div>` +
      `<div class="tbar"><i class="${esc(t.budget)}" style="width:${t.use === null || t.use === undefined ? 0 : Math.max(2, t.use)}%"></i></div>`;
    if (el.__html !== html) { el.innerHTML = html; el.__html = html; }
  }

  // ---- dragging a view to another place in its group ----

  // The row being dragged and where it would land: {id, group, y, on, over, after}.
  function showDrag() {
    const d = local.drag;
    root.querySelectorAll(".row.dragging, .row.drop-before, .row.drop-after").forEach((r) => r.classList.remove("dragging", "drop-before", "drop-after"));
    if (!d || !d.on) return;
    const me = root.querySelector(`.row[data-id="${CSS.escape(d.id)}"]`);
    if (me) me.classList.add("dragging");
    const over = d.over && root.querySelector(`.row[data-id="${CSS.escape(d.over)}"]`);
    if (over) over.classList.add(d.after ? "drop-after" : "drop-before");
  }

  function dragMove(e) {
    const d = local.drag;
    if (!d) return;
    if (!d.on && Math.abs(e.clientY - d.y) < 5) return;
    d.on = true;
    const el = document.elementFromPoint(e.clientX, e.clientY);
    const row = el && el.closest(".row");
    if (row && row.dataset.sort === d.group) {
      const r = row.getBoundingClientRect();
      d.over = row.dataset.id;
      d.after = e.clientY > r.top + r.height / 2;
    }
    showDrag();
  }

  function dragEnd() {
    const d = local.drag;
    local.drag = null;
    document.removeEventListener("mousemove", dragMove);
    document.removeEventListener("mouseup", dragEnd);
    if (!d || !d.on) return showDrag();
    // a click follows the mouse-up on the same row: it isn't a pick
    local.dragged = true;
    setTimeout(() => { local.dragged = false; }, 0);
    showDrag();
    if (!d.over || d.over === d.id) return;
    const rows = [...root.querySelectorAll(`.row[data-sort="${CSS.escape(d.group)}"]`)].map((r) => r.dataset.id);
    let i = rows.indexOf(d.over) + (d.after ? 1 : 0);
    if (rows[i] === d.id) i += 1;
    post({ t: "reorder", id: d.id, before: rows[i] || null });
  }

  // ---- status pane ----

  function drawStatus() {
    const mouse = (S.mouse || []).map((m) => `<span class="mb"><svg viewBox="0 0 12 16"><rect x="1" y="1" width="10" height="14" rx="5"/><path class="${m.b}" d="${m.b === "l" ? "M1 6 V6 a5 5 0 0 1 5 -5 V6 Z" : m.b === "r" ? "M11 6 a5 5 0 0 0 -5 -5 V6 Z" : "M5 2.5 h2 v3 h-2 Z"}"/></svg>${esc(m.label)}</span>`).join("");
    // project time's pill: today's working time and the stage, the dot
    // beating while it counts (Mockup_Project_Time_REV02)
    const t = S.timer;
    const pill = t ? `<button class="tpill ${esc(t.cls)}${t.pulse ? "" : " still"} ${S.popup === "time" ? "open" : ""}" data-popup="time" title="${esc(t.hint)}">` +
      `<span class="dot"></span><span class="tclock">${esc(t.text)}</span>${t.stage ? `<span class="tstage">${esc(t.stage)}</span>` : ""}</button>` : "";
    const html = `<div class="status">${pill}<span class="mice">${mouse}</span><div class="msg"><span class="pulse"></span><span title="${esc(S.message)}">${esc(S.message)}</span>${S.hint ? `<span class="hint" title="${esc(S.hint)}">${esc(S.hint)}</span>` : ""}</div>` +
      S.controls.map(control).join("") + `</div>`;
    // nothing changed, nothing drawn: hover and the beating dot stay put
    if (local.statusHtml === html && root.firstElementChild) return;
    local.statusHtml = html;
    root.innerHTML = html;
  }

  // ---- the 3D view's navigation buttons ----

  function drawNav() {
    let html = `<div class="nav">`;
    for (const b of S.buttons) {
      if (!b) continue;
      html += `<button class="ibtn ${b.on ? "on" : ""}" data-act="${esc(b.id)}" title="${esc(b.hint)}" ${b.off ? "disabled" : ""}>${icon(b.icon)}</button>`;
    }
    // dropdowns: what the 3D view shows
    for (const m of S.menus || []) {
      const label = m.label ? `<span class="v">${esc(m.label)}</span>` : "";
      html += `<span class="qsep"></span><button class="dd ${m.on ? "on" : ""} ${S.popup === m.key ? "open" : ""}" data-popup="${esc(m.key)}" title="${esc(m.hint)}">${icon(m.icon)}${label}${icon("chevron-down", "chev")}</button>`;
    }
    html += `<span class="qsep"></span><button class="dd ${S.popup === S.look.key ? "open" : ""}" data-popup="${esc(S.look.key)}" title="${esc(S.look.hint)}">${icon("axis-3d")}<span class="v">View</span>${icon("chevron-down", "chev")}</button></div>`;
    root.innerHTML = html;
  }

  // ---- command palette ----

  function drawPalette() {
    if (local.key !== S.key) {
      local.key = S.key;
      local.query = "";
      local.hot = 0;
    }
    const words = (local.query || "").toLowerCase().split(/\s+/).filter(Boolean);
    const found = S.items.filter((i) => i.id && words.every((w) => i.label.toLowerCase().includes(w)));
    local.found = found;
    local.hot = Math.max(0, Math.min(local.hot || 0, found.length - 1));
    let html = `<div class="palette"><label class="pq">${icon("search")}<input data-key="palette" data-live="1" placeholder="Type a command, a tool or a view" value="${esc(local.query || "")}"></label><div class="pl">`;
    found.slice(0, 60).forEach((i, n) => {
      html += `<button class="mi ${n === local.hot ? "hot" : ""}" data-act="${esc(i.id)}" data-n="${n}" title="${esc(i.hint)}"><span class="ck">${i.icon ? icon(i.icon) : ""}</span><span class="label">${esc(i.label)}</span>${i.key ? `<span class="key">${esc(i.key)}</span>` : ""}</button>`;
    });
    if (found.length === 0) html += emptyState(`Nothing matches “${esc(local.query || "")}”`, "Try a tool, a command or a view’s name.");
    html += `</div><div class="pfoot"><span>↑↓ choose</span><span>Enter run</span><span>Esc close</span></div></div>`;
    keepFocus(() => { root.innerHTML = html; });
    const input = root.querySelector("input[data-key=palette]");
    if (input && document.activeElement !== input) { input.focus(); input.setSelectionRange(input.value.length, input.value.length); }
    const hot = root.querySelector(".mi.hot");
    if (hot) hot.scrollIntoView({ block: "nearest" });
    // the height it wants, measured without the window's cap, so a window
    // made taller gives the list its room back
    const pal = root.querySelector(".palette");
    pal.style.maxHeight = "none";
    const want = Math.ceil(pal.scrollHeight) + 2;
    pal.style.maxHeight = "";
    post({ t: "size", w: window.innerWidth, h: want });
  }

  // ---- popup menus ----

  function drawPopup() {
    if (S.close) return drawClose(S.close);
    if (S.report) return drawReport(S.report);
    if (S.mark) return drawMark(S.mark);
    if (S.time) return drawTime(S.time);
    if (S.kinds) return drawKinds(S.kinds);
    local.timeNote = false;
    local.stageOpen = false;
    if (S.tpl) return drawTpl(S.tpl);
    if (S.dialog) return drawDialog(S.dialog);
    if (S.key === "palette" && S.items && S.items.length) return drawPalette();
    if (S.controls && S.controls.length) return drawPanel(controlsHtml());
    if (S.help) return drawPanel(helpHtml());
    if (!S.items || S.items.length === 0) {
      root.innerHTML = "";
      local.stack = [];
      local.key = null;
      return;
    }
    if (local.key !== S.key) {
      local.key = S.key;
      local.stack = [];
      local.hot = -1;
    }
    // Follow the open submenu into fresh items, by label.
    let items = S.items;
    let title = null;
    for (const label of local.stack) {
      const sub = items.find((i) => i.items && i.label === label);
      if (!sub) { local.stack = []; items = S.items; title = null; break; }
      items = sub.items;
      title = label;
    }
    const row = (i) => {
      if (i.sep) return `<div class="msep"></div>`;
      if (i.header) return `<div class="mhead">${esc(i.header)}</div>`;
      if (i.items) return `<button class="mi" data-sub="${esc(i.label)}" ${i.enabled ? "" : "disabled"}><span class="ck">${i.icon ? icon(i.icon) : ""}</span><span class="label">${esc(i.label)}</span>${icon("chevron-right", "sub")}</button>`;
      // one icon column for every row; a toggle's state at the row's end,
      // before its shortcut (A7-6). A toggle without an icon shows its
      // tick in the icon column, as a plain check item.
      // a number typed in the menu (Divisions, MU102): Enter applies it
      if (i.id && i.id.startsWith("input:")) {
        return `<label class="mi mfield"><span class="ck">${icon("hash")}</span><input type="text" inputmode="numeric" data-menuinput="${esc(i.id.slice(6))}" placeholder="${esc(i.label)}" value="${esc(i.hint)}" spellcheck="false"></label>`;
      }
      const toggle = i.checked != null;
      const lead = i.icon ? icon(i.icon) : (i.checked ? icon("check") : "");
      const state = toggle && i.icon ? `<span class="state">${i.checked ? icon("check") : ""}</span>` : "";
      return `<button class="mi ${toggle && i.checked ? "on" : ""}" data-act="${esc(i.id)}" data-keep="${i.keep ? 1 : 0}" title="${esc(i.hint)}" ${i.enabled ? "" : "disabled"}>` +
        `<span class="ck">${lead}</span><span class="label">${esc(i.label)}</span>${state}${i.key ? `<span class="key">${esc(i.key)}</span>` : ""}</button>`;
    };
    const cols = S.columns > 1 && title === null ? columns(items, S.columns) : null;
    let html = `<div class="mwrap"><div class="menu ${cols ? "cols" : ""}" ${cols ? `style="--cols:${cols.length}"` : ""}>`;
    if (title !== null) html += `<button class="head" data-back="1">${icon("arrow-left")}<span>${esc(title)}</span></button><div class="msep"></div>`;
    if (cols) {
      // a long menu with sections reads as a panel of columns
      for (const col of cols) html += `<div class="col">${col.map(row).join("")}</div>`;
    } else {
      html += items.map(row).join("");
    }
    html += `</div><div class="more">${icon("chevron-down")}</div></div>`;
    root.innerHTML = html;
    if (local.hot >= 0) {
      const b = root.querySelectorAll(".menu .mi:not(:disabled), .menu .head")[local.hot];
      if (b) b.classList.add("hot");
    }
    const m = root.querySelector(".menu");
    m.addEventListener("scroll", showMore);
    local.sentH = local.sentW = null;
    reportSize();
    showMore();
  }

  // ---- New project: two large choices and the samples to learn from ----

  // New project (Templates in the app, REV05): sources on the left, the
  // picked source's tiles in the middle, the picked start on the right.
  function dialogItems(d) {
    const src = d.sources.find((s) => s.key === local.src) || d.sources[0];
    // the search field narrows a source's tiles by name and line
    const q = (local.find || "").trim().toLowerCase();
    const items = q ? src.items.filter((c) => (c.title + " " + c.line).toLowerCase().includes(q)) : src.items;
    return { src, items };
  }
  // A set's sheet as the choices on the right make it: A3 or ARCH B, with
  // or without CreaBIM's mark (MU102: unticking the mark changed nothing)
  function picOf(c) {
    if (!c || !c.pics || !local.set) return c && c.pic;
    return c.pics[(local.set.us ? "us" : "metric") + "|" + (local.set.mark ? "mark" : "plain")] || c.pic;
  }
  function drawDialog(d) {
    const fresh = local.key !== S.key;
    // the search field being typed in: drawn again, it keeps the keyboard
    // (MU102: one letter, then the field let go)
    const typing = !fresh && document.activeElement && document.activeElement.matches && document.activeElement.matches("[data-find]");
    if (fresh) { local.key = S.key; local.src = d.sources[0].key; local.pick = null; local.find = ""; local.std = d.standard; local.set = { us: d.standard === "us", start: "empty", mark: true }; local.zoom = false; }
    const { src, items } = dialogItems(d);
    if (!items.some((c) => c.id === local.pick)) local.pick = items.length ? items[0].id : null;
    const picked = items.find((c) => c.id === local.pick);
    const side = d.sources.map((s) => `<button class="np-src ${s.key === src.key ? "on" : ""}" data-src="${esc(s.key)}">` +
      `${icon(s.icon)}<span>${esc(s.label)}</span>${s.count === undefined || s.count === null ? "" : `<i>${s.count}</i>`}</button>`).join("");
    // every source's pictures are decoded once, as the window opens: a
    // source's first showing blinked while its pictures decoded (MU100)
    if (fresh) {
      local.pics = [];
      for (const s of d.sources) for (const c of s.items) {
        for (const url of [c.pic].concat(c.pics ? Object.values(c.pics) : [])) {
          if (!url) continue;
          const im = new Image();
          im.src = url;
          if (im.decode) im.decode().catch(() => {});
          local.pics.push(im);
        }
      }
    }
    const tile = (c) => `<button class="np-big ${c.earlier ? "small " : ""}${local.pick === c.id ? "on" : ""}" data-pick="${esc(c.id)}" title="${esc(c.line)}">` +
      `<span class="np-pic">${c.pic ? `<img ${c.set ? 'class="fit" ' : ""}decoding="sync" src="${esc(picOf(c))}" alt="">` : icon(c.icon)}${c.badge ? `<i class="np-badge">${esc(c.badge)}</i>` : ""}</span><span class="np-text">${c.tag ? `<span class="np-title"><b>${esc(c.title)}</b><i class="np-tag">${esc(c.tag)}</i></span>` : `<b>${esc(c.title)}</b>`}<span>${esc(c.line)}</span></span></button>`;
    const empty = src.empty ? `<div class="np-empty"><span class="mark-still">${markSvg("still", 56)}</span><b>${esc(src.empty.title)}</b><span>${esc(src.empty.line)}</span>` +
      (src.folder ? `<button class="np-btn" data-np="folder">Show the folder</button>` : "") + `</div>` : "";
    const under = items.length && (src.hint || src.folder) ? `<div class="np-under"><span>${esc(src.hint || "")}</span>` +
      (src.folder ? `<button class="np-btn" data-np="folder">Show the folder</button>` : "") + `</div>` : "";
    // an empty project takes its units from a drawing standard, picked here
    const std = picked && picked.id === "new.empty" ? `<div class="np-row"><span>Standard</span><span class="np-seg">` +
      d.standards.map(([k, n]) => `<button class="${local.std === k ? "on" : ""}" data-std="${esc(k)}">${esc(n)}</button>`).join("") + `</span></div>` : "";
    // one of CreaBIM's sets: its sizes, what it starts with, and the mark
    const seg = (what, v, label, on) => `<button class="${on ? "on" : ""}" data-set="${what}" data-v="${v}">${esc(label)}</button>`;
    const setRows = (c) => !c.set ? "" :
      `<div class="np-row"><span>Sizes</span><span class="np-seg">${seg("us", "", "Metric", !local.set.us)}${seg("us", "1", "US", local.set.us)}</span></div>` +
      `<div class="np-row stack"><span>Starts as</span><span class="np-seg">${seg("start", "empty", "Empty", local.set.start === "empty")}${seg("start", "starter", "Ridgeline", local.set.start === "starter")}${seg("start", "maple", "Maple Lane", local.set.start === "maple")}</span></div>` +
      `<button class="np-tick ${local.set.mark ? "on" : ""}" data-set="mark" role="checkbox" aria-checked="${local.set.mark}" title="CreaBIM's mark, faint behind the drawings, and “Drawn in CreaBIM” in the title block. Untick to leave them off.">` +
      `<span class="tp-box">${local.set.mark ? icon("check") : ""}</span><span>CreaBIM's mark on the sheets</span></button>`;
    const prev = picked ? `<span class="np-pic${picked.pic ? " zoomable" : ""}"${picked.pic ? ` data-np="zoom" title="Look closer (Space)"` : ""}>${picked.pic ? `<img ${picked.set ? 'class="fit" ' : ""}decoding="sync" src="${esc(picOf(picked))}" alt="">` : icon(picked.icon)}</span><b>${esc(picked.title)}</b><span class="np-line">${esc(picked.line)}</span>` +
      (picked.pic ? `<button class="np-zoomhint" data-np="zoom" title="Look closer (Space)">${icon("zoom-in")}<span>Look closer</span><span class="np-key">Space</span></button>` : "") +
      std + setRows(picked) + `<div class="np-row"><span>Holds</span><span>${esc(picked.holds || "")}</span></div>` :
      `<span class="np-none">Pick a start on the left</span>`;
    // the tiles scroll between the title and the buttons when the window
    // is too short for them (a small window, or Windows' larger text):
    // Create and Cancel were out of reach below the edge (issue 24)
    const was = root.querySelector(".np-body");
    const top = was && !fresh ? was.scrollTop : 0;
    const sideHtml = `${side}<span class="spacer"></span>` +
      `<button class="np-link" data-np="open">${icon("folder-open")}<span>Open a file…</span></button>` +
      `<button class="np-link np-more" data-np="more"><span>More templates to download</span></button>`;
    const bodyHtml =
      (src.items.length > 4 || local.find ? `<label class="np-find">${icon("search")}<input type="text" data-find placeholder="Find one" value="${esc(local.find || "")}"><i>${items.length} of ${src.items.length}</i></label>` : "") +
      `<div class="np-sub">${esc(src.sub || "")}</div>` +
      (items.length ? `<div class="np-tiles">${items.filter((c) => !c.earlier).map(tile).join("")}</div>` +
        // the starter houses of earlier versions, under their own heading
        (items.some((c) => c.earlier) ? `<div class="np-sub">${esc(src.earlier || "Earlier")}</div><div class="np-tiles">${items.filter((c) => c.earlier).map(tile).join("")}</div>` : "") + under :
        (src.items.length ? `<div class="np-empty"><span class="mark-still">${markSvg("still", 56)}</span><b>Nothing matches “${esc(local.find)}”</b><span>Try part of its name.</span></div>` : empty));
    // a change inside the window redraws only what changed: the card, its
    // title and its buttons stay as they are, and a part whose content is
    // the same is not touched, so nothing blinks (MU120)
    const card = !fresh && root.querySelector(".np:not(.tp)");
    if (card) {
      // a part with pictures changes only once they are decoded: drawn at
      // once, the tiles showed empty for a frame while their pictures
      // loaded, a blink on the first visit to a source (MU102)
      const put = (sel, html) => {
        const el = card.querySelector(sel);
        if (!el || el.__html === html) return;
        el.__html = html;
        const next = document.createElement("div");
        next.innerHTML = html;
        const imgs = [...next.querySelectorAll("img")].filter((i) => !(i.complete && i.naturalWidth));
        const swap = () => { if (el.__html !== html) return; el.replaceChildren(...next.childNodes); };
        if (!imgs.length || !imgs[0].decode) { swap(); return; }
        const was = el.querySelector("[data-find]") === document.activeElement;
        Promise.all(imgs.map((i) => i.decode().catch(() => {}))).then(() => {
          swap();
          if (was) { const f = el.querySelector("[data-find]"); if (f) { f.focus(); f.setSelectionRange(f.value.length, f.value.length); } }
        });
      };
      put(".np-side", sideHtml);
      put(".np-body", bodyHtml);
      put(".np-prev", prev);
      const go = card.querySelector('[data-np="create"]');
      if (go) go.disabled = !picked;
    } else {
      root.innerHTML = `<div class="np"><div class="np-head"><span class="logo np-logo">${woven("new", fresh)}</span><b>${esc(d.title)}</b><span class="np-esc">Esc to close</span></div>` +
        `<div class="np-main"><div class="np-side">${sideHtml}</div>` +
        `<div class="np-body">${bodyHtml}</div>` +
        `<div class="np-prev">${prev}</div></div>` +
        `<div class="np-foot"><span>${esc(d.note)}</span><span class="spacer"></span><button class="np-btn" data-np="cancel">Cancel</button>` +
        `<button class="np-btn primary" data-np="create" ${picked ? "" : "disabled"}>Create</button></div></div>`;
      for (const [sel, html] of [[".np-side", sideHtml], [".np-body", bodyHtml], [".np-prev", prev]]) root.querySelector(sel).__html = html;
    }
    drawZoom(d, items, picked, seg);
    const body = root.querySelector(".np-body");
    if (body) {
      body.scrollTop = top;
      const on = body.querySelector(".on");
      if (on && on.scrollIntoView) on.scrollIntoView({ block: "nearest" });
    }
    // typing in the search field keeps its place in the field
    const find = root.querySelector("[data-find]");
    if (find && !find.__wired) {
      find.__wired = true;
      if (local.finding || typing) { local.finding = true; find.focus(); find.setSelectionRange(find.value.length, find.value.length); }
      find.addEventListener("input", () => { local.find = find.value; local.finding = true; local.pick = null; drawDialog(S.dialog); });
      find.addEventListener("blur", () => { local.finding = false; });
    }
    // the app is told the window has drawn itself: it comes on whole
    if (fresh) post({ t: "size", w: window.innerWidth, h: window.innerHeight });
  }

  // Save as template (Templates in the app, REV05): a short card, its
  // parts behind a fold. All it chooses stays here until Save.
  function drawTpl(t) {
    const fresh = local.key !== S.key || !local.tp;
    if (fresh) { local.key = S.key; local.tp = { name: t.name, parts: t.parts.map((p) => p.on), to: "mine", open: false }; }
    const tp = local.tp;
    const was = root.querySelector("[data-tp=name]");
    if (was && !fresh) tp.name = was.value;
    const all = tp.parts[t.building];
    const seg = (what, v, label, on) => `<button class="${on ? "on" : ""}" data-tp="${what}" data-v="${v}">${esc(label)}</button>`;
    const part = (p, i) => `<button class="tp-part ${tp.parts[i] ? "on" : ""}" data-tp="part" data-i="${i}" role="checkbox" aria-checked="${tp.parts[i]}">` +
      `<span class="tp-box">${tp.parts[i] ? icon("check") : ""}</span><span class="tp-text"><b>${esc(p.title)}</b>` +
      `<span>${p.count === null || p.count === undefined ? "" : p.count + " · "}${esc(p.line)}</span></span></button>`;
    root.innerHTML = `<div class="np tp" role="dialog" aria-modal="true"><div class="np-head"><span class="logo np-logo">${woven("new", fresh)}</span><b>Save as template</b><span class="np-esc">Esc to close</span></div>` +
      `<p class="tp-line">Keeps this project's look as a start for new projects. It appears under My templates in New project.</p>` +
      `<div class="tp-row"><span>Name</span><input type="text" data-tp="name" value="${esc(tp.name)}" spellcheck="false"></div>` +
      `<div class="tp-row"><span>Keeps</span><span class="np-seg">${seg("keep", "look", "The look only", !all)}${seg("keep", "all", "The look and the building", all)}</span></div>` +
      `<button class="tp-fold ${tp.open ? "open" : ""}" data-tp="fold">${icon(tp.open ? "chevron-down" : "chevron-right")}<span>Choose the parts</span></button>` +
      (tp.open ? `<div class="tp-parts">${t.parts.map(part).join("")}</div>` : "") +
      `<div class="tp-row"><span>Save to</span><span class="np-seg">${seg("to", "mine", "My templates", tp.to === "mine")}${seg("to", "file", "A file…", tp.to === "file")}</span></div>` +
      `<div class="np-foot"><span>${all ? "" : "The building is left out."}</span><span class="spacer"></span><button class="np-btn" data-tp="cancel">Cancel</button>` +
      `<button class="np-btn primary" data-tp="save">Save template</button></div></div>`;
    const name = root.querySelector("[data-tp=name]");
    if (name && fresh) { name.focus(); name.select(); }
    tplSize(fresh);
  }
  // The app gives the card the height it needs: measured again when its
  // window changes width (it is first laid out while parked, narrower).
  function tplSize(force) {
    const card = root.querySelector(".tp");
    if (!card || !(S && S.tpl)) return;
    const h = Math.ceil(card.getBoundingClientRect().height) + 2;
    if (force || h !== local.tpH) { local.tpH = h; post({ t: "size", w: window.innerWidth, h }); }
  }
  window.addEventListener("resize", () => tplSize(false));
  function tplClick(b, t) {
    const tp = local.tp, what = b.dataset.tp;
    const name = root.querySelector("[data-tp=name]");
    if (name) tp.name = name.value;
    if (what === "name") return;
    if (what === "cancel") return post({ t: "tpl", do: "cancel" });
    if (what === "save") return post({ t: "tpl", do: "save", name: tp.name, parts: tp.parts, to: tp.to });
    if (what === "fold") tp.open = !tp.open;
    if (what === "to") tp.to = b.dataset.v;
    if (what === "keep") tp.parts[t.building] = b.dataset.v === "all";
    if (what === "part") tp.parts[+b.dataset.i] = !tp.parts[+b.dataset.i];
    // a building is kept with its views and types
    if (tp.parts[t.building]) t.parts.forEach((p, i) => { if (p.key === "views" || p.key === "types") tp.parts[i] = true; });
    drawTpl(t);
  }
  function tplKey(e) {
    const t = S && S.tpl;
    if (!t) return false;
    if (e.key === "Escape") post({ t: "tpl", do: "cancel" });
    else if (e.key === "Enter") { const n = root.querySelector("[data-tp=name]"); if (n) local.tp.name = n.value; post({ t: "tpl", do: "save", name: local.tp.name, parts: local.tp.parts, to: local.tp.to }); }
    else return false;
    e.preventDefault();
    return true;
  }

  // Look closer (MU101, A): the picked start's sheet fills the window's
  // middle, drawn from the set itself (an SVG, so its lines and words stay
  // sharp at any size); arrows step through the source's tiles, the set's
  // sizes and start stay at hand, Create and Cancel stay below, Esc or
  // Back returns to the tiles
  function drawZoom(d, items, picked, seg) {
    const card = root.querySelector(".np:not(.tp)");
    if (!card) return;
    let z = card.querySelector(".np-zoom");
    const on = local.zoom && picked && picked.pic;
    if (!on) { if (z) z.remove(); return; }
    const withPic = items.filter((c) => c.pic);
    const i = withPic.findIndex((c) => c.id === picked.id);
    const sets = picked.set ?
      `<span class="np-seg">${seg("us", "", "Metric", !local.set.us)}${seg("us", "1", "US", local.set.us)}</span>` +
      `<span class="np-seg">${seg("start", "empty", "Empty", local.set.start === "empty")}${seg("start", "starter", "Ridgeline", local.set.start === "starter")}${seg("start", "maple", "Maple Lane", local.set.start === "maple")}</span>` : "";
    const html = `<div class="nz-bar"><button class="np-btn" data-np="unzoom">${icon("chevron-left")}<span>Back</span></button>` +
      `<b>${esc(picked.title)}</b><span class="nz-of">${i + 1} of ${withPic.length}${picked.set ? (local.set.us ? " · ARCH B" : " · A3") : ""}</span><span class="spacer"></span>${sets}</div>` +
      `<div class="nz-stage"><button class="nz-arrow" data-np="prev" title="The one before (←)" ${withPic.length > 1 ? "" : "disabled"}>${icon("chevron-left")}</button>` +
      `<div class="nz-sheet"><img ${picked.set ? 'class="fit" ' : ""}decoding="sync" src="${esc(picOf(picked))}" alt="${esc(picked.title)}"></div>` +
      `<button class="nz-arrow" data-np="next" title="The next one (→)" ${withPic.length > 1 ? "" : "disabled"}>${icon("chevron-right")}</button></div>` +
      `<div class="nz-line">${esc(picked.line)}${picked.set ? (local.set.start === "starter" ? " · starts with Ridgeline House, this version's starter: its drawings and sheets on this look" : local.set.start === "maple" ? " · starts with Maple Lane House, the starter of 1.1: its drawings and sheets on this look" : " · starts empty: two levels, nothing drawn") : ""}</div>` +
      `<div class="nz-dots">${withPic.map((c) => `<i class="${c.id === picked.id ? "on" : ""}"></i>`).join("")}</div>`;
    if (!z) { z = document.createElement("div"); z.className = "np-zoom"; card.querySelector(".np-main").appendChild(z); }
    if (z.__html !== html) { z.innerHTML = html; z.__html = html; }
  }
  function zoomStep(by) {
    const d = S && S.dialog;
    if (!d) return;
    const all = dialogItems(d).items.filter((c) => c.pic).map((c) => c.id);
    if (!all.length) return;
    const i = Math.max(0, all.indexOf(local.pick));
    local.pick = all[(i + by + all.length) % all.length];
    drawDialog(d);
  }

  // What Create starts: an empty project takes the standard picked here.
  function dialogAct() {
    if (!local.pick) return;
    if (local.pick.startsWith("new.set:")) {
      const s = local.set;
      post({ t: "act", id: [local.pick, s.us ? "us" : "metric", s.start, s.mark ? "mark" : "plain"].join("|") });
      return;
    }
    post({ t: "act", id: local.pick === "new.empty" && local.std ? "new.empty:" + local.std : local.pick });
  }

  // ---- Save the changes (REV02 compact): the unsaved nest asks ----

  // The woven mark waiting to be saved (animation bank 1 + 2 + 5): the
  // bird hovers over its shadow, the nest is pale and breathes, its
  // outline marches, two strands weave, an orange dot pulses. Saved, the
  // weave closes, the nest fills, the dot turns green and the bird lands.
  function unsavedNest() {
    const nest = "M130 52 C162 64 172 100 166 128 C160 156 146 166 142 190 L118 190 C114 166 98 156 94 128 C88 100 98 64 130 52 Z";
    return `<svg class="nest" width="46" height="49" viewBox="0 -14 212 224" aria-hidden="true"><g transform="translate(9 4)">` +
      `<path class="wm-branch" d="M26 52 H174"/><ellipse class="wm-shadow" cx="40" cy="46" rx="22" ry="3.5"/>` +
      `<path class="wm-nest" d="${nest}"/><path class="wm-strand" d="M98 98 L164 124"/><path class="wm-strand s2" d="M96 132 L160 156"/>` +
      `<path class="wm-rim" d="${nest}"/><circle class="wm-dot" cx="184" cy="20" r="12"/>` +
      `<g class="wm-bird"><path d="M44.5 8.0 C51.2 2.1 63.8 3.0 67.2 12.2 C69.7 18.9 66.4 24.8 63.0 29.0 C61.3 39.9 52.9 50.0 38.6 50.8 C32.8 51.3 27.7 50.0 23.5 48.3 L8.4 56.7 L16.8 43.3 C14.3 35.7 16.0 27.3 21.8 22.3 C27.7 17.2 37.8 14.7 44.5 8.0 Z"/><path d="M65.5 10.5 L80.6 16.4 L65.5 21.4 Z"/><circle class="wm-eye" cx="61.5" cy="11.4" r="2.8"/></g></g></svg>`;
  }

  // A long file name in a card's title, shortened in the middle so its
  // start and its extension stay: "Maple Lane House for…studio).creabim".
  // The whole name is the title's hover text.
  function shortName(name, max = 44) {
    if (!name || name.length <= max) return name || "";
    const dot = name.lastIndexOf(".");
    const ext = dot > 0 && name.length - dot <= 10 ? name.slice(dot) : "";
    const keep = max - 1 - ext.length;
    const head = Math.ceil(keep * 0.6), tail = keep - head;
    return name.slice(0, head).trimEnd() + "…" + name.slice(name.length - ext.length - tail);
  }

  // Drawn once for a question; Saving… and Saved change it in place, so
  // the nest's motion carries on into its settling.
  function drawClose(c) {
    const failed = c.phase === "failed" || (c.phase === "saving" && c.error);
    const key = [c.file, c.fresh, c.line, failed ? c.error : ""].join("|");
    const asking = c.phase === "ask" || c.phase === "failed";
    let dlg = root.querySelector(".cs");
    // (a Save as… gives the file its name while it saves: the window
    // drawn for the question stays, so the nest settles where it is)
    if (!dlg || (asking && (dlg.dataset.k !== key || dlg.classList.contains("saved")))) {
      const fn = `<span class="fn" title="${esc(c.file)}">${esc(shortName(c.file))}</span>`;
      const title = failed ? `Couldn't save ${fn}` : c.fresh ? "Save your new project before closing?" : `Save ${fn} before closing?`;
      const line = failed ? "Your changes are still open. Nothing was lost." : esc(c.line);
      root.innerHTML = `<div class="cs unsaved" role="dialog" aria-modal="true" aria-labelledby="cs-t">` +
        `<div class="cs-body"><div class="cs-tile unsaved">${unsavedNest()}</div>` +
        `<div><h2 id="cs-t">${title}</h2><div class="cs-sub">${line}</div></div>` +
        (failed ? `<div class="cs-err">${esc(c.error)}</div>` : "") + `</div>` +
        // the buttons main first; Settings > Look > Window buttons puts them
        // on the right, main last (the default), or on the left
        `<div class="cs-foot"><div class="bs">` +
        // (icons stand in for the words when the window is narrow)
        `<button class="cs-btn primary focus" data-cs="save" title="Save, then close (Enter)"><span class="spin"></span>${icon("save", "ic")}<span class="lbl tx"></span></button>` +
        `<button class="cs-btn danger" data-cs="discard" title="Don't save: close and let the changes go">${icon("trash-2", "ic")}<span class="tx">Don't save</span></button>` +
        `<button class="cs-btn" data-cs="cancel" title="Cancel: back to the project (Esc)">${icon("undo-2", "ic")}<span class="tx">Cancel</span></button>` +
        `</div><span class="cs-sp"></span><span class="cs-hints"><kbd>Enter</kbd> save <kbd>Esc</kbd> back</span></div></div>`;
      dlg = root.querySelector(".cs");
      dlg.dataset.k = key;
      local.closeH = null;
    }
    const main = c.fresh || failed ? "Save as…" : "Save";
    const lbl = dlg.querySelector(".lbl");
    dlg.classList.toggle("saving", c.phase === "saving");
    if (c.phase === "saved") {
      if (!dlg.classList.contains("saved")) {
        dlg.querySelectorAll(".unsaved").forEach((e) => { e.classList.remove("unsaved"); e.classList.add("saved"); });
        dlg.classList.remove("unsaved");
        dlg.classList.add("saved", "closing");
        dlg.querySelector("h2").textContent = "Saved.";
        dlg.querySelector(".cs-sub").innerHTML = `Everything is in <b>${esc(c.file)}</b>. Closing CreaBIM…`;
        const err = dlg.querySelector(".cs-err");
        if (err) err.remove();
      }
      lbl.textContent = "Saved";
    } else {
      lbl.textContent = c.phase === "saving" ? "Saving…" : main;
    }
    closeSize();
  }

  // Save the changes is as tall as it needs, measured uncut; the window
  // may give less, and then it scrolls. Measured again when the pane is
  // placed or resized (it is drawn first while parked, at another width).
  function closeSize() {
    const dlg = root.querySelector(".cs");
    if (!dlg) return;
    const body = dlg.querySelector(".cs-body");
    const keep = [dlg.style.height, body.style.flex, body.style.overflowY];
    dlg.style.height = "auto"; body.style.flex = "none"; body.style.overflowY = "visible";
    dlg.style.maxHeight = "none";
    const h = Math.ceil(dlg.getBoundingClientRect().height);
    dlg.style.maxHeight = "";
    [dlg.style.height, body.style.flex, body.style.overflowY] = keep;
    // (again when the window was opened anew at another height)
    if (h !== local.closeH || Math.abs(window.innerHeight - h) > 1) {
      local.closeH = h;
      post({ t: "size", w: window.innerWidth, h });
    }
  }
  window.addEventListener("resize", closeSize);

  // Enter saves, Esc goes back; nothing while it saves.
  function closeKey(e) {
    const c = S && S.close;
    if (!c) return false;
    const busy = c.phase === "saving" || c.phase === "saved";
    if (e.key === "Enter" && !busy) closeAct("save");
    else if (e.key === "Escape" && !busy) closeAct("cancel");
    else if (e.key !== "Tab") return false;
    if (e.key !== "Tab") e.preventDefault();
    return true;
  }

  function closeAct(what) {
    const dlg = root.querySelector(".cs");
    if (!dlg || dlg.classList.contains("saving") || dlg.classList.contains("saved")) return;
    // the button spins at once; the app saves on its next frame
    if (what === "save") {
      dlg.classList.add("saving");
      dlg.querySelector(".lbl").textContent = "Saving…";
    }
    post({ t: "closeask", do: what });
  }


  // ---- Report a problem (Mockup_Report_Card_REV00, C) ----

  // The woven mark with a note pinned to the branch: the tag sways while
  // it asks; its lines are written in while the report is saved; its dot
  // turns green once saved.
  function reportTag() {
    const nest = "M110 52 C142 64 152 100 146 128 C140 156 126 166 122 190 L98 190 C94 166 78 156 74 128 C68 100 78 64 110 52 Z";
    return `<svg class="rpn" width="46" height="49" viewBox="0 -14 224 224" aria-hidden="true"><g transform="translate(9 4)">` +
      `<path class="wm-branch" d="M26 52 H196"/>` +
      `<path class="wm-nest" d="${nest}"/><path class="wm-strand" d="M78 98 L144 124"/><path class="wm-strand s2" d="M76 132 L140 156"/>` +
      `<g class="wm-bird"><path d="M44.5 8.0 C51.2 2.1 63.8 3.0 67.2 12.2 C69.7 18.9 66.4 24.8 63.0 29.0 C61.3 39.9 52.9 50.0 38.6 50.8 C32.8 51.3 27.7 50.0 23.5 48.3 L8.4 56.7 L16.8 43.3 C14.3 35.7 16.0 27.3 21.8 22.3 C27.7 17.2 37.8 14.7 44.5 8.0 Z"/><path d="M65.5 10.5 L80.6 16.4 L65.5 21.4 Z"/><circle class="wm-eye" cx="61.5" cy="11.4" r="2.8"/></g>` +
      `<g class="rp-tag"><path class="rp-ink" d="M182 52 L182 66"/><path class="rp-paper" d="M160 66 L204 66 L204 130 L160 130 Z"/>` +
      `<path class="rp-ink rp-ln l1" d="M168 82 L196 82"/><path class="rp-ink rp-ln l2" d="M168 98 L192 98"/><path class="rp-ink rp-ln l3" d="M168 114 L188 114"/>` +
      `<circle class="rp-dot" cx="200" cy="66" r="9"/></g></g></svg>`;
  }

  function drawReport(r) {
    let dlg = root.querySelector(".cs.rp");
    if (!dlg) {
      root.innerHTML = `<div class="cs rp asking" role="dialog" aria-modal="true" aria-labelledby="rp-t">` +
        `<div class="cs-body"><div class="cs-tile">${reportTag()}</div>` +
        `<div><h2 id="rp-t">Report a problem</h2><div class="cs-sub">CreaBIM saves one file to attach to a GitHub issue or an email: what it did lately, the steps that were slow, any crash, your settings, your system and graphics card. Nothing is sent anywhere.</div>` +
        `<button class="rp-tick" data-rp="tick" title="It helps rebuild the problem exactly, but it is your work: leave it off if it is private"><span class="rp-box"></span>Include the project as it is now</button></div></div>` +
        `<div class="cs-foot"><div class="bs">` +
        `<button class="cs-btn primary focus" data-rp="save" title="Pick where to save it (Enter)"><span class="spin"></span>${icon("save", "ic")}<span class="lbl tx">Save the report…</span></button>` +
        `<button class="cs-btn" data-rp="cancel" title="Back to the project (Esc)">${icon("undo-2", "ic")}<span class="tx">Cancel</span></button>` +
        `</div><span class="cs-sp"></span><span class="cs-hints"><kbd>Enter</kbd> save <kbd>Esc</kbd> back</span></div></div>`;
      dlg = root.querySelector(".cs.rp");
      local.closeH = null;
    }
    dlg.classList.toggle("asking", r.phase === "ask" || r.phase === "failed");
    dlg.classList.toggle("saving", r.phase === "saving");
    dlg.classList.toggle("saved", r.phase === "saved");
    dlg.querySelector(".rp-tick").classList.toggle("on", !!r.project);
    dlg.querySelector(".lbl").textContent = r.phase === "saving" ? "Saving…" : r.phase === "saved" ? "Saved" : "Save the report…";
    const sub = dlg.querySelector(".cs-sub");
    let err = dlg.querySelector(".cs-err");
    if (r.phase === "saved") {
      dlg.querySelector("h2").textContent = "Report saved.";
      sub.innerHTML = `In <b>${esc(r.line)}</b>. Attach it to a GitHub issue or an email.`;
    }
    if (r.phase === "failed") {
      if (!err) { err = document.createElement("div"); err.className = "cs-err"; dlg.querySelector(".cs-body").appendChild(err); }
      err.textContent = `Couldn't save it: ${r.line}`;
    } else if (err) err.remove();
    closeSize();
  }

  // Enter saves, Esc goes back; nothing while it saves.
  function reportKey(e) {
    const r = S && S.report;
    if (!r) return false;
    const busy = r.phase === "saving" || r.phase === "saved";
    if (e.key === "Enter" && !busy) reportAct("save");
    else if (e.key === "Escape" && !busy) reportAct("cancel");
    else if (e.key !== "Tab") return false;
    if (e.key !== "Tab") e.preventDefault();
    return true;
  }

  function reportAct(what) {
    const dlg = root.querySelector(".cs.rp");
    if (!dlg || dlg.classList.contains("saving") || dlg.classList.contains("saved")) return;
    if (what === "tick") dlg.querySelector(".rp-tick").classList.toggle("on");
    post({ t: "report", do: what });
  }

  // An empty list: a faded, still nest, a bold line and what to try.
  // (`bold` is already escaped.)
  function emptyState(bold, hint) {
    return `<div class="empty-state"><span class="mark-still">${markSvg("still", 40)}</span><b>${bold}</b><span>${esc(hint)}</span></div>`;
  }

  // ---- Mark moments (REV02): the export and import cards ----

  const NEST_D = "M130 52 C162 64 172 100 166 128 C160 156 146 166 142 190 L118 190 C114 166 98 156 94 128 C88 100 98 64 130 52 Z";
  const BIRD_D = `<path d="M44.5 8.0 C51.2 2.1 63.8 3.0 67.2 12.2 C69.7 18.9 66.4 24.8 63.0 29.0 C61.3 39.9 52.9 50.0 38.6 50.8 C32.8 51.3 27.7 50.0 23.5 48.3 L8.4 56.7 L16.8 43.3 C14.3 35.7 16.0 27.3 21.8 22.3 C27.7 17.2 37.8 14.7 44.5 8.0 Z" pathLength="1"/><path d="M65.5 10.5 L80.6 16.4 L65.5 21.4 Z" pathLength="1"/><circle class="wm-eye" cx="61.5" cy="11.4" r="2.8"/>`;

  // The woven mark of the cards (and the About lockup and the empty lists):
  // its parts move only by the classes it is given.
  function markSvg(cls, w) {
    const h = Math.round(w * 224 / 212);
    const twigs = /iAB/.test(cls)
      ? `<path class="wm-twig t1" d="M112 120 L146 108"/><path class="wm-twig t2" d="M108 150 L150 140"/><path class="wm-twig t3" d="M116 88 L144 80"/>`
      : "";
    return `<svg class="nest mkn ${cls}" width="${w}" height="${h}" viewBox="0 -14 212 224" aria-hidden="true"><g transform="translate(9 4)">` +
      `<path class="wm-branch" d="M26 52 H174" pathLength="1"/><ellipse class="wm-shadow" cx="40" cy="46" rx="22" ry="3.5"/>` +
      `<path class="wm-nest" d="${NEST_D}" pathLength="1"/><path class="wm-strand" d="M98 98 L164 124"/><path class="wm-strand s2" d="M96 132 L160 156"/>` +
      `<path class="wm-rim" d="${NEST_D}"/>${twigs}<circle class="wm-dot" cx="184" cy="20" r="12"/><g class="wm-bird">${BIRD_D}</g></g></svg>`;
  }

  function markTitle(m) {
    const fn = `<span class="fn" title="${esc(m.file)}">${esc(shortName(m.file))}</span>`;
    const exp = m.kind === "export";
    if (m.phase === "choose") return `Export ${esc(m.name)}`;
    if (m.phase === "working") return exp ? `Exporting ${fn}…` : `Opening ${fn}…`;
    if (m.phase === "failed") return exp ? `Couldn't export ${fn}` : `Couldn't open ${fn}`;
    return exp ? "Exported." : `Opened ${fn}`;
  }

  function markSub(m) {
    const exp = m.kind === "export";
    if (m.phase === "choose") return m.choices ? "Choose how it goes out, then where it is written." : "";
    if (m.phase === "working") return exp ? "CreaBIM stays open while it works." : "Reading the file. Nothing in your project changes yet.";
    if (m.phase === "failed") return exp ? "Your project is as it was. Nothing was lost." : "Your project is as it was.";
    if (exp) {
      const what = m.files > 1 ? `${m.files} PDFs` : `<b title="${esc(m.file)}">${esc(shortName(m.file, 40))}</b>`;
      const unit = m.unit || "element";
      return `${what} · ${esc(m.size)} · ${m.count} ${unit}${m.count === 1 ? "" : "s"} · ${esc(m.secs)}`;
    }
    return esc(m.line);
  }

  function markProg(m) {
    const unit = m.unit || "element";
    const count = m.total > 0 ? `${m.n} of ${m.total} ${unit}s` : "";
    // the time so far rides with the step: "Making the project · 12 s"
    const step = [m.step, m.secs].filter(Boolean).join(" · ");
    return { count, step, pct: m.total > 0 ? Math.min(100, 100 * m.n / m.total) : 2 };
  }

  function markMid(m) {
    if (m.phase === "choose" && m.choices) {
      const c = m.choices;
      const rows = c.rows.map((r) => `<button class="mk-row ${c.on === r.id ? "on" : ""}" data-mk="${esc(c.key)}" data-v="${esc(r.id)}" ${r.off ? "disabled" : ""}>` +
        `<span class="mk-radio"></span><span><b>${esc(r.title)}</b><span>${esc(r.line)}</span></span></button>`).join("");
      const each = c.each == null ? "" :
        `<div class="mk-seg"><span>As</span><button class="${c.each ? "" : "on"}" data-mk="each" data-v="0">One PDF</button><button class="${c.each ? "on" : ""}" data-mk="each" data-v="1">A PDF each</button></div>`;
      const white = c.white == null ? "" :
        `<div class="mk-seg"><span>Paper</span><button class="${c.white ? "" : "on"}" data-mk="white" data-v="0">As the master</button><button class="${c.white ? "on" : ""}" data-mk="white" data-v="1">Print on white</button></div>`;
      return `<div class="mk-full mk-choose"><div class="mk-q">${esc(c.title)}</div>${rows}${each}${white}<div class="mk-note">${esc(c.note)}</div></div>`;
    }
    if (m.phase === "working") {
      const p = markProg(m);
      return `<div class="mk-full"><div class="mk-bar"><i style="width:${p.pct}%"></i></div><div class="mk-prog"><span class="n">${esc(p.count)}</span><span class="w">${esc(p.step)}</span></div></div>`;
    }
    if (m.phase === "failed") return `<div class="mk-full mk-err">${esc(m.error)}</div>`;
    if (m.kind === "export") return `<div class="mk-full mk-path" title="${esc(m.path)}"><bdi>${esc(m.path)}</bdi></div>`;
    // import: the verdict, the kinds, the list
    let html = `<div class="mk-full">`;
    if (m.flag) {
      html += `<div class="mk-flag"><i></i><span>${esc(m.flag)}${m.more ? ` <em>+ ${m.more} more note${m.more === 1 ? "" : "s"}</em>` : ""}</span>` +
        (m.select ? `<button class="mk-link" data-mk="select">Select them</button>` : "") + `</div>`;
    } else {
      html += `<div class="mk-flag good"><i></i><span>Everything came in as it was written.</span></div>`;
    }
    html += `<div class="mk-chips">${(m.chips || []).map((c) => `<span class="mk-chip"><b>${c.n}</b> ${esc(c.kind)}</span>`).join("")}` +
      (m.morekinds ? `<span class="mk-chip">+ ${m.morekinds} more kind${m.morekinds === 1 ? "" : "s"}</span>` : "") + `</div>`;
    const notes = m.notes || [], details = m.details || [];
    if (notes.length || details.length) {
      html += `<div class="mk-fold ${local.markFold ? "open" : ""}"><button class="mk-fold-h" data-mk="fold">${icon("chevron-right", "chev")}` +
        `<span>The whole report${details.length ? ` · ${details.length} element${details.length === 1 ? "" : "s"} by name` : ""}</span></button>` +
        `<div class="mk-fold-b"><div class="mk-fold-in"><div class="mk-lines">${markGroups(m)}</div></div></div></div>`;
    }
    return html + `</div>`;
  }

  // The whole report: each note, folding open on the elements it names
  // (a file of 1,000 notes reads as its dozen reasons first).
  function markGroups(m) {
    const groups = m.groups || (m.notes || []).map((n) => ({ note: n, items: [] }));
    // the house fold: a chevron that turns, the text hanging beside it
    return groups.map((g) => {
      if (!g.items.length) return `<div class="mk-g leaf"><span class="mk-g-dot"></span><span>${esc(g.note)}</span></div>`;
      return `<div class="mk-g"><button class="mk-g-h" data-mk="gfold">${icon("chevron-right", "chev")}<span>${esc(g.note)}</span></button>` +
        `<div class="mk-g-b">${g.items.map((d) => `<div>${esc(d)}</div>`).join("")}</div></div>`;
    }).join("");
  }

  // The buttons main first, then the hints; the side they sit on is the
  // page's data-dir (Settings > Look > Window buttons).
  function markFoot(m) {
    const exp = m.kind === "export";
    const foot = (btns, hints) => `<div class="bs">${btns}</div><span class="mk-sp"></span><span class="mk-hints">${hints}</span>`;
    if (m.phase === "choose") {
      return foot(`<button class="mk-btn primary" data-mk="go">Export…</button><button class="mk-btn" data-mk="cancel">Cancel</button>`,
        `<kbd>Enter</kbd> export <kbd>Esc</kbd> back`);
    }
    if (m.phase === "working") return foot(`<button class="mk-btn" data-mk="stop">Stop</button>`, `<kbd>Esc</kbd> stop`);
    if (m.phase === "failed") {
      return foot(`<button class="mk-btn primary" data-mk="done">Close</button>` + (exp ? `<button class="mk-btn" data-mk="again">Try again</button>` : ""),
        `<kbd>Enter</kbd> close`);
    }
    if (exp) {
      return foot(`<button class="mk-btn primary" data-mk="done">Done</button><button class="mk-btn" data-mk="folder" title="Show in folder">${icon("folder-open", "ic")}<span class="tx">Show in folder</span></button>`,
        `<kbd>Enter</kbd> done`);
    }
    return foot(`<button class="mk-btn primary" data-mk="done">Done</button><button class="mk-btn" data-mk="copy" title="Copy report">${icon("copy", "ic")}<span class="tx">Copy report</span></button>`,
      `File › Import report shows it again`);
  }

  function markTile(m) {
    if (m.phase === "working") return m.kind === "export" ? "xA" : "iAB";
    if (m.phase === "done") return m.kind === "export" || m.good ? "dot-good landing closed" : "dot-warn landing closed";
    if (m.phase === "failed") return "dot-bad";
    return "still";
  }

  // Drawn once a phase; the bar moves in place, and the working nest
  // settles where it is when the job ends.
  function drawMark(m) {
    const key = [m.kind, m.phase, m.file, m.what || "", m.phase === "choose" ? JSON.stringify(m.choices || {}) : ""].join("|");
    let card = root.querySelector(".mk");
    if (card && card.dataset.k === key) {
      if (m.phase === "working") {
        const p = markProg(m);
        card.querySelector(".mk-bar i").style.width = p.pct + "%";
        card.querySelector(".mk-prog .n").textContent = p.count;
        card.querySelector(".mk-prog .w").textContent = p.step;
        if (m.kind === "import") card.querySelector(".wm-nest").style.opacity = (0.25 + 0.6 * p.pct / 100).toFixed(2);
      }
      return markSize();
    }
    const settling = card && card.dataset.phase === "working" && m.phase !== "working" && card.dataset.kind === m.kind;
    if (settling) {
      // the same nest lands: only its classes change
      const svg = card.querySelector("svg.nest");
      svg.setAttribute("class", "nest mkn " + markTile(m));
      svg.querySelector(".wm-nest").style.opacity = "";
      card.querySelector("h2").innerHTML = markTitle(m);
      card.querySelector(".mk-sub").innerHTML = markSub(m);
      card.querySelector(".mk-mid").innerHTML = markMid(m);
      card.querySelector(".mk-foot").innerHTML = markFoot(m);
    } else {
      root.innerHTML = `<div class="mk" role="dialog" aria-modal="true" aria-labelledby="mk-t">` +
        `<div class="mk-body"><div class="mk-tile">${markSvg(markTile(m), 46)}</div>` +
        `<div><h2 id="mk-t">${markTitle(m)}</h2><div class="mk-sub">${markSub(m)}</div></div>` +
        `<div class="mk-mid">${markMid(m)}</div></div><div class="mk-foot">${markFoot(m)}</div></div>`;
      card = root.querySelector(".mk");
      local.markH = null;
    }
    card.dataset.k = key;
    card.dataset.phase = m.phase;
    card.dataset.kind = m.kind;
    markSize();
  }

  // As tall as it needs, measured uncut (as Save the changes).
  function markSize() {
    const card = root.querySelector(".mk");
    if (!card || local.markFolding) return;
    const body = card.querySelector(".mk-body");
    const keep = [card.style.maxHeight, body.style.flex, body.style.overflowY];
    card.style.maxHeight = "none"; body.style.flex = "none"; body.style.overflowY = "visible";
    const h = Math.ceil(card.getBoundingClientRect().height);
    [card.style.maxHeight, body.style.flex, body.style.overflowY] = keep;
    if (h !== local.markH || Math.abs(window.innerHeight - h) > 1) {
      local.markH = h;
      post({ t: "size", w: window.innerWidth, h });
    }
  }
  window.addEventListener("resize", markSize);
  // The whole report opens as the project tree's groups do. The pane is
  // made tall enough first (it is clear below the card), then the card
  // grows into it smoothly; closing, it shrinks first and the pane follows.
  // The app keeps the card's top where it is, so nothing jumps.
  function markFold(b) {
    const fold = b.closest(".mk-fold");
    const card = root.querySelector(".mk");
    const open = !fold.classList.contains("open");
    local.markFold = open;
    const still = window.matchMedia && window.matchMedia("(prefers-reduced-motion: reduce)").matches;
    if (open) {
      const extra = fold.querySelector(".mk-fold-in").scrollHeight;
      const h = Math.ceil(card.getBoundingClientRect().height) + extra;
      local.markH = h;
      post({ t: "size", w: window.innerWidth, h });
    }
    local.markFolding = true;
    requestAnimationFrame(() => {
      fold.classList.toggle("open", open);
      setTimeout(() => { local.markFolding = false; markSize(); }, still ? 0 : 230);
    });
  }

  function markKey(e) {
    const m = S && S.mark;
    if (!m) return false;
    if (e.key === "Enter") post({ t: "mark", do: "enter" });
    else if (e.key === "Escape") post({ t: "mark", do: "esc" });
    else return e.key === "Tab";
    e.preventDefault();
    return true;
  }

  function markClick(b) {
    const what = b.dataset.mk, value = b.dataset.v || "";
    if (what === "fold") return markFold(b);
    // a note of the whole report folds in place (inside its scrolling list)
    if (what === "gfold") return b.closest(".mk-g").classList.toggle("open");
    // a choice shows at once; the app keeps it
    if (value && (what === "ifcbody" || what === "scope" || what === "each" || what === "white")) {
      b.parentElement.querySelectorAll(`[data-mk="${what}"]`).forEach((x) => x.classList.toggle("on", x === b));
    }
    post({ t: "mark", do: what, value });
  }

  function dialogKey(e) {
    if (closeKey(e)) return true;
    if (reportKey(e)) return true;
    if (markKey(e)) return true;
    if (tplKey(e)) return true;
    const d = S && S.dialog;
    if (!d) return false;
    // arrows move through the tiles of the source shown; Tab goes to the
    // next source; Enter creates what is picked
    const all = dialogItems(d).items.map((c) => c.id);
    let i = Math.max(0, all.indexOf(local.pick));
    if (e.key === "Escape" && local.zoom) { local.zoom = false; drawDialog(d); }
    else if (e.key === "Escape") post({ t: "close" });
    else if (e.key === " " && !local.finding && dialogItems(d).items.some((c) => c.id === local.pick && c.pic)) { local.zoom = !local.zoom; drawDialog(d); }
    else if (local.zoom && (e.key === "ArrowRight" || e.key === "ArrowLeft")) zoomStep(e.key === "ArrowRight" ? 1 : -1);
    else if (local.finding && e.key !== "Enter" && e.key !== "Tab") return false;
    else if (e.key === "Enter") dialogAct();
    else if (e.key === "Tab") {
      const k = d.sources.map((s) => s.key);
      const n = k.length, at = Math.max(0, k.indexOf(local.src));
      local.src = k[(at + (e.shiftKey ? n - 1 : 1)) % n]; local.pick = null; drawDialog(d);
    }
    else if ((e.key === "ArrowRight" || e.key === "ArrowDown") && all.length) { local.pick = all[(i + 1) % all.length]; drawDialog(d); }
    else if ((e.key === "ArrowLeft" || e.key === "ArrowUp") && all.length) { local.pick = all[(i + all.length - 1) % all.length]; drawDialog(d); }
    else return false;
    e.preventDefault();
    return true;
  }

  document.addEventListener("click", (e) => {
    const cs = S && S.close && e.target.closest && e.target.closest("[data-cs]");
    if (cs) { closeAct(cs.dataset.cs); return; }
    const rp = S && S.report && e.target.closest && e.target.closest("[data-rp]");
    if (rp) { reportAct(rp.dataset.rp); return; }
    const mk = S && S.mark && e.target.closest && e.target.closest("[data-mk]");
    if (mk) { markClick(mk); return; }
    const tpb = S && S.tpl && e.target.closest && e.target.closest("[data-tp]");
    if (tpb) { tplClick(tpb, S.tpl); return; }
    if (!(S && S.dialog)) return;
    const pick = e.target.closest && e.target.closest("[data-pick]");
    if (pick) { local.pick = pick.dataset.pick; drawDialog(S.dialog); return; }
    const src = e.target.closest && e.target.closest("[data-src]");
    if (src) { local.src = src.dataset.src; local.pick = null; local.find = ""; local.finding = false; local.zoom = false; drawDialog(S.dialog); return; }
    const st = S && S.dialog && e.target.closest && e.target.closest("[data-set]");
    if (st) {
      const k = st.dataset.set;
      if (k === "mark") local.set.mark = !local.set.mark;
      else if (k === "us") local.set.us = !!st.dataset.v;
      else local.set.start = st.dataset.v;
      drawDialog(S.dialog);
      return;
    }
    const sd = e.target.closest && e.target.closest("[data-std]");
    if (sd) { local.std = sd.dataset.std; drawDialog(S.dialog); return; }
    const b = e.target.closest && e.target.closest("[data-np]");
    if (b && b.dataset.np === "zoom") { local.zoom = true; drawDialog(S.dialog); return; }
    if (b && b.dataset.np === "unzoom") { local.zoom = false; drawDialog(S.dialog); return; }
    if (b && (b.dataset.np === "prev" || b.dataset.np === "next")) { zoomStep(b.dataset.np === "next" ? 1 : -1); return; }
    if (b && b.dataset.np === "cancel") post({ t: "close" });
    if (b && b.dataset.np === "create") dialogAct();
    if (b && b.dataset.np === "folder") post({ t: "act", id: "template.folder", keep: true });
    if (b && b.dataset.np === "open") post({ t: "act", id: "open" });
    if (b && b.dataset.np === "more") post({ t: "act", id: "templates.more" });
  });
  document.addEventListener("dblclick", (e) => {
    const pick = S && S.dialog && e.target.closest && e.target.closest("[data-pick]");
    if (pick) { local.pick = pick.dataset.pick; dialogAct(); }
  });

  // A tool's help card closes when the pointer, having been on it, leaves.
  document.documentElement.addEventListener("mouseenter", () => { local.inside = true; });
  document.documentElement.addEventListener("mouseleave", () => {
    if (pane === "popup" && S && S.help && local.inside) post({ t: "close" });
    local.inside = false;
  });

  // A popup that is a small panel rather than a list: the strip's
  // overflow, or a tool's quick help.
  function drawPanel(inner) {
    local.key = S.key;
    local.stack = [];
    local.hot = -1;
    keepFocus(() => { root.innerHTML = `<div class="mwrap"><div class="menu panel">${inner}</div><div class="more">${icon("chevron-down")}</div></div>`; });
    root.querySelector(".menu").addEventListener("scroll", showMore);
    local.sentH = local.sentW = null;
    reportSize();
    showMore();
  }

  // The clock ticks every second and the panel is sent again each time.
  // Drawn whole, its Work stage list closed under the pointer a moment
  // after it opened (MU100): only the parts that changed are replaced, and
  // the list and the note field are kept while they are in use.
  function drawTime(t) {
    const html = timeHtml(t);
    const old = local.key === S.key && root.querySelector(".menu.panel > .tp");
    if (old) {
      const tmp = document.createElement("div");
      tmp.innerHTML = html;
      const fresh = tmp.firstElementChild;
      if (fresh && fresh.children.length === old.children.length) {
        const kids = [...old.children], next = [...fresh.children];
        let grew = false;
        kids.forEach((a, i) => {
          const b = next[i];
          if (a.outerHTML === b.outerHTML) return;
          if (a.tagName !== b.tagName) { grew = true; }
          // the list or the field being used stays itself
          if (a === document.activeElement && (a.tagName === "SELECT" || a.tagName === "INPUT")) return;
          if (a.tagName === "SELECT" && a.dataset.open === "1") return;
          a.replaceWith(b);
        });
        if (grew) reportSize();
        return;
      }
    }
    drawPanel(html);
  }

  // The door and window picker (Mockup_Door_Window_Picker_REV00): your
  // types first, then every kind by family, as pictures; a family on the
  // left goes to its row; the search narrows the tiles as you type
  function drawKinds(k) {
    const t = (it, cls) => `<button class="kp-t ${it.on ? "on" : ""} ${cls || ""}" data-act="kind.pick:${esc(k.target)}:${esc(it.id)}" data-name="${esc((it.label + " " + (it.line || "")).toLowerCase())}" title="${esc(it.label)}">` +
      `${it.svg}<b>${esc(it.label)}</b>${it.line ? `<small>${esc(it.line)}</small>` : ""}</button>`;
    const all = k.families.reduce((n, f) => n + f.items.length, 0);
    const fams = `<button class="kp-f on" data-kpf="">All<i>${all}</i></button>` + (k.types.length ? `<button class="kp-f" data-kpf="types">Your types<i>${k.types.length}</i></button>` : "") +
      k.families.map((f, i) => `<button class="kp-f" data-kpf="f${i}">${esc(f.name)}<i>${f.items.length}</i></button>`).join("");
    let body = "";
    if (k.types.length) body += `<section data-kps="types"><h4>In this project</h4><div class="kp-grid">${k.types.map((it) => t(it, "type")).join("")}</div></section>`;
    k.families.forEach((f, i) => { body += `<section data-kps="f${i}"><h4>${esc(f.name)}</h4><div class="kp-grid">${f.items.map((it) => t(it)).join("")}</div></section>`; });
    const what = k.kind === "door" ? "door" : "window";
    const html = `<div class="kp"><div class="kp-top"><label class="kp-find">${icon("search")}<input data-kpfind placeholder="Find a ${what} kind or type" spellcheck="false"></label>` +
      `<span class="kp-title">${k.target === "sel" ? "Change the selected " + what + "s" : "The next " + what}</span></div>` +
      `<div class="kp-body"><div class="kp-fams">${fams}</div><div class="kp-tiles">${body}<div class="kp-none" hidden>Nothing matches. Try part of its name.</div></div></div>` +
      `<div class="kp-foot"><span>${esc(k.current || "")}</span><span>Click one to pick it · Esc to close</span></div></div>`;
    if (local.kpHtml === html && root.querySelector(".kp")) return;
    local.kpHtml = html;
    drawPanel(html);
    const find = root.querySelector("[data-kpfind]");
    const on = root.querySelector(".kp-t.on");
    if (on && on.scrollIntoView) on.scrollIntoView({ block: "nearest" });
    find.addEventListener("input", () => {
      const q = find.value.trim().toLowerCase();
      let any = false;
      root.querySelectorAll(".kp-tiles section").forEach((sec) => {
        let n = 0;
        sec.querySelectorAll(".kp-t").forEach((b) => { const hit = !q || b.dataset.name.includes(q); b.hidden = !hit; if (hit) n++; });
        sec.hidden = n === 0;
        any = any || n > 0;
      });
      root.querySelector(".kp-none").hidden = any;
    });
    find.addEventListener("keydown", (e) => {
      if (e.key === "Enter") { const b = [...root.querySelectorAll(".kp-t")].find((x) => !x.hidden && !x.closest("section").hidden); if (b) b.click(); e.preventDefault(); e.stopPropagation(); }
    });
  }
  document.addEventListener("click", (e) => {
    const f = S && S.kinds && e.target.closest && e.target.closest("[data-kpf]");
    if (!f) return;
    root.querySelectorAll(".kp-f").forEach((b) => b.classList.toggle("on", b === f));
    const box = root.querySelector(".kp-tiles");
    const sec = f.dataset.kpf ? root.querySelector(`[data-kps="${f.dataset.kpf}"]`) : null;
    if (box) box.scrollTop = sec ? sec.offsetTop : 0;
    e.stopPropagation();
  }, true);

  // Project time's panel over the pill: the clock, Pause, the stage, the
  // stage's budget and today's three counters.
  function timeHtml(t) {
    const go = t.manual && !t.running && t.clock === "00:00:00";
    const pause = t.running ? `${icon("pause")}<span>Pause</span>` : `${icon("play")}<span>${go ? "Start" : "Resume"}</span>`;
    // Note opens a field for what the counting entry is about; with no
    // entry counting (paused) there is nothing to write on
    const canNote = t.note !== null && t.note !== undefined;
    const noteOpen = canNote && (local.timeNote || t.note);
    // the pin keeps it in sight as a strip at the browser's foot (MU101)
    const pin = `<button class="tp-pin ${t.pinned ? "on" : ""}" data-act="time.pin" title="${t.pinned ? "Unpin: back to the pill in the status bar" : "Pin: keep it at the foot of the Project browser"}">${icon("pin")}</button>`;
    let h = `<div class="tp"><div class="tp-top"><span class="tp-h">${esc(t.project)} · today</span>${pin}</div>` +
      `<div class="tp-big">${esc(t.clock)}</div>` +
      `<div class="tp-row"><button class="btn pri" data-act="time.pause" data-keep="1">${pause}</button>` +
      `<button class="btn ${noteOpen ? "on" : ""}" data-tnote="1" ${canNote ? "" : "disabled"} title="${canNote ? "What you are working on, kept with this stretch of time" : "Resume the clock to write a note on what you are doing"}">${icon("sticky-note")}<span>Note</span></button></div>`;
    if (noteOpen) {
      h += `<input class="tp-note-in" data-key="time.note" data-text="1" placeholder="What you are working on" value="${esc(t.note)}">`;
    }
    // the stages as a list drawn in the card: a native list is a window of
    // its own, and the app's update each second shut it (MU100, MU101)
    const cur = t.stages.find((s) => s.on) || t.stages[0] || { label: "" };
    h += `<div class="tp-h">Work stage</div><button class="tp-sel ${local.stageOpen ? "open" : ""}" data-tstage-btn="1" aria-expanded="${!!local.stageOpen}"><span>${esc(cur.label)}</span>${icon(local.stageOpen ? "chevron-up" : "chevron-down")}</button>`;
    if (local.stageOpen) {
      h += `<div class="tp-list" role="listbox">` + t.stages.map((s, i) => `<button class="tp-opt ${s.on ? "on" : ""}" role="option" aria-selected="${s.on}" data-tstage-i="${i}" ${s.done && !s.on ? "disabled" : ""}>${s.on ? icon("check") : `<i></i>`}<span>${esc(s.label)}${s.done ? " · done" : ""}</span></button>`).join("") +
        // your own stages are made in the Time area (MU102)
        `<button class="tp-opt edit" data-act="time.view:stages">${icon("pen-tool")}<span>Edit the stages…</span></button></div>`;
    }
    if (t.stage_line) {
      h += `<div class="tp-h">${esc(t.stage_line)}</div>`;
      h += `<div class="tbar"><i class="${esc(t.budget)}" style="width:${t.use === null || t.use === undefined ? 0 : Math.max(2, t.use)}%"></i></div>`;
    }
    h += `<div class="tp-h">${esc(t.today)}</div>` +
      `<button class="btn tp-area" data-act="time.area">${icon("clock")}<span>Open the Time area</span></button></div>`;
    return h;
  }

  // The tool options strip's controls that don't fit there, one a row.
  function controlsHtml() {
    return S.controls.map((c) => {
      if (c.kind === "head") return `<div class="mhead">${esc(c.label)}</div>`;
      if (c.kind === "sep") return `<div class="msep"></div>`;
      return `<div class="prow">${control(c, true)}</div>`;
    }).join("");
  }

  // How to use a tool or an element, in a few lines, and its guide page.
  function helpHtml() {
    const h = S.help;
    return `<div class="hp"><div class="hp-title">${h.icon ? icon(h.icon) : icon("circle-help")}<b>${esc(h.title)}</b>` +
      (h.keys ? `<span class="hp-key">${esc(h.keys)}</span>` : "") + `</div>` +
      h.lines.map((l) => `<p>${inline(l)}</p>`).join("") +
      (h.page ? `<button class="hp-more" data-act="help:${esc(h.page)}">${icon("book-open")}<span>Read more in the guide</span>${icon("arrow-right")}</button>` : "") +
      `</div>`;
  }

  // Split a menu at its headers into `n` columns of about equal height,
  // keeping the sections in order.
  function columns(items, n) {
    const sections = [];
    for (const i of items) {
      if (i.header || sections.length === 0) sections.push([]);
      if (!i.sep) sections[sections.length - 1].push(i);
    }
    n = Math.min(n, sections.length);
    const h = sections.map((s) => s.length);
    let best = null;
    // every way to cut the sections into n runs (menus are short)
    (function cut(from, left, acc) {
      if (left === 1) {
        const runs = acc.concat([[from, sections.length]]);
        const tall = Math.max(...runs.map(([a, b]) => h.slice(a, b).reduce((x, y) => x + y, 0)));
        if (!best || tall < best.tall) best = { tall, runs };
        return;
      }
      for (let k = from + 1; k <= sections.length - left + 1; k++) cut(k, left - 1, acc.concat([[from, k]]));
    })(0, n, []);
    return best.runs.map(([a, b]) => [].concat(...sections.slice(a, b)));
  }

  // A shadow and an arrow at the bottom while more of the menu is below.
  function showMore() {
    const m = root.querySelector(".menu");
    const more = root.querySelector(".more");
    if (!m || !more) return;
    more.classList.toggle("on", m.scrollTop + m.clientHeight < m.scrollHeight - 2);
  }

  // the page's size changes as the app places it: measure again then
  window.addEventListener("resize", () => {
    if (pane !== "popup" || !root.querySelector(".menu")) return;
    reportSize();
    showMore();
  });

  // The size the menu wants: its content's own width and height, measured
  // apart from the page it is shown in. (Measuring it as placed fed the
  // page's width back into the menu's, so a menu with a scrollbar grew
  // wider every time it was placed.)
  function naturalSize(m) {
    const keep = [m.style.minWidth, m.style.width, m.style.maxHeight, m.style.overflowY];
    m.style.minWidth = "0";
    m.style.width = "max-content";
    m.style.maxHeight = "none";
    m.style.overflowY = "hidden";
    const r = m.getBoundingClientRect();
    [m.style.minWidth, m.style.width, m.style.maxHeight, m.style.overflowY] = keep;
    return { w: Math.ceil(r.width), h: Math.ceil(r.height) };
  }

  function reportSize() {
    const m = root.querySelector(".menu");
    if (!m) return;
    const { w, h } = naturalSize(m);
    // taller than the page it was given: it scrolls, and the scrollbar
    // needs room beside the items
    const bar = h > window.innerHeight - 2 + 1 ? 10 : 0;
    // a pixel to spare each way: the panel's last device pixel was lost
    // at some display scales, and with it the menu's right and bottom
    // edge (MU102)
    // (no shadow margin round it: the panel is solid and the menu fills
    // it, #112 #121)
    const want = { w: w + 3 + bar, h: h + 3 };
    if (want.w === local.sentW && want.h === local.sentH) return;
    local.sentW = want.w;
    local.sentH = want.h;
    post({ t: "size", w: want.w, h: want.h });
  }

  // Arrow keys, Enter and Esc in an open menu, from this page or passed on
  // from the views.
  function menuKey(k) {
    const buttons = [...root.querySelectorAll(".menu .mi:not(:disabled), .menu .head")];
    if (buttons.length === 0) return;
    if (k === "ArrowDown" || k === "ArrowUp") {
      const n = buttons.length;
      local.hot = local.hot < 0 ? (k === "ArrowDown" ? 0 : n - 1) : (local.hot + (k === "ArrowDown" ? 1 : -1) + n) % n;
      buttons.forEach((b, i) => b.classList.toggle("hot", i === local.hot));
      buttons[local.hot].scrollIntoView({ block: "nearest" });
      showMore();
    } else if (k === "Enter" || k === "ArrowRight") {
      const b = buttons[local.hot];
      if (b && (k === "Enter" || b.dataset.sub)) b.click();
    } else if (k === "ArrowLeft" || k === "Backspace") {
      if (local.stack.length) { local.stack.pop(); local.hot = -1; drawPopup(); }
    } else if (k === "Escape") {
      post({ t: "close" });
    }
  }

  // ---- the Help panel: the guide's pages, written in Markdown ----

  // Every page, from the app: {section, id, title, md}. The shortcuts page
  // is made by the app from its menus and arrives with the state.
  function helpPages() {
    const gen = (S && S.generated) || {};
    return (window.HELP || []).map((p) => gen[p.id] ? Object.assign({}, p, { md: gen[p.id] }) : p);
  }

  // A heading's id, as the app's help::slug makes it.
  function slug(s) {
    let out = "";
    for (const c of s.toLowerCase()) {
      if (/[\p{L}\p{N}]/u.test(c)) out += c;
      else if (out && !out.endsWith("-")) out += "-";
    }
    return out.replace(/-+$/, "");
  }

  function mdLink(text, href) {
    // a website opens in the system's browser
    if (/^https?:/.test(href)) return `<a class="ext" data-act="url:${href}" title="${href}">${text}</a>`;
    return `<a data-act="help:${href}">${text}</a>`;
  }

  function mdImage(alt, src) {
    const url = S && S.images && S.images[src];
    // About: the mark drawn as a pen sketch, then filled, the word fading
    // in beside it (Mark Moments REV02, C with A's word); once, as the page
    // opens. The word is the lockup's own, in the theme's logo colour.
    if (src === "img/creabim-lockup.png" && url) {
      return `<div class="about-lock" role="img" aria-label="CreaBIM">${markSvg(local.aboutPlay ? "aCA" : "still", 64)}` +
        `<span class="about-word" style="--word:url('${url}')"></span></div><div class="about-ver">${alt}</div>`;
    }
    // click a picture to see it as wide as the panel lets it, again to go back
    return url ? `<figure class="zoomable" title="Click to enlarge"><img src="${url}" alt="${alt}"><figcaption>${alt}</figcaption></figure>` : "";
  }

  // Inline marks, on text already escaped.
  function inline(s) {
    let out = esc(s);
    out = out.replace(/`([^`]+)`/g, (m, c) => `<kbd>${c}</kbd>`);
    out = out.replace(/!\[([^\]]*)\]\(([^)]+)\)/g, (m, alt, src) => mdImage(alt, src));
    out = out.replace(/\[([^\]]+)\]\(([^)]+)\)/g, (m, t, href) => mdLink(t, href));
    out = out.replace(/\*\*([^*]+)\*\*/g, "<b>$1</b>");
    out = out.replace(/\*([^*]+)\*/g, "<i>$1</i>");
    return out;
  }

  const LI = /^(\s*)([-*]|\d+\.)\s+(.*)$/;

  // A list from line i, nested by indent. Returns [html, next line].
  function mdList(lines, i) {
    const first = lines[i].match(LI);
    const indent = first[1].length;
    const tag = /\d/.test(first[2]) ? "ol" : "ul";
    let html = `<${tag}>`;
    let item = null;
    const close = () => { if (item) html += `<li>${inline(item.text)}${item.sub}</li>`; item = null; };
    while (i < lines.length) {
      const l = lines[i];
      if (/^\s*$/.test(l)) {
        const next = lines[i + 1] && lines[i + 1].match(LI);
        if (next && next[1].length >= indent) { i++; continue; }
        break;
      }
      const m = l.match(LI);
      if (m) {
        if (m[1].length < indent) break;
        if (m[1].length > indent && item) {
          const [sub, j] = mdList(lines, i);
          item.sub += sub;
          i = j;
          continue;
        }
        close();
        item = { text: m[3], sub: "" };
        i++;
        continue;
      }
      if (item && /^\s+\S/.test(l)) { item.text += " " + l.trim(); i++; continue; }
      break;
    }
    close();
    return [html + `</${tag}>`, i];
  }

  function cells(l) {
    return l.trim().replace(/^\|/, "").replace(/\|$/, "").split("|").map((c) => c.trim());
  }

  function markdown(text) {
    const lines = text.replace(/\r/g, "").split("\n");
    let html = "";
    let para = [];
    const flush = () => { if (para.length) html += `<p>${inline(para.join(" "))}</p>`; para = []; };
    let i = 0;
    while (i < lines.length) {
      const l = lines[i];
      let m;
      if (/^\s*$/.test(l)) { flush(); i++; continue; }
      if (/^<!--\s*upcoming\b.*-->\s*$/.test(l)) { flush(); html += `<div class="soon"><div class="soon-tag">Coming soon</div>`; i++; continue; }
      if (/^<!--\s*\/upcoming\s*-->\s*$/.test(l)) { flush(); html += `</div>`; i++; continue; }
      if (/^<!--.*-->\s*$/.test(l)) { i++; continue; }
      if ((m = l.match(/^(#{1,4})\s+(.*)$/))) {
        flush();
        const n = m[1].length;
        html += `<h${n} id="h-${slug(m[2])}">${inline(m[2])}</h${n}>`;
        i++;
        continue;
      }
      if (/^!\[[^\]]*\]\([^)]+\)\s*$/.test(l)) { flush(); html += inline(l.trim()); i++; continue; }
      if (/^>/.test(l)) {
        flush();
        const inner = [];
        while (i < lines.length && /^>/.test(lines[i])) inner.push(lines[i++].replace(/^>\s?/, ""));
        html += `<div class="tip">${markdown(inner.join("\n"))}</div>`;
        continue;
      }
      if (LI.test(l)) { flush(); const [h, j] = mdList(lines, i); html += h; i = j; continue; }
      if (/^\|/.test(l)) {
        flush();
        const rows = [];
        while (i < lines.length && /^\|/.test(lines[i])) rows.push(lines[i++]);
        const head = rows.length > 1 && /^[\s|:-]+$/.test(rows[1]) ? cells(rows[0]) : null;
        const body = head ? rows.slice(2) : rows;
        html += `<table>` + (head ? `<thead><tr>${head.map((c) => `<th>${inline(c)}</th>`).join("")}</tr></thead>` : "") +
          `<tbody>${body.map((r) => `<tr>${cells(r).map((c) => `<td>${inline(c)}</td>`).join("")}</tr>`).join("")}</tbody></table>`;
        continue;
      }
      para.push(l.trim());
      i++;
    }
    flush();
    return html;
  }

  // A page's words without the marks (or its title), for searching.
  function plain(md) {
    return md.replace(/^# .*$/m, " ").replace(/<!--.*?-->/g, " ").replace(/!\[[^\]]*\]\([^)]*\)/g, " ")
      .replace(/\[([^\]]+)\]\([^)]*\)/g, "$1").replace(/[#*`>|]/g, " ").replace(/\s+/g, " ").trim();
  }

  function search(q) {
    const words = q.toLowerCase().split(/\s+/).filter(Boolean);
    const out = [];
    for (const p of helpPages()) {
      const text = plain(p.md);
      const low = text.toLowerCase();
      const title = p.title.toLowerCase();
      if (!words.every((w) => low.includes(w) || title.includes(w))) continue;
      let score = 0;
      for (const w of words) {
        if (title.includes(w)) score += 10;
        score += Math.min(5, low.split(w).length - 1);
      }
      // a few words round the first hit
      const at = Math.max(0, low.indexOf(words[0]));
      const from = Math.max(0, at - 50);
      let snip = esc((from ? "…" : "") + text.slice(from, at + 110) + "…");
      for (const w of words) snip = snip.replace(new RegExp(w.replace(/[.*+?^${}()|[\]\\]/g, "\\$&"), "gi"), (m) => `<mark>${m}</mark>`);
      out.push({ p, score, snip });
    }
    return out.sort((a, b) => b.score - a.score);
  }

  function drawHelp() {
    // a page opened from elsewhere (a menu, F1) ends the search
    const nav = S.page + "#" + (S.anchor || "");
    if (local.hnav !== undefined && local.hnav !== nav) local.hq = "";
    local.hnav = nav;
    const pages = helpPages();
    const q = local.hq || "";
    const page = pages.find((p) => p.id === S.page);
    let body = "";
    if (q.trim()) {
      const found = search(q);
      local.hfound = found;
      body = (found.length ? `<div class="hres-n">${found.length + (found.length === 1 ? " page" : " pages")} for “${esc(q)}”</div>` :
        emptyState(`Nothing in the guide about “${esc(q)}”`, "Try a shorter word, or a tool’s name.")) +
        found.map((f) => `<button class="hres" data-act="help:${esc(f.p.id)}"><b>${esc(f.p.title)}</b><span class="hsec">${esc(f.p.section)}</span><span class="snip">${f.snip}</span></button>`).join("");
    } else if (!page) {
      // the contents: every page by section
      body = `<article class="hdoc"><h1>Help</h1><p>Pick a page, or search the guide above. <kbd>F1</kbd> opens the page about the tool you are using.</p>`;
      let section = null;
      for (const p of pages) {
        if (p.section !== section) {
          if (section !== null) body += `</div>`;
          section = p.section;
          body += `<h3>${esc(section)}</h3><div class="toc">`;
        }
        body += `<a data-act="help:${esc(p.id)}">${esc(p.title)}</a>`;
      }
      body += `</div></article>`;
    } else {
      const i = pages.indexOf(page);
      const prev = pages[i - 1], next = pages[i + 1];
      // the woven mark over each page, woven once as the page opens
      const opened = local.hwoven !== page.id;
      local.hwoven = page.id;
      // (About has its own lockup instead)
      local.aboutPlay = opened && page.id === "about";
      const mark = page.id === "about" ? "" : `<div class="help-mark logo">${woven("guide", opened)}</div>`;
      body = `${mark}<div class="crumb"><a data-act="help:contents">Contents</a> › ${esc(page.section)}</div><article class="hdoc">${foldSections(markdown(page.md), page.id)}</article>` +
        // two equal cards, a small label over the page's title (MU30)
        `<div class="pn">${prev ? `<a class="pn-card" data-act="help:${esc(prev.id)}"><small>← Previous</small><span>${esc(prev.title)}</span></a>` : "<span></span>"}` +
        `${next ? `<a class="pn-card next" data-act="help:${esc(next.id)}"><small>Next →</small><span>${esc(next.title)}</span></a>` : "<span></span>"}</div>`;
    }
    const where = (q.trim() ? "?" : "") + S.page + "#" + (S.anchor || "");
    const scroller = root.querySelector(".help-body");
    const keep = scroller && local.hwhere === where ? scroller.scrollTop : null;
    root.innerHTML = `<div class="help"><div class="help-head">` +
      `<button class="ibtn" data-act="help.back" title="Back" ${S.back ? "" : "disabled"}>${icon("arrow-left")}</button>` +
      `<button class="ibtn ${page || q ? "" : "on"}" data-act="help:contents" title="Contents">${icon("book-open")}</button>` +
      `<label class="search">${icon("search")}<input data-key="helpq" data-live="1" placeholder="Search the guide" value="${esc(q)}"></label>` +
      `<button class="ibtn" data-act="help.close" title="Close (F1 or Esc)">${icon("x")}</button></div>` +
      `<div class="help-body">${body}</div></div>`;
    const b = root.querySelector(".help-body");
    if (keep !== null) b.scrollTop = keep;
    else {
      const h = S.anchor && !q.trim() && root.querySelector(`#h-${CSS.escape(S.anchor)}`);
      const fold = h && h.closest("details");
      if (fold) fold.open = true;
      if (h) h.scrollIntoView({ block: "start" });
      else b.scrollTop = 0;
    }
    local.hwhere = where;
  }

  // ---- drawing and events ----

  // the tip's own panel (MU102): the tip laid out, its size told
  function drawTip() {
    const text = (S && S.tip) || "";
    root.innerHTML = text ? `<div class="webtip tipwin"></div>` : "";
    const d = root.firstElementChild;
    if (!d) return;
    d.textContent = text;
    const r = d.getBoundingClientRect();
    post({ t: "size", text, w: Math.ceil(r.width) + 1, h: Math.ceil(r.height) + 1 });
  }
  const draw = { top: drawTop, left: drawLeft, status: drawStatus, popup: drawPopup, nav: drawNav, help: drawHelp, tip: drawTip }[pane];

  // Keys typed in the views while the palette is open.
  function paletteKey(k) {
    const found = local.found || [];
    if (k === "ArrowDown" || k === "ArrowUp") {
      local.hot = (local.hot + (k === "ArrowDown" ? 1 : -1) + found.length) % Math.max(found.length, 1);
    } else if (k === "Enter") {
      const i = found[local.hot];
      post(i ? { t: "act", id: i.id, keep: false } : { t: "close" });
      return false;
    } else if (k === "Escape") {
      post({ t: "close" });
      return false;
    } else if (k === "Backspace") {
      local.query = (local.query || "").slice(0, -1);
      local.hot = 0;
    }
    return true;
  }

  window.CB = {
    state(s) {
      // profile pictures for the lists, drawn as icons (MU37)
      if (s.icons) Object.assign(window.ICONS, s.icons);
      S = s;
      // the splash's shade over this band too (MU79)
      document.documentElement.classList.toggle("cb-dim", !!s.dim);
      if (s.theme) document.documentElement.dataset.theme = s.theme;
      // which side the buttons sit in CreaBIM's own windows
      if (s.dir) document.documentElement.dataset.dir = s.dir;
      // the bands are drawn again as their state comes (the status bar's
      // clock, every second); one drawn between a press and its release
      // swapped the button under the pointer and the click was lost (the
      // clock pill "stuck", MU101): wait for the release
      if (local.pressing && performance.now() - local.pressAt < 1500 && (pane === "status" || pane === "top")) { local.pending = true; return; }
      keepFocus(draw);
      if (pane === "top") setTimeout(openKindsOnce, 0);
    },
    nav(k) {
      if (S && S.key !== "palette") menuKey(k);
    },
    feed(f) {
      if (!S || S.key !== "palette") return;
      if (f.text) { local.query = (local.query || "") + f.text; local.hot = 0; }
      for (const k of f.keys) if (!paletteKey(k)) return;
      drawPalette();
    },
  };

  function focusViews(e) {
    // Give the keyboard back to the views unless a text box was clicked.
    const t = e.target;
    if (!(t instanceof HTMLInputElement)) post({ t: "focus" });
  }

  document.addEventListener("mousedown", () => { local.pressing = true; local.pressAt = performance.now(); local.downPopup = S ? S.popup : null; }, true);
  document.addEventListener("mouseup", () => {
    local.pressing = false;
    // after the click has gone to the button it was pressed on
    if (local.pending) setTimeout(() => { if (!local.pressing && local.pending) { local.pending = false; keepFocus(draw); } }, 0);
  }, true);
  document.addEventListener("mousedown", (e) => {
    post({ t: "down" });
    const g = e.target.closest("[data-scrub]");
    if (g) startScrub(e, g);
    const row = e.button === 0 && e.target.closest(".row[data-sort]");
    if (row) {
      local.drag = { id: row.dataset.id, group: row.dataset.sort, y: e.clientY, on: false };
      document.addEventListener("mousemove", dragMove);
      document.addEventListener("mouseup", dragEnd);
    }
  });

  document.addEventListener("click", (e) => {
    if (local.dragged) return;
    const fig = e.target.closest("figure.zoomable");
    if (fig) { fig.classList.toggle("big"); return; }
    // Project time's Work stage: the list opens in the card; a pick goes
    const tsb = S && S.time && e.target.closest("[data-tstage-btn]");
    if (tsb) { local.stageOpen = !local.stageOpen; drawPanel(timeHtml(S.time)); return; }
    const tso = S && S.time && e.target.closest("[data-tstage-i]");
    if (tso) {
      if (!tso.disabled) post({ t: "act", id: `time.stage:${tso.dataset.tstageI}`, keep: true });
      local.stageOpen = false;
      drawPanel(timeHtml(S.time));
      return;
    }
    // Project time's Note: the field for the entry counting now
    const tn = S && S.time && e.target.closest("[data-tnote]");
    if (tn) {
      if (!tn.disabled) {
        local.timeNote = !local.timeNote;
        drawPanel(timeHtml(S.time));
        const f = root.querySelector(".tp-note-in");
        if (f) f.focus();
      }
      return;
    }
    const rg = e.target.closest("[data-railgroup]");
    if (rg) local.open[rg.dataset.railgroup] = true;
    const fold = e.target.closest("[data-fold]");
    if (fold && S && S.groups) {
      for (const g of S.groups) local.open[g.key] = fold.dataset.fold === "1";
      drawLeft();
      return focusViews(e);
    }
    const t = e.target.closest("button, .row, .group, a[data-act]");
    if (!t || t.disabled) return focusViews(e);
    if (t.dataset.popup) {
      const r = t.getBoundingClientRect();
      // the press closed it already (the app shuts a menu on any press
      // outside it): what was open then decides, or the click opened it
      // again and a second click never shut it (MU101)
      const was = local.downPopup !== undefined ? local.downPopup : S.popup;
      if (was === t.dataset.popup) post({ t: "close" });
      // Project time's card stands clear of the status bar ("Space A")
      else {
        // a menu stands a little off its button, so its edge reads (MU102)
        const gap = t.dataset.popup === "time" ? 10 : 8;
        // (the panel starts 8 px left and 6 px above the menu it holds,
        // for its shadow; a menu above a control has 10 px below it)
        const sh = t.dataset.popup === "time" ? [0, 0, 0] : [8, 6, 10];
        post({ t: "popup", key: t.dataset.popup, x: r.left - sh[0], y: r.bottom + 6 - sh[1], w: r.width, up: pane === "status" ? r.top - gap + sh[2] : null, top: r.top - gap + sh[2] });
      }
    } else if (t.dataset.back) {
      local.stack.pop();
      local.hot = -1;
      drawPopup();
      return;
    } else if (t.dataset.sub) {
      local.stack.push(t.dataset.sub);
      local.hot = -1;
      drawPopup();
      root.querySelector(".menu").scrollTop = 0;
      return;
    } else if (t.dataset.act) {
      // a page picked from the search results ends the search
      if (pane === "help" && t.dataset.act.startsWith("help:") && local.hq) {
        local.hq = "";
        drawHelp();
      }
      post({ t: "act", id: t.dataset.act, keep: t.dataset.keep === "1" });
    } else if (t.dataset.group) {
      const g = S.groups.find((x) => x.key === t.dataset.group);
      const was = local.open[g.key] !== undefined ? local.open[g.key] : g.open;
      local.open[g.key] = !was;
      // the group clicked folds smoothly (AMD-017); the others redraw still
      local.moving = { key: g.key, open: !was, at: performance.now() };
      drawLeft();
      setTimeout(drawLeft, 230);
    } else if (t.dataset.id) {
      post({ t: "pick", id: t.dataset.id, add: e.shiftKey || e.ctrlKey || e.metaKey });
    }
    focusViews(e);
  });

  // Double-click a row of the project browser: a type opens on its own,
  // an element is framed in the 3D view.
  document.addEventListener("dblclick", (e) => {
    const row = e.target.closest(".row");
    if (row && row.dataset.id && !local.dragged) post({ t: "open", id: row.dataset.id });
  });

  // Slide between menus while one is open, like a menu bar.
  document.addEventListener("mouseover", (e) => {
    const t = e.target.closest("[data-menubar]");
    if (!t || !S || !S.popup || !S.popup.startsWith("menu:") || S.popup === t.dataset.popup) return;
    const r = t.getBoundingClientRect();
    post({ t: "popup", key: t.dataset.popup, x: r.left, y: r.bottom + 4, w: r.width });
  });

  document.addEventListener("input", (e) => {
    const t = e.target;
    if (t.dataset.key === "filter") {
      local.filter = t.value;
      drawLeft();
    } else if (t.dataset.key === "palette") {
      local.query = t.value;
      local.hot = 0;
      drawPalette();
    } else if (t.dataset.key === "helpq") {
      local.hq = t.value;
      keepFocus(drawHelp);
    }
  });

  document.addEventListener("mousedown", (e) => {
    const sel = e.target.closest && e.target.closest("select[data-tstage]");
    if (sel) sel.dataset.open = "1";
  }, true);
  document.addEventListener("focusout", (e) => {
    if (e.target.dataset && e.target.dataset.tstage === "1") delete e.target.dataset.open;
  }, true);

  document.addEventListener("change", (e) => {
    const t = e.target;
    if (t.dataset.tstage === "1") {
      delete t.dataset.open;
      post({ t: "act", id: `time.stage:${t.value}`, keep: true });
    } else if (t.dataset.text === "1") {
      post({ t: "set", key: t.dataset.key, text: t.value });
    } else if (t.dataset.key && !["filter", "palette", "helpq"].includes(t.dataset.key)) {
      const v = parseFloat(t.value.replace(",", "."));
      if (!isNaN(v)) setNumber(t, v);
    }
  });

  document.addEventListener("keydown", (e) => {
    const t = e.target;
    if (t instanceof HTMLInputElement && t.dataset.key === "palette") {
      if (e.key === "ArrowDown" || e.key === "ArrowUp") {
        e.preventDefault();
        paletteKey(e.key);
        drawPalette();
      } else if (e.key === "Enter" || e.key === "Escape") {
        paletteKey(e.key);
        post({ t: "focus" });
      }
      return;
    }
    if (t instanceof HTMLInputElement && t.dataset.key === "helpq") {
      // Esc clears the search (and then closes), Enter opens the best page
      if (e.key === "Escape") {
        if (t.value) { local.hq = ""; keepFocus(drawHelp); } else post({ t: "act", id: "help.close" });
      } else if (e.key === "Enter" && local.hfound && local.hfound.length) {
        const id = local.hfound[0].p.id;
        local.hq = "";
        post({ t: "act", id: "help:" + id });
      } else if (e.key === "F1") {
        post({ t: "act", id: "help.close" });
      }
      return;
    }
    if (pane === "help" && e.key === "Escape") {
      post({ t: "act", id: "help.close" });
      return;
    }
    if (t instanceof HTMLInputElement && t.dataset.menuinput) {
      if (e.key === "Enter") {
        const n = parseInt(t.value, 10);
        if (n >= 2 && n <= 20) { post({ t: "act", id: `${t.dataset.menuinput}:${n}` }); }
        else { t.select(); }
      } else if (e.key === "Escape") post({ t: "close" });
      return;
    }
    if (t instanceof HTMLInputElement) {
      if (e.key === "Enter") { t.blur(); post({ t: "focus" }); }
      else if (e.key === "Escape") { t.value = t.defaultValue; t.blur(); post({ t: "focus" }); }
      else if ((e.key === "ArrowUp" || e.key === "ArrowDown") && t.dataset.step) {
        e.preventDefault();
        const step = parseFloat(t.dataset.step) * (e.shiftKey ? 10 : 1);
        setNumber(t, value(t) + (e.key === "ArrowUp" ? step : -step));
      }
      return;
    }
    if (pane === "popup" && dialogKey(e)) return;
    if (pane === "popup" && e.key === "Escape" && S && S.help) { post({ t: "close" }); return; }
    // a menu walks with them only while it shows: the keyboard can stay in
    // this pane after its menu is put away, and Enter and the arrows then
    // never reached the views' text boxes (MU87)
    const menuShown = root.querySelector(".menu .mi, .menu .head") !== null;
    if (pane === "popup" && menuShown && ["ArrowDown", "ArrowUp", "ArrowLeft", "ArrowRight", "Enter", "Escape"].includes(e.key)) {
      e.preventDefault();
      menuKey(e.key);
      return;
    }
    // Anything else is a shortcut for the views.
    post({ t: "key", key: e.key, code: e.code, ctrl: e.ctrlKey || e.metaKey, shift: e.shiftKey, alt: e.altKey, repeat: e.repeat });
    if (e.ctrlKey || e.metaKey || e.key === "F1" || e.key === "F3" || e.key === "Tab") e.preventDefault();
  });

  // A number field's value: lengths keep theirs (metres) beside the text.
  function value(input) {
    return input.dataset.text === "1" ? parseFloat(input.dataset.v) : parseFloat(input.value);
  }

  // Drag a number's label sideways to change it.
  function startScrub(e, label) {
    const input = label.parentElement.querySelector("input");
    const step = parseFloat(label.dataset.step);
    const x0 = e.clientX;
    const v0 = value(input);
    let moved = false;
    e.preventDefault();
    function move(ev) {
      const dx = ev.clientX - x0;
      if (Math.abs(dx) > 2) moved = true;
      if (moved) setNumber(input, v0 + Math.round(dx / 4) * step * (ev.shiftKey ? 10 : 1));
    }
    function up() {
      document.removeEventListener("mousemove", move);
      document.removeEventListener("mouseup", up);
      if (!moved) { input.focus(); input.select(); }
    }
    document.addEventListener("mousemove", move);
    document.addEventListener("mouseup", up);
  }

  // Hints shown by the app in its theme (MU26): the system's black
  // tooltip is kept from showing, the text goes to the app instead.
  (function () {
    let timer = null, shown = null, own = null;
    function hide() {
      clearTimeout(timer);
      timer = null;
      if (own) { own.remove(); own = null; }
      if (shown) { shown = null; post({ t: "tip", text: "" }); }
    }
    // The tip right under its control, drawn here, when it fits inside
    // this panel (MU103: the app can't draw over a web panel, so a top
    // row's tip showed under the whole bar, far from its button).
    function inside(el, text, r) {
      const d = document.createElement("div");
      d.className = "webtip";
      d.textContent = text;
      document.body.appendChild(d);
      const w = d.offsetWidth, h = d.offsetHeight;
      const x = Math.max(4, Math.min(r.left, window.innerWidth - w - 4));
      const y = r.bottom + 6;
      if (y + h > window.innerHeight - 2 || w > window.innerWidth - 8) { d.remove(); return false; }
      d.style.left = x + "px";
      d.style.top = y + "px";
      d.style.visibility = "visible";
      own = d;
      return true;
    }
    document.addEventListener("mouseover", (e) => {
      const el = e.target.closest && e.target.closest("[title], [data-tip]");
      if (!el) { hide(); return; }
      if (el.hasAttribute("title")) {
        el.dataset.tip = el.getAttribute("title");
        el.removeAttribute("title");
      }
      if (el === shown) return;
      hide();
      const text = el.dataset.tip || "";
      if (!text) return;
      timer = setTimeout(() => {
        const r = el.getBoundingClientRect();
        shown = el;
        if (inside(el, text, r)) return;
        post({ t: "tip", text, x: r.left, y: r.top, w: r.width, h: r.height });
      }, 450);
    });
    document.addEventListener("mouseleave", hide);
    // the app's word that the pointer is on the drawing area: this page
    // never hears it leave
    window.CB.away = () => { clearTimeout(timer); timer = null; shown = null; };
    document.addEventListener("mousedown", hide, true);
    document.addEventListener("wheel", hide, { passive: true });
  })();

  document.addEventListener("contextmenu", (e) => e.preventDefault());
  post({ t: "ready" });
})();
