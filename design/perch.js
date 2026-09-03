/* ============================================================
   Perch: shared behaviour for the mockups.
   Keyboard-first: j/k move, Enter opens, x dismisses, ⌘K palette.
   The palette vocabulary is the CLI vocabulary, verbatim.
   ============================================================ */

const PERCH_COMMANDS = [
  { group: "Watching", cmd: "watch add <company>",     desc: "detect ATS, start monitoring" },
  { group: "Watching", cmd: "watch list",              desc: "companies and their boards" },
  { group: "Watching", cmd: "watch rm <company>",      desc: "stop monitoring" },
  { group: "Watching", cmd: "sync",                    desc: "poll every watched board now" },

  { group: "Feed",     cmd: "feed",                    desc: "matched roles, newest first" },
  { group: "Feed",     cmd: "feed --fresh",            desc: "posted in the last 24 hours" },
  { group: "Feed",     cmd: "feed --company <name>",   desc: "one company only" },
  { group: "Feed",     cmd: "feed --all",              desc: "every open role, matched or not" },
  { group: "Feed",     cmd: "open <role>",             desc: "detail pane" },
  { group: "Feed",     cmd: "dismiss <role>",          desc: "remove from the queue" },

  { group: "Applying",  cmd: "apply <role>",           desc: "review, attach, open in browser" },
  { group: "Applying",  cmd: "apps",                   desc: "in flight, responded, archived" },
  { group: "Applying",  cmd: "apps mark <role> <state>", desc: "record a reply" },

  { group: "Profile",  cmd: "profile edit",            desc: "open profile.toml" },
  { group: "Profile",  cmd: "profile import <file>",   desc: "propose fields from a résumé" },
  { group: "Profile",  cmd: "docs list",               desc: "résumé and letter variants" },

  { group: "Rules",    cmd: "rules edit",              desc: "match rules, plain text" },
  { group: "Rules",    cmd: "rules list",              desc: "the rules as Perch reads them" },
  { group: "Rules",    cmd: "rules test <role>",       desc: "which rule fires, and why" },

  { group: "Model",    cmd: "model list",              desc: "endpoints Perch can reach" },
  { group: "Model",    cmd: "model set <name>",        desc: "choose extraction model" },
  { group: "Model",    cmd: "model off",               desc: "run with no inference" },
];

function perchPalette() {
  if (document.querySelector(".palette-scrim")) return;
  const scrim = document.createElement("div");
  scrim.className = "palette-scrim";
  scrim.innerHTML = `
    <div class="palette" role="dialog" aria-label="Command palette">
      <input class="palette-input" placeholder="Type a command…" autocomplete="off" spellcheck="false">
      <div class="palette-list"></div>
      <div class="palette-foot">
        <span><span class="kbd">↑</span> <span class="kbd">↓</span> move</span>
        <span><span class="kbd">↵</span> run</span>
        <span><span class="kbd">esc</span> close</span>
        <span style="margin-left:auto">same words as the CLI</span>
      </div>
    </div>`;
  document.body.appendChild(scrim);

  const input = scrim.querySelector(".palette-input");
  const list = scrim.querySelector(".palette-list");
  let active = 0, shown = [];

  const esc = t => t.replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;");

  function render(q) {
    const needle = q.trim().toLowerCase();
    shown = PERCH_COMMANDS.filter(c =>
      !needle || c.cmd.toLowerCase().includes(needle) || c.desc.toLowerCase().includes(needle));
    if (active >= shown.length) active = Math.max(0, shown.length - 1);
    let html = "", lastGroup = null;
    shown.forEach((c, i) => {
      if (c.group !== lastGroup) { html += `<div class="palette-group">${esc(c.group)}</div>`; lastGroup = c.group; }
      html += `<div class="palette-item ${i === active ? "is-active" : ""}" data-i="${i}">
                 <span class="cmd">${esc(c.cmd)}</span><span class="desc">${esc(c.desc)}</span></div>`;
    });
    if (!shown.length) html = `<div class="palette-group">no command by that name</div>`;
    list.innerHTML = html;
    const el = list.querySelector(".is-active");
    if (el) el.scrollIntoView({ block: "nearest" });
  }

  function open()  { scrim.classList.add("is-open"); input.value = ""; active = 0; render(""); input.focus(); }
  /* Blur on close, or focus stays in the palette input and j/k stall silently. */
  function close() { scrim.classList.remove("is-open"); input.blur(); document.body.focus(); }

  input.addEventListener("input", () => { active = 0; render(input.value); });
  list.addEventListener("mousemove", e => {
    const it = e.target.closest(".palette-item"); if (!it) return;
    active = +it.dataset.i; render(input.value);
  });
  scrim.addEventListener("mousedown", e => { if (e.target === scrim) close(); });

  input.addEventListener("keydown", e => {
    if (e.key === "ArrowDown") { e.preventDefault(); active = Math.min(active + 1, shown.length - 1); render(input.value); }
    if (e.key === "ArrowUp")   { e.preventDefault(); active = Math.max(active - 1, 0); render(input.value); }
    if (e.key === "Escape")    { close(); }
    if (e.key === "Enter")     { close(); }
  });

  window.perchOpenPalette = open;
  window.perchClosePalette = close;

  document.addEventListener("keydown", e => {
    if ((e.metaKey || e.ctrlKey) && e.key.toLowerCase() === "k") { e.preventDefault(); open(); }
    else if (e.key === "Escape" && scrim.classList.contains("is-open")) close();
  });
}

/* j / k / Enter over any [data-queue] of [data-row] elements.
   x sets a row aside, but only on a queue marked [data-clearable="<verb>"],
   and it never destroys: the row collapses in place and u puts it back.
   Perch does not delete things, so its keyboard cannot either. Screens that
   hold nothing clearable omit the attribute. No page has to defend
   itself against a shared key. */
function perchQueue(opts = {}) {
  const queues = [...document.querySelectorAll("[data-queue]")];
  if (!queues.length) return;
  const rows = () => queues.flatMap(q => [...q.querySelectorAll("[data-row]")])
                           .filter(r => r.dataset.setAside !== "1");
  const undos = [];

  function select(el) {
    if (!el) return;
    queues.flatMap(q => [...q.querySelectorAll("[data-row]")])
          .forEach(r => r.classList.remove("is-selected"));
    el.classList.add("is-selected");
    el.scrollIntoView({ block: "nearest" });
    if (opts.onSelect) opts.onSelect(el);
  }
  function current() { return document.querySelector("[data-row].is-selected"); }
  function move(d) {
    const r = rows(); if (!r.length) return;
    const i = r.indexOf(current());
    select(r[Math.min(Math.max((i < 0 ? -1 : i) + d, 0), r.length - 1)]);
  }

  /* Collapse in place and hand back the function that restores it. */
  function collapse(el) {
    const h = el.offsetHeight;
    el.style.transition = "opacity .18s ease, max-height .22s ease, padding .22s ease";
    el.style.overflow = "hidden";
    el.style.maxHeight = h + "px";
    requestAnimationFrame(() => {
      el.style.opacity = "0"; el.style.maxHeight = "0px";
      el.style.paddingTop = "0"; el.style.paddingBottom = "0";
    });
    return () => {
      el.style.opacity = ""; el.style.maxHeight = h + "px";
      el.style.paddingTop = ""; el.style.paddingBottom = "";
      setTimeout(() => { el.style.maxHeight = ""; el.style.overflow = ""; }, 240);
    };
  }

  function note(text) {
    const bar = document.querySelector(".hint-bar"); if (!bar) return;
    let n = bar.querySelector(".undo-note");
    if (!n) {
      n = document.createElement("span");
      n.className = "undo-note";
      bar.insertBefore(n, bar.querySelector(".spacer") || null);
    }
    n.innerHTML = `${text}. <span class="kbd">u</span> to undo`;
    n.classList.add("is-on");
    clearTimeout(note._t);
    note._t = setTimeout(() => n.classList.remove("is-on"), 6000);
  }

  function setAside(el) {
    const q = el.closest("[data-queue]");
    if (!q || !q.hasAttribute("data-clearable")) return;
    const r = rows();
    const next = r[r.indexOf(el) + 1] || r[r.indexOf(el) - 1];
    el.dataset.setAside = "1";
    const restore = (opts.onSetAside && opts.onSetAside(el)) || collapse(el);
    undos.push(() => { el.dataset.setAside = ""; restore(); select(el); });
    note(q.dataset.clearable || "set aside");
    select(next);
  }

  function undo() {
    const u = undos.pop(); if (!u) return;
    u();
    const n = document.querySelector(".hint-bar .undo-note");
    if (n) n.classList.remove("is-on");
  }

  document.addEventListener("keydown", e => {
    if (document.querySelector(".palette-scrim.is-open")) return;
    if (/^(INPUT|TEXTAREA|SELECT)$/.test(document.activeElement.tagName)) return;
    if (e.metaKey || e.ctrlKey || e.altKey) return;
    const k = e.key;
    if (k === "j" || k === "ArrowDown") { e.preventDefault(); move(1); }
    else if (k === "k" || k === "ArrowUp") { e.preventDefault(); move(-1); }
    else if (k === "Enter") { e.preventDefault(); if (opts.onOpen) opts.onOpen(current()); }
    else if (k === "x") { e.preventDefault(); const el = current(); if (el) setAside(el); }
    else if (k === "u") { e.preventDefault(); undo(); }
    else if (opts.keys && opts.keys[k]) { e.preventDefault(); opts.keys[k](current()); }
  });

  queues.forEach(q => q.addEventListener("click", e => {
    const el = e.target.closest("[data-row]"); if (!el) return;
    select(el); if (opts.onOpen) opts.onOpen(el);
  }));

  if (opts.selectFirst !== false) select(rows()[0]);
}

/* Light/dark toggle. Mockup convenience, matches the app's system default. */
function perchTheme() {
  const root = document.documentElement;
  const saved = localStorage.getItem("perch-theme");
  if (saved) root.setAttribute("data-theme", saved);
  else if (window.matchMedia("(prefers-color-scheme: dark)").matches) root.setAttribute("data-theme", "dark");
  window.perchToggleTheme = () => {
    const next = root.getAttribute("data-theme") === "dark" ? "light" : "dark";
    root.setAttribute("data-theme", next);
    localStorage.setItem("perch-theme", next);
  };
}

/* Shared left rail, so every mockup navigates like the real app. */
function perchRail(active) {
  const items = [
    ["feed.html",       "Feed",       `<path d="M4 5h16M4 12h16M4 19h10"/>`],
    ["applications.html","Applications",`<path d="M5 4h9l5 5v11a1 1 0 0 1-1 1H5a1 1 0 0 1-1-1V5a1 1 0 0 1 1-1z"/><path d="M14 4v5h5"/>`],
    ["watchlist.html",  "Watchlist",  `<circle cx="12" cy="12" r="3"/><path d="M2 12s3.5-6 10-6 10 6 10 6-3.5 6-10 6-10-6-10-6z"/>`],
    ["profile.html",    "Profile",    `<circle cx="12" cy="8" r="3.5"/><path d="M4.5 20a7.5 7.5 0 0 1 15 0"/>`],
  ];
  return `
  <nav class="rail">
    <div class="wordmark">Perch<span class="dot"></span></div>
    ${items.map(([href, label, path]) => `
      <a class="rail-item ${active === label ? "is-active" : ""}" href="${href}">
        <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-linecap="round" stroke-linejoin="round">${path}</svg>
        ${label}
      </a>`).join("")}
    <button class="rail-item" onclick="perchOpenPalette()">
      <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-linecap="round" stroke-linejoin="round"><path d="M4 17l6-6-6-6M12 19h8"/></svg>
      Command
      <span style="margin-left:auto" class="kbd">⌘K</span>
    </button>
    <div class="rail-foot">
      <div class="line"><span style="width:5px;height:5px;border-radius:50%;background:var(--ink-4);display:inline-block"></span> last sync 11 minutes ago</div>
      <div class="line" style="cursor:pointer" onclick="perchToggleTheme()">everything stays on this Mac</div>
    </div>
  </nav>`;
}

function perchBoot(opts = {}) {
  perchTheme();
  const slot = document.querySelector("[data-rail]");
  if (slot) slot.outerHTML = perchRail(opts.active);
  perchPalette();
  perchQueue(opts.queue || {});
}
