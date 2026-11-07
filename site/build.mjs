// Builds the website in docs/ from site/voices.json and the page below.
//
//   cargo test --release site_voices -- --ignored   # writes site/voices.json
//   node site/build.mjs 0.4.0
import fs from "node:fs";

const here = new URL(".", import.meta.url).pathname.replace(/^\/([A-Za-z]:)/, "$1");
const version = process.argv[2] || "0.4.0";
const voices = JSON.parse(fs.readFileSync(here + "voices.json", "utf8"));
const out = here + "../docs/";

const esc = s => s.replace(/&/g, "&amp;").replace(/</g, "&lt;");

// A tile's wave as an SVG path in relative steps: one unit across per point, so the
// sixty-point snapshot is a few hundred bytes. Height 32, as the app's tiles draw it.
function tilePath(points) {
  const y = v => Math.round((16 - v * 13.4) * 10) / 10;
  let d = "M0 " + y(points[0]), prev = y(points[0]);
  for (let i = 1; i < points.length; i++) {
    const now = y(points[i]);
    d += "l1 " + Math.round((now - prev) * 10) / 10;
    prev = now;
  }
  return d;
}

const groups = [];
for (const [name, cat, pts] of voices) {
  let g = groups.find(g => g.cat === cat);
  if (!g) groups.push(g = { cat, list: [] });
  g.list.push({ name, pts });
}

const gallery = groups.map(g => `
      <div class="group">
        <h3>${esc(g.cat)}</h3>
        <ul>${g.list.map(v => `
          <li><svg viewBox="0 0 59 32" preserveAspectRatio="none" aria-hidden="true"><path d="${tilePath(v.pts)}"/></svg><span>${esc(v.name)}</span></li>`).join("")}
        </ul>
      </div>`).join("");

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
<script src="voices.js" defer></script>
<script src="site.js" defer></script>
</head>
<body>
<nav>
  <a class="mark" href="#top"><img src="images/logo.svg" alt="" width="26" height="26">goby</a>
  <a class="link" href="#voices">Voices</a>
  <a class="link" href="#setup">Setup</a>
  <a class="link" href="#faq">FAQ</a>
  <a class="link" href="https://got.az/azer/goby">Source</a>
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
        <a class="download" href="https://github.com/Sadykhzadeh/goby/releases/latest">
          <svg viewBox="0 0 20 20" fill="none" stroke="currentColor" stroke-width="2.2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M10 3v10M5.5 8.8 10 13.3l4.5-4.5M4 17h12"/></svg>
          Download Goby ${version}
        </a>
        <small>Windows 10 and 11 · <a href="https://got.az/azer/goby/releases">also on got.az</a></small>
      </div>
    </div>
  </section>

  <figure class="shot">
    <img src="images/goby.webp" width="1016" height="820" loading="lazy" decoding="async" alt="Goby's main window: the Minion voice selected above a gallery of 61 voices, each drawn as its own waveform.">
  </figure>

  <section id="voices" class="voices">
    <h2>Every voice is drawn the way it sounds.</h2>
    <p class="lead">The same test tone run through each recipe, exactly as the app's tiles show it. Pitch it up or down, mix in your own voice, and save the result as a voice of your own.</p>
    <div class="gallery">${gallery}
    </div>
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
    <p>About <b>28 ms</b> from your mouth to the game. Under <b>0.4 %</b> of one core for the heaviest voice. Updates only from <b>signed</b> releases. Nothing sent anywhere else.</p>
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
    <a href="https://got.az/azer/goby">Source on got.az</a>
    <a href="https://github.com/Sadykhzadeh/goby">Mirror on GitHub</a>
    <a href="https://github.com/Sadykhzadeh/goby/releases">All releases</a>
    <a href="https://azer.one">Made by Azer Sadykhzadeh</a>
  </div>
  <span class="word" aria-hidden="true">goby</span>
</footer>
</body>
</html>
`;

fs.writeFileSync(out + "index.html", html);
fs.writeFileSync(out + "voices.js",
  "// The waveform of every voice, as its tile in the app draws it: a buzzy test tone run\n" +
  "// through the recipe, 60 points of the last 20 ms. Generated from src/voices.rs.\n" +
  "const VOICES = " + JSON.stringify(voices.map(([n, , p]) => [n, p])) + ";\n");
console.log("built", out, html.length, "bytes of html");
