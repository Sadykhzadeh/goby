// The hero's wave: it eases from one voice's waveform to the next. Frames are drawn only
// while a change is in flight, and the voices step on only while the hero is on screen.
const wave = document.getElementById("wave"), echo = document.getElementById("echo");
const nameEl = document.getElementById("name"), countEl = document.getElementById("count");
const button = document.getElementById("voice"), hero = document.querySelector(".hero");
const W = 1200, H = 260;
const still = matchMedia("(prefers-reduced-motion: reduce)").matches;

function path(p, amp) {
  const step = W / (p.length - 1), y = v => (H / 2 - v * amp).toFixed(1);
  let d = `M0 ${y(p[0])}`;
  for (let i = 1; i < p.length; i++) {
    const x0 = (i - 1) * step, x1 = i * step, c = step / 2;
    d += ` C${(x0 + c).toFixed(1)} ${y(p[i - 1])} ${(x1 - c).toFixed(1)} ${y(p[i])} ${x1.toFixed(1)} ${y(p[i])}`;
  }
  return d;
}

let at = Math.max(0, VOICES.findIndex(v => v[0] === "Minion"));
let shown = VOICES[at][1].slice(), from = shown, to = shown, t0 = 0, running = false;

function draw(k) {
  const e = 1 - Math.pow(1 - Math.min(1, k), 3);
  shown = to.map((v, i) => from[i] + (v - from[i]) * e);
  wave.setAttribute("d", path(shown, H * 0.36));
  echo.setAttribute("d", path(shown, H * 0.44));
}

function frame(now) {
  const k = (now - t0) / 650;
  draw(k);
  running = k < 1;
  if (running) requestAnimationFrame(frame);
}

function show(i) {
  at = (i + VOICES.length) % VOICES.length;
  nameEl.textContent = VOICES[at][0];
  countEl.textContent = `${at + 1} / ${VOICES.length}`;
  from = shown; to = VOICES[at][1]; t0 = performance.now();
  if (still) return draw(1);
  if (!running) { running = true; requestAnimationFrame(frame); }
}

let lastClick = 0, visible = true;
button.addEventListener("click", () => { lastClick = performance.now(); show(at + 1); });
new IntersectionObserver(([e]) => { visible = e.isIntersecting; }).observe(hero);
draw(1);
if (!still) {
  setInterval(() => {
    if (visible && !document.hidden && performance.now() - lastClick > 6000) show(at + 1);
  }, 2600);
}
