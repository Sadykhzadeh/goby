// The hero's wave, and loading Goby's window when it comes near. Frames are drawn only
// while something is changing.
const $ = id => document.getElementById(id);
const still = matchMedia("(prefers-reduced-motion: reduce)").matches;
// The page's texts in its language, and where the other files are: the Russian page is
// one folder down.
const TEXT = JSON.parse($("texts").textContent);
const HERE = document.currentScript.src;

// Every voice's name and waveform, as the app draws it on its tile.
const VOICES = JSON.parse($("hero-voices").textContent).map(([name, pts]) => ({ name, pts: pts.map(v => v / 100) }));

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
  let at = Math.max(0, VOICES.findIndex(v => v.name === nameEl.textContent));
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
      if (visible && !document.hidden && !hero.matches(":hover, :focus-within") && performance.now() - lastClick > 6000) show(at + 1);
    }, 2600);
  }
}

// ---- The window: the app's own UI, built for the web. Fetched once it is about to be seen,
// or when someone asks to hear the voices; window.js swaps it in for the picture. ----
let loading;
const load = () => loading ??= fetch(new URL("goby.wasm", HERE))
  .then(r => r.ok ? r.arrayBuffer() : Promise.reject(new Error("goby.wasm: " + r.status)))
  .then(async bytes => {
    const w = await import(new URL("window.js", HERE));
    await w.open(bytes);
    return w;
  })
  // A failed load is forgotten, so the next click or visit asks for the files again.
  .catch(err => { loading = undefined; throw err; });
new IntersectionObserver((seen, io) => {
  if (seen.some(e => e.isIntersecting)) { io.disconnect(); load().catch(() => {}); }
}, { rootMargin: "600px" }).observe($("win"));
const loadFailed = err => { $("try-note").textContent = TEXT.loadfail + err.message; };
$("try").addEventListener("click", () => load().then(w => w.toggle(), loadFailed));
$("sample").addEventListener("click", () => load().then(w => w.sample(), loadFailed));
