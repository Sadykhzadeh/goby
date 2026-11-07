// Builds the website in docs/ from site/voices.json and the page below.
//
//   cargo test --release site_voices -- --ignored   # writes site/voices.json
//   node site/build.mjs 0.4.0
//
// The "try it" voices are the app's own DSP built for the browser; see site/dsp-wasm.
import fs from "node:fs";

const here = new URL(".", import.meta.url).pathname.replace(/^\/([A-Za-z]:)/, "$1");
const version = process.argv[2] || "0.4.0";
const voices = JSON.parse(fs.readFileSync(here + "voices.json", "utf8"));
const out = here + "../docs/";
const repo = "https://github.com/Sadykhzadeh/goby";

const esc = s => s.replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/"/g, "&quot;");

// The categories in the app's order (voices.rs, Category::ALL).
const cats = ["Basics", "People", "Creatures", "Robots", "Places", "Devices", "Fun"];

// A tile's wave as an SVG path in relative steps: one unit across per point, ±11 high
// as the app draws it. site.js reads the points back from here, so this is also the data.
function tilePath(points) {
  const y = v => Math.round(-v * 110) / 10;
  let d = "M0 " + y(points[0]), prev = y(points[0]);
  for (let i = 1; i < points.length; i++) {
    const now = y(points[i]);
    d += "l1 " + Math.round((now - prev) * 10) / 10;
    prev = now;
  }
  return d;
}

// The header's scope: a smooth curve through the points, 1000 x 44. site.js has the same.
function scopePath(p) {
  const step = 1000 / (p.length - 1), y = v => (22 - v * 18).toFixed(1);
  let d = `M0 ${y(p[0])}`;
  for (let i = 1; i < p.length; i++) {
    const x0 = (i - 1) * step, x1 = i * step, c = step / 2;
    d += `C${(x0 + c).toFixed(1)} ${y(p[i - 1])} ${(x1 - c).toFixed(1)} ${y(p[i])} ${x1.toFixed(1)} ${y(p[i])}`;
  }
  return d;
}

// Icons in the style of Segoe Fluent, drawn on a 16 unit grid. The font itself only
// exists on Windows, so the page carries its own.
function star() {
  const pts = [];
  for (let k = 0; k < 10; k++) {
    const a = -Math.PI / 2 + k * Math.PI / 5, r = k % 2 ? 2.9 : 6.6;
    pts.push(`${(8 + r * Math.cos(a)).toFixed(2)} ${(8.6 + r * Math.sin(a)).toFixed(2)}`);
  }
  return "M" + pts.join("L") + "z";
}
function gear() {
  const pts = [], deg = Math.PI / 180;
  for (let k = 0; k < 8; k++) {
    const a = k * 45 * deg;
    for (const [o, r] of [[-17, 5.1], [-9, 6.7], [9, 6.7], [17, 5.1]]) {
      pts.push(`${(8 + r * Math.cos(a + o * deg)).toFixed(2)} ${(8 + r * Math.sin(a + o * deg)).toFixed(2)}`);
    }
  }
  return "M" + pts.join("L") + "z";
}
const icons = {
  voices: `<circle cx="8" cy="8" r="6.5"/><path d="M5.6 9.6c1.3 1.6 3.5 1.6 4.8 0"/><circle cx="6" cy="6.4" r=".5" fill="currentColor" stroke="none"/><circle cx="10" cy="6.4" r=".5" fill="currentColor" stroke="none"/>`,
  apps: `<rect x="2" y="2.5" width="2.5" height="2.5" rx=".4"/><rect x="2" y="6.75" width="2.5" height="2.5" rx=".4"/><rect x="2" y="11" width="2.5" height="2.5" rx=".4"/><path d="M7 3.75h7M7 8h7M7 12.25h7"/>`,
  settings: `<path d="${gear()}"/><circle cx="8" cy="8" r="2.2"/>`,
  tone: `<path d="M6 12V3.6l7.5-1.6v8.5"/><circle cx="4.4" cy="12" r="1.6"/><circle cx="11.9" cy="10.5" r="1.6"/><path d="M6 6.2l7.5-1.6"/>`,
  volume: `<path d="M2 6h2.6L8 3v10L4.6 10H2z"/><path d="M10.4 6.1a2.8 2.8 0 0 1 0 3.8M12.4 4.3a5.4 5.4 0 0 1 0 7.4"/>`,
  headphone: `<path d="M2.5 11.5V8.5a5.5 5.5 0 0 1 11 0v3"/><path d="M2.5 9.5h2v4.5h-1a1 1 0 0 1-1-1zM13.5 9.5h-2v4.5h1a1 1 0 0 0 1-1z"/>`,
  star: `<path d="${star()}"/>`,
  starred: `<path d="${star()}" fill="currentColor" stroke="none"/>`,
  mic: `<rect x="5.8" y="1.5" width="4.4" height="8" rx="2.2"/><path d="M3.3 7.6a4.7 4.7 0 0 0 9.4 0M8 12.3v2.2"/>`,
  gate: `<path d="M1.5 5h7M11.6 5h2.9M1.5 11h2M6.6 11h7.9"/><circle cx="10" cy="5" r="1.6"/><circle cx="5" cy="11" r="1.6"/>`,
  more: `<circle cx="3.5" cy="8" r=".9" fill="currentColor" stroke="none"/><circle cx="8" cy="8" r=".9" fill="currentColor" stroke="none"/><circle cx="12.5" cy="8" r=".9" fill="currentColor" stroke="none"/>`,
  speakers: `<rect x="4" y="1.5" width="8" height="13" rx="1.5"/><circle cx="8" cy="10" r="2.4"/><circle cx="8" cy="4.8" r=".9"/>`,
  mix: `<rect x="2" y="2" width="12" height="12" rx="2"/><path d="M4 8.5h1.8l1.4-2.8 2 5 1.3-2.2H12"/>`,
  minimize: `<path d="M3 8h10"/>`,
  power: `<path d="M8 1.8v5.6M4.6 3.9a5.4 5.4 0 1 0 6.8 0"/>`,
  keyboard: `<rect x="1.5" y="4" width="13" height="8.5" rx="1.5"/><path d="M4 6.6h.6M6.5 6.6h.6M9 6.6h.6M11.5 6.6h.6M4 8.4h.6M11.5 8.4h.6M5.6 10.3h4.8"/>`,
  update: `<path d="M8 2v9M4.5 7.5 8 11l3.5-3.5M3 14h10"/>`,
  speed: `<path d="M2.3 12a6 6 0 1 1 11.4 0"/><path d="M8.6 9.2l2.6-3.4"/><circle cx="8" cy="10" r="1.1" fill="currentColor" stroke="none"/>`,
  info: `<circle cx="8" cy="8" r="6.5"/><path d="M8 7.3v4"/><circle cx="8" cy="5.1" r=".6" fill="currentColor" stroke="none"/>`,
  add: `<path d="M8 2.5v11M2.5 8h11"/>`,
  down: `<path d="M4 6l4 4 4-4"/>`,
  remove: `<path d="M4 4l8 8M12 4l-8 8"/>`,
  check: `<path d="M3 8.6l3 3 7-7"/>`,
  open: `<path d="M9.2 2.5h4.3v4.3M13.5 2.5 8 8M12 9.5v3a1 1 0 0 1-1 1H3.5a1 1 0 0 1-1-1V5a1 1 0 0 1 1-1h3"/>`,
  game: `<path d="M5 4.5h6a3.4 3.4 0 0 1 3.3 4.2l-.6 2.9a1.7 1.7 0 0 1-3 .8L9.4 11H6.6l-1.3 1.4a1.7 1.7 0 0 1-3-.8l-.6-2.9A3.4 3.4 0 0 1 5 4.5z"/><path d="M5 6.6v2.8M3.6 8h2.8"/><circle cx="10.4" cy="7.2" r=".5" fill="currentColor" stroke="none"/><circle cx="11.8" cy="8.7" r=".5" fill="currentColor" stroke="none"/>`,
  chat: `<path d="M3.5 2.5h9a2 2 0 0 1 2 2V10a2 2 0 0 1-2 2H7.5L4.3 14.5V12h-.8a2 2 0 0 1-2-2V4.5a2 2 0 0 1 2-2z"/>`,
  globe: `<circle cx="8" cy="8" r="6.5"/><ellipse cx="8" cy="8" rx="2.7" ry="6.5"/><path d="M1.5 8h13M2.4 4.8h11.2M2.4 11.2h11.2"/>`,
  video: `<rect x="1.5" y="4" width="9" height="8" rx="1.5"/><path d="M10.5 7l4-2v6l-4-2"/>`,
};
const sprite = `<svg class="sprite" aria-hidden="true">${Object.entries(icons)
  .map(([id, body]) => `<symbol id="i-${id}" viewBox="0 0 16 16">${body}</symbol>`).join("")}</svg>`;
const icon = (id, cls = "ic") => `<svg class="${cls}" aria-hidden="true"><use href="#i-${id}"/></svg>`;

// The window starts as in the app's screenshot: Minion chosen, a few voices starred.
const chosen = voices.findIndex(v => v[0] === "Minion");
const stars = ["Minion", "Robot", "Helium", "Demon"];

const tiles = voices.map(([name, cat, pts], i) => {
  const on = i === chosen, fav = stars.includes(name);
  return `<li class="tile${fav ? " fav" : ""}" data-cat="${cats.indexOf(cat)}"><button class="pick" type="button" aria-pressed="${on}"><svg viewBox="0 -12 59 24" preserveAspectRatio="none" aria-hidden="true"><path d="${tilePath(pts)}"/></svg><span class="nm">${esc(name)}</span></button><button class="star" type="button" aria-pressed="${fav}" aria-label="Favorite: ${esc(name)}">${icon(fav ? "starred" : "star")}</button></li>`;
}).join("");

const tabs = ["All", "Favorites", ...cats].map((t, i) =>
  `<button class="tab" type="button" role="tab" aria-selected="${i === 0}" data-shelf="${i - 2}" data-label="${t}">${t}</button>`).join("");

// A settings card: icon, title, description, and what sits on its right.
function row({ id = "", ico, title, desc, side = "", tag = "div", attrs = "" }) {
  const sideHtml = {
    "": "",
    on: `<span class="sw" aria-hidden="true"></span>`,
    off: `<span class="sw" aria-hidden="true"></span>`,
    menu: icon("down", "chev"),
    open: icon("open", "chev"),
    add: icon("add", "chev"),
  }[side] ?? side;
  const sw = side === "on" || side === "off";
  const a = sw ? ` role="switch" aria-checked="${side === "on"}"` : side === "menu" ? ` aria-haspopup="menu" aria-expanded="false"` : "";
  const t = sw || side === "menu" || side === "add" ? "button" : tag;
  const type = t === "button" ? ` type="button"` : "";
  return `<${t} class="row"${id ? ` id="${id}"` : ""}${type}${a}${attrs}>${icon(ico)}<span class="tx"><span class="tt">${title}</span><span class="ds">${desc}</span></span>${sideHtml}</${t}>`;
}

function slider({ id, ico, title, min, max, step, value, readout }) {
  return `<div class="row tall">${icon(ico)}<label class="tt" for="${id}">${title}</label><output class="val" for="${id}">${readout}</output><span class="trk r-${id}" aria-hidden="true"><span class="fill"></span></span><input class="rng" id="${id}" type="range" min="${min}" max="${max}" step="${step}" value="${value}"></div>`;
}

// The programs on the Programs page. Real icons are not ours to ship, so each shows the
// kind of program it is, as the app does for a program whose file it cannot read.
const programs = [
  { exe: "cs2.exe", name: "Counter-Strike 2", kind: "game", routed: true, state: "Hearing Goby right now" },
  { exe: "discord.exe", name: "Discord", kind: "chat", routed: true, state: "Running, hears Goby" },
  { exe: "telegram.exe", name: "Telegram", kind: "chat", active: true },
  { exe: "obs64.exe", name: "OBS Studio", kind: "video" },
  { exe: "chrome.exe", name: "Google Chrome", kind: "globe" },
  { exe: "steam.exe", name: "Steam", kind: "game" },
];
const programRow = p => p.routed
  ? `<div class="row prog" data-exe="${p.exe}">${icon(p.kind)}<span class="tx"><span class="tt">${p.name}</span><span class="ds">${p.state}</span></span><button class="x" type="button" aria-label="Stop sending Goby to ${p.name}">${icon("remove")}</button></div>`
  : `<button class="row prog" type="button" data-exe="${p.exe}">${icon(p.kind)}<span class="tx"><span class="tt">${p.name}</span><span class="ds">${p.active ? "Listening now · add" : "Add it to hear Goby"}</span></span>${icon("add", "chev")}</button>`;

const minion = voices[chosen][2], dry = voices[0][2];

const app = `
  <div class="win" id="win">
    <div class="bar">
      <img src="images/logo.svg" alt="" width="16" height="16"><span class="ttl">Goby</span>
      <button class="cap" type="button" data-cap="min" aria-label="Minimize"><svg viewBox="0 0 10 10" aria-hidden="true"><path d="M0 5.5h10"/></svg></button>
      <button class="cap" type="button" data-cap="max" aria-label="Maximize"><svg viewBox="0 0 10 10" aria-hidden="true"><rect class="one" x=".5" y=".5" width="9" height="9" rx="1.2"/><path class="two" d="M2.5 2.5V1.5a1 1 0 0 1 1-1h5a1 1 0 0 1 1 1v5a1 1 0 0 1-1 1h-1M.5 3.5a1 1 0 0 1 1-1h5a1 1 0 0 1 1 1v5a1 1 0 0 1-1 1h-5a1 1 0 0 1-1-1z"/></svg></button>
      <button class="cap close" type="button" data-cap="close" aria-label="Close"><svg viewBox="0 0 10 10" aria-hidden="true"><path d="M.5.5l9 9M9.5.5l-9 9"/></svg></button>
    </div>
    <div class="body">
      <nav class="rail" aria-label="Goby pages">
        <button type="button" data-page="voices" aria-current="page" aria-label="Voices" data-tip="Voices">${icon("voices")}</button>
        <button type="button" data-page="programs" aria-label="Programs" data-tip="Programs">${icon("apps")}</button>
        <button type="button" data-page="settings" aria-label="Settings" data-tip="Settings">${icon("settings")}</button>
      </nav>

      <div class="page" id="p-voices">
        <div class="card scope">
          <h3 class="vname" id="vname">Minion</h3>
          <p class="line" id="vline">Basics</p>
          <button class="save" id="save" type="button" hidden>${icon("star")}Save as my voice</button>
          <span class="saved" id="saved" hidden>${icon("starred")}In Favorites</span>
          <button class="effect" id="effect" type="button" role="switch" aria-checked="true">Effect<span class="sw" aria-hidden="true"></span></button>
          <svg class="trace" viewBox="0 0 1000 44" preserveAspectRatio="none" aria-hidden="true"><path class="raw" id="raw" d="${scopePath(dry)}"/><path class="voiced" id="voiced" d="${scopePath(minion)}"/></svg>
        </div>
        <div class="card quick">
          <div class="q">${icon("tone")}<label class="tt" for="tone">Pitch</label><output class="val" for="tone">0 st</output><span class="trk r-tone" aria-hidden="true"><span class="fill"></span></span><input class="rng" id="tone" type="range" min="-12" max="12" step="1" value="0"></div>
          <div class="q">${icon("volume")}<label class="tt" for="volume">Volume</label><output class="val" for="volume">100 %</output><span class="trk r-volume" aria-hidden="true"><span class="fill"></span></span><input class="rng" id="volume" type="range" min="0" max="2" step="0.05" value="1"></div>
          <button class="q hear" id="hear" type="button" role="switch" aria-checked="false">${icon("headphone")}<span class="tx"><span class="tt">Hear myself</span><span class="ds">In your headphones</span></span><span class="sw" aria-hidden="true"></span></button>
        </div>
        <div class="tabs" role="tablist" aria-label="Voice categories">${tabs}<span class="ind" aria-hidden="true"></span></div>
        <div class="gallery" id="gallery">
          <ul class="tiles" id="tiles">${tiles}</ul>
          <div class="empty" id="empty" hidden><b>Star a voice, or right-click it, to keep it here</b><span>Favorites are in the tray menu too, and Ctrl+Alt+PgUp/PgDn step through them only</span></div>
        </div>
      </div>

      <div class="page" id="p-programs" hidden>
        <h3 class="title">Programs</h3>
        <div class="pane">
          <p class="note">The programs chosen here hear Goby instead of your microphone. Everything else hears your own mic.</p>
          ${row({ ico: "mic", title: "Goby Mic", desc: "Goby reaches the programs through it" })}
          <h4>Hear Goby</h4>
          <div class="list" id="routed">${programs.filter(p => p.routed).map(programRow).join("")}</div>
          <div class="anchor">${row({ id: "add-app", ico: "add", title: "Add a program", desc: "From the list or any program file", side: "menu" })}</div>
          <div class="recent" id="recent-box">
            <h4>Recently used the microphone</h4>
            <div class="list" id="recent">${programs.filter(p => !p.routed).map(programRow).join("")}</div>
          </div>
        </div>
      </div>

      <div class="page" id="p-settings" hidden>
        <h3 class="title">Settings</h3>
        <div class="pane">
          <h4>Microphone</h4>
          <div class="anchor">${row({ id: "pick-mic", ico: "mic", title: "Device", desc: "Microphone", side: "menu" })}</div>
          ${row({ id: "gate", ico: "gate", title: "Noise gate", desc: "Silence between phrases", side: "on" })}
          <button class="row" id="adv-btn" type="button" aria-expanded="false" aria-controls="adv">${icon("more")}<span class="tx"><span class="tt">Advanced</span><span class="ds">Latency, gain, gate threshold</span></span>${icon("down", "chev")}</button>
          <div class="adv" id="adv" hidden>
            ${row({ id: "fast", ico: "speed", title: "Low latency", desc: "−10 ms, but only Goby gets the mic", side: "off" })}
            ${slider({ id: "gain", ico: "volume", title: "Gain", min: -12, max: 24, step: 1, value: 0, readout: "0 dB" })}
            ${slider({ id: "thresh", ico: "gate", title: "Gate threshold", min: -80, max: -10, step: 1, value: -50, readout: "−50 dB" })}
          </div>
          <h4>Output</h4>
          <div class="anchor">${row({ id: "pick-out", ico: "speakers", title: "Send to", desc: "Speakers", side: "menu" })}</div>
          ${slider({ id: "mix", ico: "mix", title: "Original voice in mix", min: 0, max: 1, step: 0.05, value: 0, readout: "0 %" })}
          <h4>General</h4>
          ${row({ id: "tray", ico: "minimize", title: "Keep running in the tray", desc: "The voice stays on when closed", side: "on" })}
          ${row({ id: "autostart", ico: "power", title: "Start with Windows", desc: "Straight to the tray, no window", side: "off" })}
          ${row({ id: "hotkeys", ico: "keyboard", title: "Hotkeys", desc: "Ctrl+Alt+G effect, PgUp/PgDn voice", side: "on" })}
          ${row({ id: "updates", ico: "update", title: "Check for updates", desc: "Once a day, only releases the author signed", side: "on" })}
          <div class="gap"></div>
          ${row({ ico: "info", title: `Goby ${version}`, desc: "Azer Sadykhzadeh · azer.one", side: "open", tag: "a", attrs: ` href="https://azer.one"` })}
        </div>
      </div>
      <div class="menu" id="menu" role="menu" hidden></div>
    </div>
    <div class="trayed" id="trayed" hidden>
      <p>Goby is still running in the tray, with the voice on.</p>
      <button type="button" id="reopen">Open Goby</button>
    </div>
  </div>`;

const html = `<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>Goby: a tiny voice changer for Windows</title>
<meta name="description" content="61 voices for CS2, Discord, OBS or any program you pick. One 500 KB Windows app, free and open source, no account.">
<meta name="theme-color" content="#13100c">
<meta property="og:title" content="Goby: a tiny voice changer for Windows">
<meta property="og:description" content="61 voices for the games and chats you pick. One 500 KB Windows app, free and open source.">
<meta property="og:image" content="https://goby.azer.one/images/social.png">
<meta property="og:url" content="https://goby.azer.one/">
<meta name="twitter:card" content="summary_large_image">
<link rel="icon" href="images/logo.svg" type="image/svg+xml">
<link rel="preconnect" href="https://api.fontshare.com">
<link rel="preconnect" href="https://cdn.fontshare.com" crossorigin>
<link rel="stylesheet" href="https://api.fontshare.com/v2/css?f[]=pally@500,700&display=swap">
<link rel="stylesheet" href="site.css">
<script src="site.js" defer></script>
</head>
<body>
${sprite}
<nav class="top">
  <a class="mark" href="#top"><img src="images/logo.svg" alt="" width="26" height="26">goby</a>
  <a class="link" href="#voices">Voices</a>
  <a class="link" href="#setup">Setup</a>
  <a class="link" href="#faq">FAQ</a>
  <a class="link" href="${repo}">GitHub</a>
</nav>

<main class="wrap" id="top">
  <section class="hero">
    <h1>Talk like <span>anyone</span> else.</h1>
    <div class="stage">
      <svg viewBox="0 0 1200 260" preserveAspectRatio="none" aria-hidden="true">
        <path class="echo" id="echo" d=""/>
        <path id="wave" d=""/>
      </svg>
      <button class="voice" id="voice" type="button" title="Next voice">
        <small id="count">2 / 61</small><span id="name">Minion</span><span class="next">next</span>
      </button>
    </div>
    <div class="foot">
      <p><b>61 voices</b> for CS2, Discord, OBS or any program you pick, from one <b>500 KB</b> Windows app. Free, open source, no account.</p>
      <div class="get">
        <a class="download" href="${repo}/releases/latest">
          <svg viewBox="0 0 20 20" fill="none" stroke="currentColor" stroke-width="2.2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M10 3v10M5.5 8.8 10 13.3l4.5-4.5M4 17h12"/></svg>
          Download Goby ${version}
        </a>
        <small>Windows 10 and 11</small>
      </div>
    </div>
  </section>

  <section id="voices" class="live">
    <div class="live-head">
      <h2>Every voice is drawn the way it sounds.</h2>
      <div class="try">
        <button class="try-btn" id="try" type="button" aria-pressed="false">${icon("mic")}<span id="try-label">Try it with your microphone</span></button>
        <p id="try-note">Put headphones on first: through speakers your voice comes back into the mic. Nothing leaves your browser.</p>
      </div>
    </div>
${app}
  </section>

  <section id="setup" class="setup">
    <div class="steps">
      <h2>Three minutes from download to the game.</h2>
      <div class="step">
        <h3>Get Goby</h3>
        <p>One file. Choose <b>Install Goby</b>, or <b>Just run it</b> to try it without installing. No administrator rights, no account.</p>
      </div>
      <div class="step">
        <h3>Add the cable</h3>
        <p>Goby reaches other programs through the free <a href="https://vb-audio.com/Cable/">VB-Audio Virtual Cable</a>. Without it, Goby walks you through installing it and picks it up without a restart.</p>
      </div>
      <div class="step">
        <h3>Pick the programs</h3>
        <p>On <b>Programs</b>, press <b>+</b> next to CS2, Discord or OBS. Only they hear the voice; everything else keeps your real microphone. A program with its own microphone setting, like Telegram, needs it on <b>Default</b> once, and Goby tells you when.</p>
      </div>
    </div>
    <figure class="side">
      <img src="images/programs.webp" width="790" height="845" loading="lazy" decoding="async" alt="Goby's Programs page: Counter-Strike 2 hearing Goby right now, and Telegram, Chrome, Steam, OBS and Discord offered with their own icons.">
    </figure>
  </section>

  <section class="facts">
    <p>About <b>28 ms</b> from your mouth to the game. Under <b>0.4 %</b> of one core for the heaviest voice.</p>
  </section>

  <section class="playing">
    <div>
      <h2>Made for playing.</h2>
      <ul class="keys">
        <li><kbd>Ctrl</kbd><kbd>Alt</kbd><kbd>G</kbd><span>effect on and off, inside the game</span></li>
        <li><kbd>Ctrl</kbd><kbd>Alt</kbd><kbd>PgUp</kbd><span>previous voice</span></li>
        <li><kbd>Ctrl</kbd><kbd>Alt</kbd><kbd>PgDn</kbd><span>next voice</span></li>
      </ul>
      <p>Star the voices you use and the hotkeys and the tray menu cycle only those. Close the window and Goby keeps going in the tray; it starts with Windows only if you ask it to.</p>
    </div>
    <figure class="side">
      <img src="images/settings.webp" width="790" height="950" loading="lazy" decoding="async" alt="Goby's Settings page: microphone, noise gate, output, tray, startup, hotkeys and updates.">
    </figure>
  </section>

  <section id="faq" class="faq">
    <h2>Questions</h2>
    <details>
      <summary>Will an anti-cheat mind it?</summary>
      <p>Goby never touches the game: it does not inject anything or read its memory. The game only sees one more microphone, made by VB-Audio's ordinary signed driver.</p>
    </details>
    <details>
      <summary>Can other programs still use my microphone?</summary>
      <p>Yes, Goby shares it. <i>Low latency</i> under Settings &gt; Advanced takes it exclusively for about 14 ms less delay, and then nothing else can record from it while Goby runs.</p>
    </details>
    <details>
      <summary>How do I know my copy is the real one?</summary>
      <p>Every release lists the SHA-256 of each file in <code>SHA256SUMS.txt</code>; compare it with <code>Get-FileHash</code> in PowerShell. Goby's own updater installs only releases signed with the author's key. Settings shows the version and the commit a copy says it was built from.</p>
    </details>
    <details>
      <summary>Why does it need VB-Cable?</summary>
      <p>A program can only record from a microphone Windows knows about. VB-Cable is that microphone; Goby plays into it. Writing a driver of its own would mean a kernel component on your machine, and Goby would rather not.</p>
    </details>
    <details>
      <summary>Mac or Linux?</summary>
      <p>No. Goby is built on Windows audio and the Windows UI, which is a large part of why it is small and fast.</p>
    </details>
  </section>
</main>

<footer>
  <div class="wrap links">
    <span>Goby ${version} · GPL-3.0</span>
    <a href="${repo}">GitHub</a>
    <a href="${repo}/releases">All releases</a>
    <a class="credit" href="https://azer.one">done with <span aria-hidden="true" class="heart">♥</span> by azer.one</a>
  </div>
  <span class="word" aria-hidden="true">goby</span>
</footer>
</body>
</html>
`;

fs.writeFileSync(out + "index.html", html);
console.log("built", out, html.length, "bytes of html");
