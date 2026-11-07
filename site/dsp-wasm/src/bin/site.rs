//! Writes the website's page, docs/index.html, from the app's sources: its version and
//! every voice's name and waveform, as the window draws them on its tiles.
//!
//!   cargo run --release --bin site        (from site/dsp-wasm)
//!
//! The window on the page is goby.wasm, the library of this crate; until it loads, and
//! without JavaScript, docs/images/window.webp shows the same window.
#![allow(dead_code)]

#[path = "../../../../crates/dsp/src/dsp.rs"]
mod dsp;
#[path = "../../../../crates/dsp/src/fx.rs"]
mod fx;
#[path = "../../../../src/lang.rs"]
mod lang;
#[path = "../../../../src/voices.rs"]
mod voices;

/// The hero eases between the voices' waveforms: [[name, [points]], ...], 60 points each.
fn hero() -> String {
    let rows: Vec<String> = voices::ALL
        .iter()
        .zip(voices::glyphs(60))
        .map(|(v, g)| {
            let points: Vec<String> = g.iter().map(|x| format!("{}", (x * 100.0).round() / 100.0 + 0.0)).collect();
            format!("[\"{}\",[{}]]", v.name(), points.join(","))
        })
        .collect();
    format!("[{}]", rows.join(","))
}

fn main() {
    let page = PAGE.replace("{version}", env!("GOBY_VERSION")).replace("{hero}", &hero());
    let out = concat!(env!("CARGO_MANIFEST_DIR"), "/../../docs/index.html");
    std::fs::write(out, &page).expect("docs/index.html");
    println!("wrote docs/index.html, {} bytes", page.len());
}

const PAGE: &str = r##"<!doctype html>
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
<script type="application/json" id="hero-voices">{hero}</script>
<nav class="top">
  <a class="mark" href="#top"><img src="images/logo.svg" alt="" width="26" height="26">goby</a>
  <a class="link" href="#voices">Voices</a>
  <a class="link" href="#setup">Setup</a>
  <a class="link" href="#faq">FAQ</a>
  <a class="link" href="https://github.com/Sadykhzadeh/goby">GitHub</a>
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
          Download Goby {version}
        </a>
        <small>Windows 10 and 11</small>
      </div>
    </div>
  </section>

  <section id="voices" class="live">
    <div class="live-head">
      <h2>Every voice is drawn the way it sounds.</h2>
      <div class="try">
        <button class="try-btn" id="try" type="button" aria-pressed="false"><svg class="ic" viewBox="0 0 16 16" aria-hidden="true"><rect x="5.8" y="1.5" width="4.4" height="8" rx="2.2"/><path d="M3.3 7.6a4.7 4.7 0 0 0 9.4 0M8 12.3v2.2"/></svg><span id="try-label">Try it with your microphone</span></button>
        <p id="try-note">Put headphones on first: through speakers your voice comes back into the mic. Nothing leaves your browser.</p>
      </div>
    </div>

  <div class="win" id="win">
    <div class="bar"><img src="images/logo.svg" alt="" width="16" height="16"><span>Goby</span></div>
    <img class="shot" src="images/window.webp" width="1000" height="680" loading="lazy" decoding="async" alt="Goby's main window: the Minion voice in the header above Pitch and Volume, and a gallery of the starred voices, each drawn as its own waveform.">
    <div class="menu" id="menu" role="menu" hidden></div>
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
    <span>Goby {version} · GPL-3.0</span>
    <a href="https://github.com/Sadykhzadeh/goby">GitHub</a>
    <a href="https://github.com/Sadykhzadeh/goby/releases">All releases</a>
    <a class="credit" href="https://azer.one">done with <span aria-hidden="true" class="heart">♥</span> by azer.one</a>
  </div>
  <span class="word" aria-hidden="true">goby</span>
</footer>
</body>
</html>
"##;
