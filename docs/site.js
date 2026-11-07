// The hero's wave, and loading Goby's window when it comes near. Frames are drawn only
// while something is changing.
const $ = id => document.getElementById(id);
const still = matchMedia("(prefers-reduced-motion: reduce)").matches;

// Every voice's name and waveform, as the app draws it on its tile.
const VOICES = JSON.parse($("hero-voices").textContent).map(([name, pts]) => ({ name, pts }));

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

// ---- The window: the app's own UI, built for the web. Fetched once it is about to be seen,
// or when someone asks to hear the voices; window.js swaps it in for the picture. ----
let loading;
const load = () => loading ??= fetch("goby.wasm")
  .then(r => r.ok ? r.arrayBuffer() : Promise.reject(new Error("goby.wasm: " + r.status)))
  .then(async bytes => {
    const w = await import("./window.js");
    await w.open(bytes);
    return w;
  });
new IntersectionObserver((seen, io) => {
  if (seen.some(e => e.isIntersecting)) { io.disconnect(); load().catch(() => {}); }
}, { rootMargin: "600px" }).observe($("win"));
$("try").addEventListener("click", () => load().then(w => w.toggle(), err => {
  $("try-note").textContent = "The voices could not load here: " + err.message;
}));
