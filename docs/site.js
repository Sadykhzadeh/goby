// The hero's wave and the live Goby window. Everything is on the page already; this only
// makes it move and respond. Frames are drawn only while something is changing.
document.documentElement.classList.add("js");
const $ = id => document.getElementById(id);
const still = matchMedia("(prefers-reduced-motion: reduce)").matches;
const CATS = ["Basics", "People", "Creatures", "Robots", "Places", "Devices", "Fun"];

// Every voice's waveform, read back from its tile: "M0 y l1 dy l1 dy ...", 11 units a side.
const tileEls = [...document.querySelectorAll("#tiles .tile")];
const VOICES = tileEls.map(li => {
  const n = li.querySelector("path").getAttribute("d").match(/-?[\d.]+/g).map(Number);
  let y = n[1];
  const pts = [-y / 11];
  for (let i = 3; i < n.length; i += 2) pts.push(-(y += n[i]) / 11);
  return { name: li.querySelector(".nm").textContent, cat: +li.dataset.cat, pts };
});

function smooth(p, w, h, amp) {
  const step = w / (p.length - 1), y = v => (h / 2 - v * amp).toFixed(1);
  let d = `M0 ${y(p[0])}`;
  for (let i = 1; i < p.length; i++) {
    const x0 = (i - 1) * step, x1 = i * step, c = step / 2;
    d += `C${(x0 + c).toFixed(1)} ${y(p[i - 1])} ${(x1 - c).toFixed(1)} ${y(p[i])} ${x1.toFixed(1)} ${y(p[i])}`;
  }
  return d;
}
const ease = k => 1 - Math.pow(1 - Math.min(1, k), 3);

// ---- The hero: it eases from one voice's waveform to the next while it is on screen. ----
{
  const wave = $("wave"), echo = $("echo"), nameEl = $("name"), countEl = $("count"), hero = document.querySelector(".hero");
  let at = Math.max(0, VOICES.findIndex(v => v.name === "Minion"));
  let shown = VOICES[at].pts.slice(), from = shown, to = shown, t0 = 0, running = false;
  const draw = k => {
    const e = ease(k);
    shown = to.map((v, i) => from[i] + (v - from[i]) * e);
    wave.setAttribute("d", smooth(shown, 1200, 260, 260 * 0.36));
    echo.setAttribute("d", smooth(shown, 1200, 260, 260 * 0.44));
  };
  const frame = now => {
    const k = (now - t0) / 650;
    draw(k);
    running = k < 1;
    if (running) requestAnimationFrame(frame);
  };
  const show = i => {
    at = (i + VOICES.length) % VOICES.length;
    nameEl.textContent = VOICES[at].name;
    countEl.textContent = `${at + 1} / ${VOICES.length}`;
    from = shown; to = VOICES[at].pts; t0 = performance.now();
    if (still) return draw(1);
    if (!running) { running = true; requestAnimationFrame(frame); }
  };
  let lastClick = 0, visible = true;
  $("voice").addEventListener("click", () => { lastClick = performance.now(); show(at + 1); });
  new IntersectionObserver(([e]) => { visible = e.isIntersecting; }).observe(hero);
  draw(1);
  if (!still) {
    setInterval(() => {
      if (visible && !document.hidden && performance.now() - lastClick > 6000) show(at + 1);
    }, 2600);
  }
}

// ---- The window. State as the app keeps it, and one function per thing it draws. ----
const win = $("win");
const S = {
  voice: VOICES.findIndex(v => v.name === "Minion"),
  tone: 0, volume: 1, mix: 0, gain: 0, thresh: -50,
  enabled: true, gate: true, hotkeys: true, tray: true,
  shelf: -2, // -2 all, -1 favorites, 0.. a category
  favs: tileEls.filter(li => li.classList.contains("fav")).map(li => tileEls.indexOf(li)),
  presets: [], // { voice, tone, volume, mix }
  live: null, latency: 0,
};

const signed = v => (v > 0 ? "+" + v : v < 0 ? "−" + -v : "0");
const readout = {
  tone: v => `${signed(v)} st`, volume: v => `${Math.round(v * 100)} %`, mix: v => `${Math.round(v * 100)} %`,
  gain: v => `${signed(v)} dB`, thresh: v => `${signed(v)} dB`,
};
const same = (a, b) => Math.abs(a - b) < 0.005;
const tweaks = p => [p.tone ? (p.tone > 0 ? "+" : "") + p.tone : "", same(p.volume, 1) ? "" : Math.round(p.volume * 100) + "%",
  p.mix > 0 ? Math.round(p.mix * 100) + "%" : ""].filter(Boolean).join(" · ");
const presetLabel = p => VOICES[p.voice].name + (p.tone ? " " + (p.tone > 0 ? "+" : "") + p.tone : "") +
  (same(p.volume, 1) ? "" : ` · ${Math.round(p.volume * 100)}%`) + (p.mix > 0 ? ` · ${Math.round(p.mix * 100)}% own` : "");
const activePreset = () => S.presets.findIndex(p => p.voice === S.voice && p.tone === S.tone && same(p.volume, S.volume) && same(p.mix, S.mix));
const tuned = () => S.tone !== 0 || S.mix !== 0 || !same(S.volume, 1);
const useIcon = (btn, id) => btn.querySelector("use").setAttribute("href", "#i-" + id);

// The header: name, the line under it, and the scope.
const rawPath = $("raw"), voicedPath = $("voiced"), vline = $("vline");
let scopeFrom = VOICES[S.voice].pts, scopeTo = scopeFrom, scopeShown = scopeFrom, scopeT0 = 0, scopeRunning = false;
function scopeFrame(now) {
  if (S.live) { scopeRunning = false; return; }
  const k = (now - scopeT0) / 300;
  scopeShown = scopeTo.map((v, i) => scopeFrom[i] + (v - scopeFrom[i]) * ease(k));
  voicedPath.setAttribute("d", smooth(S.enabled ? scopeShown : VOICES[0].pts, 1000, 44, 18));
  scopeRunning = k < 1;
  if (scopeRunning) requestAnimationFrame(scopeFrame);
}
function idleScope() {
  if (S.live) return;
  rawPath.setAttribute("d", smooth(VOICES[0].pts, 1000, 44, 18));
  scopeFrom = scopeShown; scopeTo = VOICES[S.voice].pts; scopeT0 = performance.now();
  if (still) { scopeShown = scopeTo; voicedPath.setAttribute("d", smooth(S.enabled ? scopeTo : VOICES[0].pts, 1000, 44, 18)); return; }
  if (!scopeRunning) { scopeRunning = true; requestAnimationFrame(scopeFrame); }
}

function header() {
  const p = activePreset();
  $("vname").textContent = p >= 0 ? presetLabel(S.presets[p]) : VOICES[S.voice].name;
  vline.textContent = "";
  if (!$("routed").children.length) {
    const b = document.createElement("button");
    b.type = "button";
    b.textContent = "Nobody hears Goby yet: pick the programs";
    b.addEventListener("click", () => go("programs"));
    vline.append(b);
  } else if (!S.enabled) {
    vline.textContent = "Effect off: your own voice goes through";
  } else {
    vline.textContent = CATS[VOICES[S.voice].cat] + (S.live && S.latency ? ` · latency ${Math.round(S.latency)} ms` : "");
  }
  $("saved").hidden = p < 0;
  $("save").hidden = p >= 0 || !tuned();
  tiles();
}

// The gallery: which tiles show on the current shelf, in what order, and which is chosen.
function tiles() {
  const p = activePreset(), fav = S.shelf === -1;
  tileEls.forEach((li, i) => {
    li.hidden = !(S.shelf === -2 || (fav && S.favs.includes(i)) || li.dataset.cat == S.shelf);
    li.style.order = fav ? S.presets.length + S.favs.indexOf(i) : "";
    li.querySelector(".pick").setAttribute("aria-pressed", i === S.voice && (!fav || p < 0));
  });
  document.querySelectorAll("#tiles .preset").forEach((li, k) => {
    li.hidden = !fav;
    li.style.order = k;
    li.querySelector(".pick").setAttribute("aria-pressed", k === p);
  });
  $("empty").hidden = !(fav && !S.favs.length && !S.presets.length);
}

function choose(i) {
  S.voice = i; S.tone = 0;
  setKnob("tone", 0);
  apply();
}
function apply() {
  header();
  idleScope();
  send();
}

function toggleFav(i) {
  const li = tileEls[i], on = !S.favs.includes(i);
  S.favs = on ? [...S.favs, i] : S.favs.filter(f => f !== i);
  li.classList.toggle("fav", on);
  const star = li.querySelector(".star");
  star.setAttribute("aria-pressed", on);
  useIcon(star, on ? "starred" : "star");
  tiles();
}

tileEls.forEach((li, i) => {
  li.querySelector(".pick").addEventListener("click", () => choose(i));
  li.querySelector(".star").addEventListener("click", () => toggleFav(i));
  li.addEventListener("contextmenu", e => { e.preventDefault(); toggleFav(i); });
});

// Saved voices: a tile with what was changed on a second line, kept in Favorites.
function renderPresets() {
  document.querySelectorAll("#tiles .preset").forEach(li => li.remove());
  const list = $("tiles");
  S.presets.forEach((p, k) => {
    const li = document.createElement("li");
    li.className = "tile preset";
    li.innerHTML = `<button class="pick" type="button" aria-pressed="false">${tileEls[p.voice].querySelector("svg").outerHTML}<span class="nm"></span><span class="tw"></span></button><button class="star" type="button"><svg class="ic" aria-hidden="true"><use href="#i-starred"/></svg></button>`;
    li.querySelector(".nm").textContent = VOICES[p.voice].name;
    li.querySelector(".tw").textContent = tweaks(p);
    const del = li.querySelector(".star");
    del.setAttribute("aria-label", "Delete saved voice " + presetLabel(p));
    li.addEventListener("pointerenter", () => useIcon(del, "remove"));
    li.addEventListener("pointerleave", () => useIcon(del, "starred"));
    del.addEventListener("focus", () => useIcon(del, "remove"));
    del.addEventListener("blur", () => useIcon(del, "starred"));
    li.querySelector(".pick").addEventListener("click", () => {
      Object.assign(S, { voice: p.voice, tone: p.tone, volume: p.volume, mix: p.mix });
      ["tone", "volume", "mix"].forEach(k => setKnob(k, S[k]));
      apply();
    });
    del.addEventListener("click", () => { S.presets.splice(k, 1); renderPresets(); header(); });
    list.prepend(li);
  });
  tiles();
}
$("save").addEventListener("click", () => {
  S.presets.push({ voice: S.voice, tone: S.tone, volume: S.volume, mix: S.mix });
  renderPresets();
  header();
});

// Category tabs and the indicator that slides between them.
const tabs = [...document.querySelectorAll(".tab")], ind = document.querySelector(".ind");
function placeIndicator(glide) {
  const t = tabs.find(t => t.getAttribute("aria-selected") === "true");
  ind.classList.toggle("glide", glide && !still);
  ind.style.transform = `translateX(${t.offsetLeft + t.offsetWidth / 2 - 8}px)`;
}
tabs.forEach(t => t.addEventListener("click", () => {
  tabs.forEach(o => o.setAttribute("aria-selected", o === t));
  S.shelf = +t.dataset.shelf;
  t.scrollIntoView({ block: "nearest", inline: "nearest", behavior: still ? "auto" : "smooth" });
  $("gallery").scrollTop = 0;
  tiles();
  placeIndicator(true);
}));
placeIndicator(false);
document.fonts?.ready.then(() => placeIndicator(false));
new ResizeObserver(() => placeIndicator(false)).observe(document.querySelector(".tabs"));

// Sliders: the readout, and the accent fill from the origin (0 for pitch, the minimum otherwise).
function setKnob(id, v, fromInput) {
  const el = $(id);
  if (!fromInput) el.value = v;
  const lo = +el.min, hi = +el.max, pct = x => ((x - lo) / (hi - lo) * 100).toFixed(2) + "%";
  const origin = id === "tone" ? 0 : lo;
  const track = el.parentNode.querySelector(".trk");
  track.style.setProperty("--a", pct(Math.min(origin, v)));
  track.style.setProperty("--b", pct(Math.max(origin, v)));
  el.parentNode.querySelector(".val").textContent = readout[id](v);
}
for (const id of ["tone", "volume", "mix", "gain", "thresh"]) {
  $(id).addEventListener("input", e => {
    S[id] = +e.target.value;
    setKnob(id, S[id], true);
    if (id === "tone" || id === "volume" || id === "mix") header();
    send();
  });
}

// Switches: the whole card or strip is the control, as in the app.
const switches = {
  effect: on => { S.enabled = on; win.classList.toggle("fx-off", !on); apply(); },
  gate: on => { S.gate = on; win.classList.toggle("gate-off", !on); send(); },
  hear: on => setLive(on),
  hotkeys: on => { S.hotkeys = on; },
  tray: on => { S.tray = on; },
};
document.querySelectorAll('.win [role="switch"]').forEach(b => b.addEventListener("click", () => {
  const on = b.getAttribute("aria-checked") !== "true";
  b.setAttribute("aria-checked", on);
  switches[b.id]?.(on);
}));

// Pages, with the rise-and-fade the app uses when switching.
const railButtons = [...document.querySelectorAll(".rail button")];
function go(page) {
  railButtons.forEach(b => b.dataset.page === page ? b.setAttribute("aria-current", "page") : b.removeAttribute("aria-current"));
  for (const name of ["voices", "programs", "settings"]) {
    const el = $("p-" + name), on = name === page;
    if (on && el.hidden) {
      el.hidden = false;
      el.classList.remove("enter");
      void el.offsetWidth;
      el.classList.add("enter");
    } else if (!on) el.hidden = true;
  }
  closeMenu();
  if (page === "voices") placeIndicator(false);
}
railButtons.forEach(b => b.addEventListener("click", () => go(b.dataset.page)));
document.querySelectorAll(".page").forEach(p => p.addEventListener("animationend", () => p.classList.remove("enter")));

$("adv-btn").addEventListener("click", e => {
  const open = $("adv").hidden;
  $("adv").hidden = !open;
  e.currentTarget.setAttribute("aria-expanded", open);
});

// The caption buttons: minimize and close leave Goby in the tray, as the app does.
const trayed = $("trayed"), body = win.querySelector(".body");
win.querySelectorAll(".cap").forEach(b => b.addEventListener("click", () => {
  if (b.dataset.cap === "max") {
    win.classList.toggle("max");
    b.setAttribute("aria-label", win.classList.contains("max") ? "Restore" : "Maximize");
    return placeIndicator(false);
  }
  const quit = b.dataset.cap === "close" && !S.tray;
  if (quit && S.live) setLive(false);
  trayed.querySelector("p").textContent = quit
    ? "Goby is closed. With Keep running in the tray off, closing quits it."
    : "Goby is still running in the tray, with the voice on.";
  $("reopen").textContent = quit ? "Start Goby" : "Open Goby";
  body.hidden = true;
  trayed.hidden = false;
  $("reopen").focus();
}));
$("reopen").addEventListener("click", () => {
  body.hidden = false;
  trayed.hidden = true;
  placeIndicator(false);
});

// Drop-down menus for the device pickers and Add a program, like the app's native menus.
const menu = $("menu");
let menuFor = null;
function closeMenu() {
  if (!menuFor) return;
  menuFor.setAttribute("aria-expanded", false);
  menu.hidden = true;
  menuFor = null;
}
function openMenu(anchor, items, current, pick) {
  if (menuFor === anchor) return closeMenu();
  closeMenu();
  menu.textContent = "";
  for (const item of items) {
    const b = document.createElement("button");
    b.type = "button";
    b.setAttribute("role", "menuitemradio");
    b.setAttribute("aria-checked", item === current);
    b.innerHTML = `<svg class="ic" aria-hidden="true"><use href="#i-check"/></svg>`;
    b.append(item);
    b.addEventListener("click", () => { closeMenu(); pick(item); anchor.focus(); });
    menu.append(b);
  }
  if (!items.length) {
    const b = document.createElement("button");
    b.disabled = true;
    b.textContent = "Every program in the list hears Goby";
    menu.append(b);
  }
  menu.hidden = false;
  const a = anchor.getBoundingClientRect(), box = body.getBoundingClientRect();
  const below = a.bottom - box.top + 4, h = menu.offsetHeight;
  menu.style.top = (below + h > box.height ? a.top - box.top - h - 4 : below) + "px";
  menu.style.left = Math.max(8, a.right - box.left - menu.offsetWidth - 8) + "px";
  anchor.setAttribute("aria-expanded", true);
  menuFor = anchor;
  menu.querySelector("button")?.focus();
}
menu.addEventListener("keydown", e => {
  const items = [...menu.querySelectorAll("button:not([disabled])")], i = items.indexOf(document.activeElement);
  if (e.key === "ArrowDown" || e.key === "ArrowUp") {
    e.preventDefault();
    items[(i + (e.key === "ArrowDown" ? 1 : items.length - 1)) % items.length]?.focus();
  }
});
document.addEventListener("pointerdown", e => { if (menuFor && !menu.contains(e.target) && !menuFor.contains(e.target)) closeMenu(); });
document.addEventListener("keydown", e => {
  if (e.key === "Escape" && menuFor) { const a = menuFor; closeMenu(); a.focus(); }
});
document.querySelectorAll(".pane").forEach(p => p.addEventListener("scroll", closeMenu, { passive: true }));

for (const [id, items] of [["pick-mic", ["Microphone", "Headset Microphone", "Webcam Microphone"]], ["pick-out", ["Speakers", "Headphones"]]]) {
  const row = $(id), ds = row.querySelector(".ds");
  row.addEventListener("click", () => openMenu(row, items, ds.textContent, v => { ds.textContent = v; }));
}

// Programs: which hear Goby, and what each of them is doing.
const PROGRAMS = {
  "cs2.exe": { name: "Counter-Strike 2", kind: "game", state: "Hearing Goby right now" },
  "discord.exe": { name: "Discord", kind: "chat", state: "Running, hears Goby" },
  "telegram.exe": { name: "Telegram", kind: "chat", active: true, state: "Records from Microphone, not Goby: set its microphone to Default" },
  "obs64.exe": { name: "OBS Studio", kind: "video", state: "Hears Goby once it starts" },
  "chrome.exe": { name: "Google Chrome", kind: "globe", state: "Hears Goby once it starts" },
  "steam.exe": { name: "Steam", kind: "game", state: "Hears Goby once it starts" },
};
function programRow(exe, routed) {
  const p = PROGRAMS[exe], el = document.createElement(routed ? "div" : "button");
  el.className = "row prog";
  el.dataset.exe = exe;
  const ic = id => `<svg class="ic" aria-hidden="true"><use href="#i-${id}"/></svg>`;
  el.innerHTML = `${ic(p.kind)}<span class="tx"><span class="tt"></span><span class="ds"></span></span>` +
    (routed ? `<button class="x" type="button">${ic("remove")}</button>` : `<svg class="chev" aria-hidden="true"><use href="#i-add"/></svg>`);
  el.querySelector(".tt").textContent = p.name;
  const ds = el.querySelector(".ds");
  ds.textContent = routed ? p.state : p.active ? "Listening now · add" : "Add it to hear Goby";
  if (routed) {
    const x = el.querySelector(".x");
    x.setAttribute("aria-label", "Stop sending Goby to " + p.name);
    x.addEventListener("click", () => { route(exe, false); $("add-app").focus(); });
  } else {
    el.type = "button";
    el.addEventListener("click", () => route(exe, true));
  }
  return el;
}
function route(exe, on) {
  document.querySelector(`.prog[data-exe="${exe}"]`)?.remove();
  $(on ? "routed" : "recent").append(programRow(exe, on));
  programsChanged();
}
function programsChanged() {
  const routed = $("routed").children.length;
  $("recent-box").hidden = !$("recent").children.length;
  $("add-app").querySelector(".ds").textContent = routed ? "From the list or any program file" : "For example CS2 or Discord";
  header();
}
document.querySelectorAll("#routed .prog").forEach(r => r.replaceWith(programRow(r.dataset.exe, true)));
document.querySelectorAll("#recent .prog").forEach(r => r.replaceWith(programRow(r.dataset.exe, false)));
$("add-app").addEventListener("click", () => {
  const offered = [...$("recent").children].map(r => PROGRAMS[r.dataset.exe].name);
  openMenu($("add-app"), offered, null, name => route(Object.keys(PROGRAMS).find(k => PROGRAMS[k].name === name), true));
});

// The hotkeys work here too while the switch is on: Ctrl+Alt+G, Ctrl+Alt+PgUp/PgDn.
document.addEventListener("keydown", e => {
  if (!S.hotkeys || !e.ctrlKey || !e.altKey || body.hidden) return;
  if (e.code === "KeyG") {
    e.preventDefault();
    $("effect").click();
  } else if (e.key === "PageUp" || e.key === "PageDown") {
    e.preventDefault();
    const ring = S.favs.length ? S.favs : VOICES.map((_, i) => i);
    const at = ring.indexOf(S.voice), step = e.key === "PageDown" ? 1 : -1;
    choose(ring[((at < 0 ? (step > 0 ? -1 : 0) : at) + step + ring.length) % ring.length]);
    // Picking a voice from a hotkey means wanting to hear it, as in the app.
    if (!S.enabled) $("effect").click();
  }
});

// ---- Hearing it: the app's own DSP, compiled to WebAssembly, on the visitor's microphone. ----
const tryBtn = $("try"), tryLabel = $("try-label"), tryNote = $("try-note");
const note0 = tryNote.innerHTML;
let liveFrame = 0;
function send() {
  S.live?.set({ voice: S.voice, tone: S.tone, volume: S.volume, mix: S.mix, enabled: S.enabled, gain: S.gain, gate: S.gate ? S.thresh : NaN });
}
function liveUi(on) {
  tryBtn.setAttribute("aria-pressed", on);
  $("hear").setAttribute("aria-checked", on);
  tryLabel.textContent = on ? "Stop listening" : "Try it with your microphone";
}
async function setLive(on) {
  if (!on) {
    S.live?.stop();
    S.live = null;
    S.latency = 0;
    cancelAnimationFrame(liveFrame);
    liveUi(false);
    tryNote.innerHTML = note0;
    header();
    idleScope();
    return;
  }
  if (S.live || tryBtn.dataset.busy) return;
  if (!window.AudioWorkletNode || !navigator.mediaDevices?.getUserMedia) {
    liveUi(false);
    tryNote.textContent = "This browser cannot run the voices live. A current Chrome, Edge, Firefox or Safari can.";
    return;
  }
  // Made inside the click, so the browser lets it play.
  const ctx = new AudioContext({ latencyHint: "interactive" });
  tryBtn.dataset.busy = "1";
  liveUi(true);
  tryNote.textContent = "Loading the voices, then your browser asks for the microphone…";
  try {
    const { start } = await import("./live.js");
    S.live = await start(ctx);
    send();
    tryNote.textContent = "Listening. Pick voices and drag Pitch while you talk.";
    header();
    liveFrame = requestAnimationFrame(drawLive);
  } catch (err) {
    ctx.close();
    S.live = null;
    liveUi(false);
    tryNote.textContent = err?.name === "NotAllowedError" ? "The microphone was not allowed, so there is nothing to hear yet."
      : err?.name === "NotFoundError" ? "No microphone was found on this device."
      : /Content Security|wasm-unsafe-eval|unsafe-eval/.test(err?.message) ? "This page is not allowed to run the voice engine yet."
      : "The voices could not start here: " + (err?.message || err);
  } finally {
    delete tryBtn.dataset.busy;
  }
}
tryBtn.addEventListener("click", () => setLive(!S.live));

// The live scope, drawn like the app's: locked to a rising zero crossing, squashed by tanh.
const rawBuf = new Float32Array(2048), outBuf = new Float32Array(2048);
function trace(s, span) {
  let start = s.length - span;
  for (let i = s.length - span - 1; i > 0; i--) if (s[i - 1] < 0 && s[i] >= 0) { start = i; break; }
  let d = "";
  for (let k = 0; k <= 192; k++) {
    const v = s[Math.min(start + Math.floor(k * span / 192), s.length - 1)];
    d += (k ? "L" : "M") + (k * 1000 / 192).toFixed(1) + " " + (22 - Math.tanh(v * 4) * 21).toFixed(1);
  }
  return d;
}
let lastLatency = 0;
function drawLive(now) {
  const L = S.live;
  if (!L) return;
  L.scope(rawBuf, outBuf);
  const span = Math.min(1536, Math.round(768 * L.rate / 48000));
  rawPath.setAttribute("d", trace(rawBuf, span));
  voicedPath.setAttribute("d", trace(outBuf, span));
  if (now - lastLatency > 500) {
    lastLatency = now;
    const l = L.latency();
    S.latency = l.total;
    const ms = v => Math.round(v * 1000);
    tryNote.innerHTML = `<b>About ${Math.round(l.total)} ms</b> from your microphone to your ears: ${ms(l.input)} ms in, ${ms(l.output)} ms out as your browser reports them, ${ms(l.voice)} ms for the voice itself.`;
    header();
  }
  liveFrame = requestAnimationFrame(drawLive);
}

header();
programsChanged();
