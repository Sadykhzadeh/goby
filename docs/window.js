// Goby's own window, from the app's UI code built for the web (goby.wasm, site/dsp-wasm).
// This file only draws what the wasm asks for on a canvas and passes input back to it.
const TEXT = '"Segoe UI Variable Text", "Segoe UI", system-ui, -apple-system, sans-serif';
const DISPLAY = '"Segoe UI Variable Display", "Segoe UI", system-ui, -apple-system, sans-serif';
const FONTS = [`12px ${TEXT}`, `14px ${TEXT}`, `600 14px ${TEXT}`, `600 20px ${DISPLAY}`, `600 28px ${DISPLAY}`];
const KEYS = ["ArrowLeft", "ArrowRight", "ArrowUp", "ArrowDown", "Home", "End", "Escape", "Backspace"];
const HOTKEYS = { KeyG: 0, PageDown: 1, PageUp: 2 };
const LINKS = [, "https://azer.one", "https://vb-audio.com/Cable/"];
const BG = "#201e1b"; // Mica over a warm wallpaper, as near as a flat colour gets
const $ = id => document.getElementById(id);
const T = JSON.parse($("texts").textContent); // the page's texts, in its language

let x, bytes, frameEl, canvas, ctx, menuEl, audio = null;
let W = 1000, H = 680, k = 1, running = false, wantsLive = false, sent = "", lastLatency = 0;
const dec = new TextDecoder(), colors = new Map(), paths = new Map(), fits = new Map();
const str = (p, n) => dec.decode(new Uint8Array(x.memory.buffer, p, n));
const css = v => {
  let c = colors.get(v);
  if (!c) colors.set(v, c = `rgba(${v >>> 24},${v >> 16 & 255},${v >> 8 & 255},${(v & 255) / 255})`);
  return c;
};
// DirectWrite cuts a line that does not fit with an ellipsis; so does this.
function fit(s, w) {
  const key = ctx.font + w + s;
  let out = fits.get(key);
  if (out === undefined) {
    out = s;
    if (ctx.measureText(s).width > w + 0.5) {
      let lo = 0, hi = s.length;
      while (lo < hi) {
        const mid = (lo + hi + 1) >> 1;
        if (ctx.measureText(s.slice(0, mid) + "…").width <= w) lo = mid; else hi = mid - 1;
      }
      out = s.slice(0, lo).trimEnd() + "…";
    }
    if (fits.size > 800) fits.clear();
    fits.set(key, out);
  }
  return out;
}

const env = {
  begin() {
    ctx.setTransform(k, 0, 0, k, 0, 0);
    ctx.fillStyle = BG;
    ctx.fillRect(0, 0, W, H);
  },
  fill(l, t, r, b, rad, c) {
    ctx.fillStyle = css(c);
    ctx.beginPath();
    ctx.roundRect(l, t, r - l, b - t, rad);
    ctx.fill();
  },
  stroke(l, t, r, b, rad, c, w) {
    ctx.strokeStyle = css(c);
    ctx.lineWidth = w;
    ctx.beginPath();
    ctx.roundRect(l + w / 2, t + w / 2, r - l - w, b - t - w, rad);
    ctx.stroke();
  },
  circle(cx, cy, rad, c) {
    ctx.fillStyle = css(c);
    ctx.beginPath();
    ctx.arc(cx, cy, rad, 0, 7);
    ctx.fill();
  },
  line(p, n, c, w) {
    const a = new Float32Array(x.memory.buffer, p, n * 2);
    ctx.beginPath();
    ctx.moveTo(a[0], a[1]);
    for (let i = 2; i < n * 2; i += 2) ctx.lineTo(a[i], a[i + 1]);
    ctx.strokeStyle = css(c);
    ctx.lineWidth = w;
    ctx.lineCap = ctx.lineJoin = "round";
    ctx.stroke();
  },
  text(p, n, f, l, t, r, b, align, c) {
    ctx.font = FONTS[f];
    ctx.fillStyle = css(c);
    ctx.textBaseline = "middle";
    ctx.textAlign = ["left", "center", "right"][align];
    ctx.fillText(fit(str(p, n), r - l), [l, (l + r) / 2, r][align], (t + b) / 2 + 1);
  },
  shape(p, n, filled, l, t, size, c) {
    let path = paths.get(p);
    if (!path) paths.set(p, path = new Path2D(str(p, n)));
    ctx.save();
    ctx.translate(l, t);
    ctx.scale(size / 16, size / 16);
    if (filled) {
      ctx.fillStyle = css(c);
      ctx.fill(path);
    } else {
      ctx.strokeStyle = css(c);
      ctx.lineWidth = 16 / size;
      ctx.lineCap = ctx.lineJoin = "round";
      ctx.stroke(path);
    }
    ctx.restore();
  },
  measure(p, n, f) {
    ctx.font = FONTS[f];
    return ctx.measureText(str(p, n)).width;
  },
  clip(l, t, r, b) {
    ctx.save();
    ctx.beginPath();
    ctx.rect(l, t, r - l, b - t);
    ctx.clip();
  },
  unclip: () => ctx.restore(),
  shift(dy, o) {
    ctx.save();
    ctx.translate(0, dy);
    ctx.globalAlpha = o;
  },
  unshift: () => ctx.restore(),
  // The app opens a native menu here; the page opens its own, under the same card.
  menu(p, n, checked, mx, my) {
    menuEl.textContent = "";
    str(p, n).split("\n").filter(Boolean).forEach((label, i) => {
      const b = document.createElement("button");
      b.type = "button";
      b.setAttribute("role", "menuitemradio");
      b.setAttribute("aria-checked", i === checked);
      const [name, note] = label.split("\t");
      b.textContent = name;
      if (note) b.append(Object.assign(document.createElement("small"), { textContent: note }));
      b.addEventListener("click", () => closeMenu(i));
      menuEl.append(b);
    });
    if (!menuEl.children.length) return x.ui_menu(-1);
    const s = canvas.clientWidth / W;
    menuEl.hidden = false;
    menuEl.style.left = Math.min(mx * s, canvas.clientWidth - menuEl.offsetWidth - 8) + "px";
    menuEl.style.top = canvas.offsetTop + Math.min(my * s, canvas.clientHeight - menuEl.offsetHeight) + "px";
    menuEl.firstChild.focus();
  },
};

function closeMenu(i) {
  if (menuEl.hidden) return;
  menuEl.hidden = true;
  x.ui_menu(i);
  canvas.focus();
  after();
}

function size() {
  const cssW = frameEl.clientWidth;
  W = Math.max(820, cssW);
  H = Math.max(560, Math.round(W * 0.68));
  const s = cssW / W, zoom = visualViewport?.scale || 1;
  k = s * devicePixelRatio * zoom;
  canvas.style.height = H * s + "px";
  canvas.width = Math.round(W * k);
  canvas.height = Math.round(H * k);
  x.ui_resize(W, H);
  kick();
}

function loop(now) {
  let heard = false;
  if (audio) {
    const n = x.ui_scope_len(), view = new Float32Array(x.memory.buffer, x.ui_heard(), 2 * n);
    audio.scope(view.subarray(0, n), view.subarray(n));
    heard = true;
    if (now - lastLatency > 500) {
      lastLatency = now;
      x.ui_latency(audio.latency());
    }
  }
  if (x.ui_frame(now, heard) || audio) requestAnimationFrame(loop);
  else running = false;
}
function kick() {
  if (!running) {
    running = true;
    requestAnimationFrame(loop);
  }
}

// After every event: draw, follow a link, and start, stop or retune the voice.
function after() {
  kick();
  const wants = x.ui_wants(), link = LINKS[wants >> 8];
  if (link) window.open(link, "_blank", "noopener");
  if (!!(wants & 1) !== wantsLive) live(wantsLive = !wantsLive);
  const p = [...new Float32Array(x.memory.buffer, x.ui_params(), 7)];
  if (audio && p.join() !== sent) {
    sent = p.join();
    audio.set({ voice: p[0], tone: p[1], volume: p[2], gain: p[3], gate: p[4], mix: p[5], enabled: p[6] });
  }
}

// The voices on the visitor's microphone, or on a recording of speech, through the same
// wasm in an AudioWorklet. One source plays at a time: `source` says which, and `run`
// numbers the starts, so one that is stopped or replaced while it is still starting
// shuts itself down instead of playing.
const tryBtn = $("try"), tryLabel = $("try-label"), note = $("try-note"), note0 = note.innerHTML;
const sampleBtn = $("sample"), sampleLabel = $("sample-label");
let source = "mic", pending = null, run = 0;

function ui() {
  const mic = wantsLive && source === "mic", playing = wantsLive && source === "sample";
  tryBtn.setAttribute("aria-pressed", mic);
  tryLabel.textContent = mic ? T["try.on"] : T["try.off"];
  sampleBtn.classList.toggle("on", playing);
  if (!sampleBtn.disabled) sampleLabel.textContent = playing ? T["sample.stop"] : T["sample.play"];
}

async function live(on) {
  if (on) {
    source = pending ?? "mic";
    pending = null;
  } else {
    run++;
    audio?.stop();
    audio = null;
    source = "mic";
    note.innerHTML = note0;
  }
  ui();
  if (on) await begin();
}

// Starts the source, and stops the one playing. Called inside a click, so the audio
// context made here is allowed to play.
async function begin() {
  const mine = ++run;
  audio?.stop();
  audio = null;
  if (!window.AudioWorkletNode || (source === "mic" && !navigator.mediaDevices?.getUserMedia)) return failed(T.unsupported);
  const ac = new AudioContext({ latencyHint: "interactive" });
  note.textContent = source === "mic" ? T.asking : T["sample.note"];
  try {
    const started = await (await import("./live.js")).start(ac, bytes, source === "sample" ? sampleBuf : null);
    if (mine !== run) return started.stop();
    audio = started;
    sent = "";
    after();
    note.textContent = source === "mic" ? T.listening : T["sample.note"];
  } catch (err) {
    ac.close();
    if (mine !== run) return;
    failed(err?.name === "NotAllowedError" ? T.denied
      : err?.name === "NotFoundError" ? T.nomic
      : T.failed + (err?.message || err));
  }
}
function failed(text) {
  x.ui_monitor(false);
  after();
  note.textContent = text;
}

function monitor(on) {
  x.ui_monitor(on);
  after();
}

// The microphone button: it starts the microphone, or takes it over from the sample.
export function toggle() {
  if (wantsLive && source === "sample") {
    source = "mic";
    ui();
    return begin();
  }
  monitor(!wantsLive);
}

// The recording: a few seconds of speech, decoded once. Its ends fade in and out and a
// short silence follows, so it loops without a click and the voices' tails die away
// before it comes round again. Without it, or if it cannot be decoded, the button says so.
let sampleBuf = null, sampleReady;
function loadSample() {
  return sampleReady ??= fetch(new URL("sample.mp3", import.meta.url))
    .then(r => r.ok ? r.arrayBuffer() : Promise.reject(new Error("sample.mp3: " + r.status)))
    .then(bytes => new OfflineAudioContext(1, 1, 48000).decodeAudioData(bytes))
    .then(speech => {
      const rate = speech.sampleRate, fade = Math.round(rate * 0.015);
      const buf = new AudioBuffer({ length: speech.length + Math.round(rate * 0.4), sampleRate: rate });
      const d = buf.getChannelData(0);
      d.set(speech.getChannelData(0));
      for (let i = 0; i < fade; i++) {
        d[i] *= i / fade;
        d[speech.length - 1 - i] *= i / fade;
      }
      return sampleBuf = buf;
    });
}
function checkSample() {
  loadSample().catch(() => {
    sampleBtn.disabled = true;
    sampleLabel.textContent = T["sample.none"];
  });
}

export async function sample() {
  if (!sampleBuf) {
    try { await loadSample(); } catch { return; }
  }
  if (!wantsLive) {
    pending = "sample";
    monitor(true);
  } else if (source === "sample") {
    monitor(false);
  } else {
    source = "sample";
    ui();
    begin();
  }
}

export async function open(wasm) {
  bytes = wasm;
  const imports = { env };
  x = (await WebAssembly.instantiate(bytes, imports)).instance.exports;
  frameEl = $("win");
  menuEl = $("menu");
  canvas = document.createElement("canvas");
  canvas.tabIndex = 0;
  canvas.setAttribute("role", "application");
  canvas.setAttribute("aria-label", T.window);
  ctx = canvas.getContext("2d");
  await document.fonts?.ready;
  // The window starts in the language of the page, not of the browser.
  const language = new TextEncoder().encodeInto(document.documentElement.lang,new Uint8Array(x.memory.buffer, x.ui_language(), 35));
  x.ui_start(1000, 680, matchMedia("(prefers-reduced-motion: reduce)").matches, language.written);
  frameEl.querySelector(".shot").replaceWith(canvas);
  size();
  new ResizeObserver(size).observe(frameEl);
  visualViewport?.addEventListener("resize", size);
  checkSample();
  // A context a browser left suspended starts with the next touch.
  document.addEventListener("pointerdown", () => audio?.resume(), true);

  const at = e => {
    const r = canvas.getBoundingClientRect(), s = r.width / W;
    return [(e.clientX - r.left) / s, (e.clientY - r.top) / s];
  };
  let lastDown = 0, lastAt = [0, 0], touch = null;
  canvas.addEventListener("pointerdown", e => {
    if (e.button !== 0) return;
    const p = at(e);
    if (e.pointerType !== "mouse" && !x.ui_draggable(...p)) {
      touch = { p, moved: false }; // a tap, unless it turns into a scroll
      return;
    }
    const double = e.timeStamp - lastDown < 500 && Math.hypot(p[0] - lastAt[0], p[1] - lastAt[1]) < 4;
    [lastDown, lastAt] = [e.timeStamp, p];
    canvas.setPointerCapture(e.pointerId);
    x.ui_move(...p);
    x.ui_down(...p, double);
    after();
  });
  canvas.addEventListener("pointermove", e => {
    if (touch) return;
    x.ui_move(...at(e));
    kick();
    if (e.buttons) after();
  });
  canvas.addEventListener("pointerup", e => {
    if (touch) {
      const { p, moved } = touch;
      touch = null;
      if (moved) return;
      x.ui_move(...p);
      x.ui_down(...p, false);
    }
    x.ui_up();
    after();
  });
  canvas.addEventListener("pointercancel", () => { touch = null; x.ui_up(); after(); });
  canvas.addEventListener("pointerleave", e => { if (e.pointerType === "mouse") { x.ui_leave(); kick(); } });
  canvas.addEventListener("contextmenu", e => {
    e.preventDefault();
    touch = null;
    x.ui_right(...at(e));
    after();
  });
  canvas.addEventListener("wheel", e => {
    const p = at(e), dy = -e.deltaY * (e.deltaMode ? 40 : 1) * W / canvas.clientWidth;
    if (x.ui_drag_scroll(...p, dy)) {
      e.preventDefault();
      kick();
    }
  }, { passive: false });
  // A finger scrolls the pane under it while it can, and the page after that.
  let lastY = 0, own = null;
  // A slider or scrollbar under the finger (pointerdown comes first) keeps the whole gesture.
  canvas.addEventListener("touchstart", e => { lastY = e.touches[0].clientY; own = touch ? null : false; }, { passive: true });
  canvas.addEventListener("touchmove", e => {
    const y = e.touches[0].clientY, dy = (y - lastY) * W / canvas.clientWidth;
    lastY = y;
    if (touch) touch.moved = true;
    if (own === false) return;
    const used = x.ui_drag_scroll(...at(e.touches[0]), dy);
    own ??= used;
    if (own && e.cancelable) {
      e.preventDefault();
      kick();
    }
  }, { passive: false });
  canvas.addEventListener("keydown", e => {
    const i = KEYS.indexOf(e.key);
    if (i >= 0 && !e.ctrlKey && !e.altKey && x.ui_key(i)) {
      e.preventDefault();
      after();
    }
  });
  // The app's hotkeys work here too, while its switch for them is on.
  document.addEventListener("keydown", e => {
    if (e.ctrlKey && e.altKey && e.code in HOTKEYS && x.ui_hotkey(HOTKEYS[e.code])) {
      e.preventDefault();
      after();
    }
  });
  document.addEventListener("pointerdown", e => { if (!menuEl.hidden && !menuEl.contains(e.target)) closeMenu(-1); }, true);
  menuEl.addEventListener("keydown", e => {
    const items = [...menuEl.children], i = items.indexOf(document.activeElement);
    if (e.key === "Escape") closeMenu(-1);
    else if (e.key === "ArrowDown" || e.key === "ArrowUp") {
      e.preventDefault();
      items[(i + (e.key === "ArrowDown" ? 1 : items.length - 1)) % items.length]?.focus();
    }
  });
}
