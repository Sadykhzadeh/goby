// Tests for site/check.mjs: each case breaks one rule on a small two-language site and
// expects the check to name it. Run with `node --test site/`.
import { spawnSync } from "node:child_process";
import { test } from "node:test";
import assert from "node:assert/strict";
import { mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import vm from "node:vm";
import { fileURLToPath } from "node:url";
import { anchorsOf, cargoCommandsIn, checkMarkdown, checkSite, linksIn, namesIn, parse, proseLines, repoPathsIn, slug } from "./check.mjs";

const page = ({ lang = "en", body = "", head = "" } = {}) => {
  const root = lang === "ru" ? "../" : "";
  return `<!doctype html>
<html lang="${lang}">
<head>
<meta charset="utf-8">
<meta http-equiv="Content-Security-Policy" content="default-src 'none'; style-src 'self'">
<title>Goby</title>
<meta name="description" content="A voice changer.">
<link rel="canonical" href="https://goby.azer.one/${lang === "ru" ? "ru/" : ""}">
<link rel="alternate" hreflang="en" href="https://goby.azer.one/">
<link rel="alternate" hreflang="ru" href="https://goby.azer.one/ru/">
<link rel="alternate" hreflang="x-default" href="https://goby.azer.one/">
<link rel="stylesheet" href="${root}site.css">
${head}
</head>
<body>
<h1>Goby</h1>
<h2>Voices</h2>
<img src="${root}images/logo.svg" alt="" width="26" height="26">
<a href="#voices">Voices</a>
<section id="voices"></section>
<button type="button">Try</button>
${body}
</body>
</html>
`;
};

const base = () => ({
  "index.html": page(),
  "ru/index.html": page({ lang: "ru" }),
  "site.css": "body { color: red; }",
  "images/logo.svg": "<svg/>",
});

function site(files) {
  const dir = mkdtempSync(join(tmpdir(), "goby-site-check-"));
  try {
    for (const [name, text] of Object.entries(files)) {
      mkdirSync(dirname(join(dir, name)), { recursive: true });
      writeFileSync(join(dir, name), text);
    }
    return checkSite(dir);
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
}

test("a clean two-language site passes", () => {
  assert.deepEqual(site(base()), []);
});

test("the parser keeps attribute values and drops comments", () => {
  const [html] = parse('<!-- <a href="x"> --><p class=a title=\'b c\'>t</p>').children.filter(c => typeof c !== "string");
  assert.equal(html.tag, "p");
  assert.equal(html.attrs.class, "a");
  assert.equal(html.attrs.title, "b c");
});

// Each case edits the clean site and must be named by the check.
const cases = [
  ["a link to a missing page", f => { f["index.html"] = page({ body: '<a href="nope.html">x</a>' }); }, /names a missing file/],
  ["an anchor that is not on the page", f => { f["index.html"] = page({ body: '<a href="#nowhere">x</a>' }); }, /anchor that does not exist/],
  ["a link to a missing page of this site", f => { f["index.html"] = page({ body: '<a href="https://goby.azer.one/gone/">x</a>' }); }, /names a missing file/],
  ["an image file that is missing", f => { f["index.html"] = page({ body: '<img src="images/none.webp" alt="" width="1" height="1">' }); }, /names a missing file/],
  ["a srcset candidate that is missing", f => { f["index.html"] = page({ body: '<img src="images/logo.svg" srcset="images/x.webp 2x" alt="" width="1" height="1">' }); }, /names a missing file/],
  ["a stylesheet that is missing", f => { f["index.html"] = page({ head: '<link rel="stylesheet" href="none.css">' }); }, /names a missing file/],
  ["a url() in a stylesheet that is missing", f => { f["site.css"] = "a{background:url(gone.png)}"; }, /url\(gone\.png\) names a missing file/],
  ["a url() in a style element that is missing", f => { f["index.html"] = page({ head: "<style>a{background:url('gone.png')}</style>" }); }, /names a missing file/],
  ["an image without alt", f => { f["index.html"] = page({ body: '<img src="images/logo.svg" width="26" height="26">' }); }, /has no alt/],
  ["an image without size", f => { f["index.html"] = page({ body: '<img src="images/logo.svg" alt="">' }); }, /no width and height/],
  ["a duplicate id", f => { f["index.html"] = page({ body: '<p id="voices">x</p>' }); }, /duplicate id "voices"/],
  ["a second h1", f => { f["index.html"] = page({ body: "<h1>Again</h1>" }); }, /expected one <h1>, found 2/],
  ["a skipped heading level", f => { f["index.html"] = page({ body: "<h4>Deep</h4>" }); }, /<h4> follows a level 2/],
  ["no lang on the html element", f => { f["index.html"] = page().replace('lang="en"', ""); }, /<html> has no lang/],
  ["no title", f => { f["index.html"] = page().replace("<title>Goby</title>", ""); }, /expected one non-empty <title>, found 0/],
  ["two titles", f => { f["index.html"] = page({ head: "<title>Again</title>" }); }, /expected one non-empty <title>, found 2/],
  ["no meta description", f => { f["index.html"] = page().replace(/<meta name="description"[^>]*>/, ""); }, /meta description, found 0/],
  ["no canonical link", f => { f["index.html"] = page().replace(/<link rel="canonical"[^>]*>/, ""); }, /canonical link, found 0/],
  ["a canonical that names another page", f => { f["index.html"] = page().replace('href="https://goby.azer.one/"', 'href="https://goby.azer.one/ru/"'); }, /canonical .* is not this page/],
  ["no Russian alternate", f => { f["index.html"] = page().replace(/<link rel="alternate" hreflang="ru"[^>]*>/, ""); }, /no hreflang="ru" alternate/],
  ["an alternate that does not point back", f => { f["ru/index.html"] = page({ lang: "ru" }).replace('hreflang="en" href="https://goby.azer.one/"', 'hreflang="en" href="https://goby.azer.one/ru/"'); }, /does not point back/],
  ["an alternate that names a page in the wrong language", f => { f["index.html"] = page().replace('hreflang="ru" href="https://goby.azer.one/ru/"', 'hreflang="ru" href="https://goby.azer.one/"'); }, /names a page in lang="en"/],
  ["a button without a name", f => { f["index.html"] = page({ body: '<button type="button"><svg></svg></button>' }); }, /has no accessible name/],
  ["an input without a label", f => { f["index.html"] = page({ body: '<input id="q" type="text">' }); }, /has no label/],
  ["a viewport that caps zoom", f => { f["index.html"] = page({ head: '<meta name="viewport" content="width=device-width, maximum-scale=1">' }); }, /viewport stops pinch zoom/],
  ["a viewport that turns zoom off", f => { f["index.html"] = page({ head: '<meta name="viewport" content="width=device-width, user-scalable=no">' }); }, /viewport stops pinch zoom/],
  ["no Content-Security-Policy", f => { f["index.html"] = page().replace(/<meta http-equiv="Content-Security-Policy"[^>]*>/, ""); }, /no Content-Security-Policy meta/],
  ["a policy that allows inline script", f => { f["index.html"] = page().replace("default-src 'none'", "default-src 'none'; script-src 'self' 'unsafe-inline'"); }, /allows unsafe inline/],
  ["an inline style the policy does not hash", f => { f["index.html"] = page({ head: "<style>p{color:red}</style>" }); }, /inline <style> is not allowed/],
  ["a blank-target link without noopener", f => { f["index.html"] = page({ body: '<a href="https://example.com/" target="_blank">x</a>' }); }, /lacks rel="noopener"/],
];

for (const [name, edit, expected] of cases) {
  test(`the check names ${name}`, () => {
    const files = base();
    edit(files);
    const found = site(files);
    assert.ok(found.some(f => expected.test(f)), `expected ${expected}, got:\n${found.join("\n")}`);
  });
}

test("the built site in docs/ passes", () => {
  const found = checkSite(fileURLToPath(new URL("../docs", import.meta.url)));
  assert.deepEqual(found, []);
});

// The two pages are one template in two languages: they must carry the same figures in the same
// order, and every figure with a unit must also be in the README, which the numbers come from.
const docs = fileURLToPath(new URL("../docs", import.meta.url));
const visibleText = html => {
  const root = parse(html);
  const out = [];
  const walk = node => {
    for (const c of node.children) {
      if (typeof c === "string") out.push(c);
      else if (c.tag !== "script" && c.tag !== "style") {
        if (c.tag === "meta" && c.attrs.name === "description") out.push(c.attrs.content);
        if (c.tag === "img" && c.attrs.alt) out.push(c.attrs.alt);
        walk(c);
      }
    }
  };
  walk(root);
  return out.join(" ");
};

test("the English and Russian pages carry the same figures in the same order", () => {
  const en = visibleText(readFileSync(join(docs, "index.html"), "utf8")).match(/\d+(?:[.,]\d+)?/g);
  const ru = visibleText(readFileSync(join(docs, "ru", "index.html"), "utf8")).match(/\d+(?:[.,]\d+)?/g);
  assert.ok(en.length > 0);
  assert.deepEqual(ru, en);
});

test("every figure with a unit on the English page is also in the README", () => {
  const readme = readFileSync(join(docs, "..", "README.md"), "utf8");
  const figures = visibleText(readFileSync(join(docs, "index.html"), "utf8")).match(/\d[\d.,]*\s?(?:%|ms|KB|voices)/g);
  assert.ok(figures.length > 0);
  for (const f of new Set(figures)) assert.ok(readme.includes(f), `${f} is not in the README`);
});

test("the hero's waveforms are 74 voices of 60 points, in whole hundredths", () => {
  const html = readFileSync(join(docs, "index.html"), "utf8");
  const voices = JSON.parse(html.match(/id="hero-voices">([\s\S]*?)<\/script>/)[1]);
  assert.equal(voices.length, 74);
  for (const [name, points] of voices) {
    assert.ok(name);
    assert.equal(points.length, 60, name);
    for (const p of points) assert.ok(Number.isInteger(p) && p >= -100 && p <= 100, `${name}: ${p}`);
  }
});

// The page's script, run against a stub of the few DOM calls it makes at load. It shows the hero's
// voices and easing, and that the window (goby.wasm) is asked for only when it nears the screen.
function runSiteScript(html, { reduced = false, hovered = false, clock = 0 } = {}) {
  const doc = fileURLToPath(new URL("../docs/index.html", import.meta.url));
  const page = readFileSync(doc, "utf8");
  const listeners = {};
  const el = id => ({
    id,
    textContent: "",
    setAttribute() {},
    addEventListener: (type, fn) => (listeners[`${id} ${type}`] ??= []).push(fn),
    querySelector: () => el("x"),
    matches: () => false,
  });
  const elements = {
    texts: { textContent: page.match(/id="texts">([\s\S]*?)<\/script>/)[1] },
    "hero-voices": { textContent: page.match(/id="hero-voices">([\s\S]*?)<\/script>/)[1] },
    name: Object.assign(el("name"), { textContent: "Minion" }),
    rotate: Object.assign(el("rotate"), { hidden: false }),
  };
  const observed = [];
  const fetches = [];
  const intervals = [];
  const ctx = {
    document: {
      currentScript: { src: "https://goby.azer.one/site.js" },
      getElementById: id => elements[id] ?? el(id),
      querySelector: () => Object.assign(el("hero"), { matches: sel => hovered && sel === ":hover, :focus-within" }),
    },
    matchMedia: () => ({ matches: reduced }),
    IntersectionObserver: class {
      constructor(cb) {
        this.cb = cb;
      }
      observe(target) {
        observed.push({ target, cb: this.cb, io: this });
      }
      disconnect() {}
    },
    requestAnimationFrame: () => 0,
    setInterval: fn => intervals.push(fn),
    performance: { now: () => clock },
    fetch: url => {
      fetches.push(String(url));
      return Promise.resolve({ ok: false, status: 404 });
    },
    URL,
    console,
  };
  vm.createContext(ctx);
  vm.runInContext(html, ctx, { filename: "site.js" });
  return { ctx, observed, fetches, intervals, listeners, elements };
}

test("the page's script shows the 74 voices in hundredths decoded and eases between them", () => {
  const siteJs = readFileSync(join(docs, "site.js"), "utf8");
  const { ctx } = runSiteScript(siteJs);
  const voices = vm.runInContext("VOICES", ctx);
  assert.equal(voices.length, 74);
  assert.equal(voices[1].name, "Minion");
  assert.equal(voices[1].pts[0], -0.68);
  assert.equal(vm.runInContext("ease(0)", ctx), 0);
  assert.equal(vm.runInContext("ease(1)", ctx), 1);
  assert.equal(vm.runInContext("ease(0.5)", ctx), 0.875);
  assert.equal(vm.runInContext("ease(2)", ctx), 1);
  assert.equal(vm.runInContext("smooth([0, 0], 100, 50, 10)", ctx), "M0 25.0C50.0 25.0 50.0 25.0 100.0 25.0");
});

test("the window and its wasm are not requested until the window nears the screen", () => {
  const siteJs = readFileSync(join(docs, "site.js"), "utf8");
  const { observed, fetches } = runSiteScript(siteJs);
  assert.equal(fetches.length, 0);
  const win = observed.find(o => o.target.id === "win");
  assert.ok(win, "the window is observed");
  win.cb([{ isIntersecting: false }], win.io);
  assert.equal(fetches.length, 0);
  win.cb([{ isIntersecting: true }], win.io);
  assert.equal(fetches.length, 1);
  assert.match(fetches[0], /goby\.wasm$/);
});

test("a window that failed to load is asked for again on the next try", async () => {
  const siteJs = readFileSync(join(docs, "site.js"), "utf8");
  const { observed, fetches, listeners } = runSiteScript(siteJs);
  const win = observed.find(o => o.target.id === "win");
  win.cb([{ isIntersecting: true }], win.io);
  await new Promise(done => setImmediate(done));
  assert.equal(fetches.length, 1);
  for (const fn of listeners["try click"]) fn();
  assert.equal(fetches.length, 2, "the failed load stays cached and the click never asks again");
});

// The window's script is a module; its exports are stripped so it runs as a plain script against
// stubs of the few elements it reads when it loads.
test("a sample that failed to load is asked for again on the next press", async () => {
  const source = readFileSync(join(docs, "window.js"), "utf8")
    .replace(/^export /gm, "")
    .replaceAll("import.meta.url", '"https://goby.azer.one/window.js"');
  const fetches = [];
  const element = id => ({
    id,
    textContent: "",
    innerHTML: "",
    disabled: false,
    classList: { toggle() {} },
    setAttribute() {},
    addEventListener() {},
  });
  const elements = { texts: { textContent: '{"sample.none":"Sample unavailable"}' } };
  const ctx = {
    document: { getElementById: id => (elements[id] ??= element(id)), documentElement: { lang: "en" } },
    fetch: url => {
      fetches.push(String(url));
      return Promise.resolve({ ok: false, status: 503 });
    },
    URL,
    TextDecoder,
    TextEncoder,
    console,
  };
  vm.createContext(ctx);
  vm.runInContext(source, ctx, { filename: "window.js" });
  await vm.runInContext("sample()", ctx);
  assert.equal(fetches.length, 1);
  assert.equal(elements["sample"].disabled, false, "a failed press leaves the button enabled");
  assert.equal(elements["sample-label"].textContent, "Sample unavailable");
  await vm.runInContext("sample()", ctx);
  assert.equal(fetches.length, 2, "the next press asks for the sample again");
  assert.match(fetches[1], /sample\.mp3$/);
  assert.equal(elements["sample"].disabled, false);
});

test("the hero's pause control holds the voices, and a second press turns them again", () => {
  const siteJs = readFileSync(join(docs, "site.js"), "utf8");
  const run = runSiteScript(siteJs, { clock: 10_000 });
  const name = () => run.elements.name.textContent;
  const press = () => run.listeners["rotate click"][0]();
  assert.equal(run.elements.rotate.hidden, false);
  press();
  assert.equal(run.elements.rotate.textContent, "Resume the voices");
  const held = name();
  run.intervals[0]();
  assert.equal(name(), held, "a paused hero does not turn over");
  press();
  assert.equal(run.elements.rotate.textContent, "Pause the voices");
  run.intervals[0]();
  assert.notEqual(name(), held, "a resumed hero turns over again");
  const still = runSiteScript(siteJs, { reduced: true });
  assert.equal(still.elements.rotate.hidden, true, "with reduced motion there is nothing to pause");
});

test("the hero cycles on its own, and holds still while the pointer or the keyboard is on it", () => {
  const siteJs = readFileSync(join(docs, "site.js"), "utf8");
  const next = { name: vm.runInContext("VOICES", runSiteScript(siteJs).ctx)[2].name };
  const idle = runSiteScript(siteJs, { clock: 10_000 });
  idle.intervals[0]();
  assert.equal(idle.elements.name.textContent, next.name);
  const on = runSiteScript(siteJs, { clock: 10_000, hovered: true });
  on.intervals[0]();
  assert.equal(on.elements.name.textContent, "Minion");
});

test("the live status is announced, and the try button names its state once", () => {
  for (const page of ["index.html", "ru/index.html"]) {
    const html = readFileSync(join(docs, page), "utf8");
    assert.match(html, /<p id="try-note" aria-live="polite">/, page);
    assert.doesNotMatch(html, /aria-pressed/, page);
  }
  assert.doesNotMatch(readFileSync(join(docs, "window.js"), "utf8"), /aria-pressed/);
  assert.match(readFileSync(join(docs, "site.css"), "utf8"), /\.try-btn\.on \{/);
});

test("every rule that removes an outline has a visible focus ring on the same element", () => {
  const css = readFileSync(join(docs, "site.css"), "utf8").replace(/\/\*[\s\S]*?\*\//g, "");
  const rules = [...css.matchAll(/([^{}]+)\{([^{}]*)\}/g)].map(([, selector, body]) => ({ selector, body }));
  const element = selector => selector.replace(/:[a-z-]+(\([^)]*\))?/g, "").trim();
  const ringed = new Set(
    rules
      .filter(r => /outline:\s*(?!none|0\b)/.test(r.body))
      .flatMap(r => r.selector.split(",").filter(s => s.includes(":focus-visible")).map(element)),
  );
  const removed = rules.filter(r => /outline:\s*(none|0)\b/.test(r.body)).flatMap(r => r.selector.split(",").map(element));
  assert.ok(removed.length > 0);
  for (const name of removed) assert.ok(ringed.has(name), `${name} removes its outline with no :focus-visible ring`);
});

test("with reduced motion the hero does not cycle on its own", () => {
  const siteJs = readFileSync(join(docs, "site.js"), "utf8");
  assert.equal(runSiteScript(siteJs).intervals.length, 1);
  assert.equal(runSiteScript(siteJs, { reduced: true }).intervals.length, 0);
});

test("the sitemap lists each page once, with its alternates and the day git last changed it", () => {
  const xml = readFileSync(join(docs, "sitemap.xml"), "utf8");
  const urls = [...xml.matchAll(/<url>([\s\S]*?)<\/url>/g)].map(([, body]) => ({
    loc: body.match(/<loc>(.*?)<\/loc>/)[1],
    lastmod: body.match(/<lastmod>(.*?)<\/lastmod>/)?.[1],
    alternates: [...body.matchAll(/hreflang="([^"]+)" href="([^"]+)"/g)].map(([, lang, href]) => `${lang} ${href}`).sort(),
  }));
  const pages = ["index.html", "ru/index.html"].map(p => readFileSync(join(docs, p), "utf8"));
  const canonicals = pages.map(p => p.match(/<link rel="canonical" href="([^"]+)"/)[1]).sort();
  assert.deepEqual(urls.map(u => u.loc).sort(), canonicals);
  for (const u of urls) {
    assert.match(u.lastmod ?? "", /^\d{4}-\d{2}-\d{2}$/, u.loc);
    assert.deepEqual(u.alternates, [
      "en https://goby.azer.one/",
      "ru https://goby.azer.one/ru/",
      "x-default https://goby.azer.one/",
    ]);
  }
});

test("robots.txt allows everything and points at the sitemap", () => {
  const robots = readFileSync(join(docs, "robots.txt"), "utf8");
  assert.match(robots, /^User-agent: \*$/m);
  assert.match(robots, /^Sitemap: https:\/\/goby\.azer\.one\/sitemap\.xml$/m);
});

test("the structured data names the app, its version and licence as the Cargo.toml and the page do", () => {
  const cargo = readFileSync(join(docs, "..", "Cargo.toml"), "utf8");
  const version = cargo.match(/^version = "(.*)"$/m)[1];
  const license = cargo.match(/^license = "(.*)"$/m)[1];
  for (const [file, lang] of [["index.html", "en"], ["ru/index.html", "ru"]]) {
    const html = readFileSync(join(docs, file), "utf8");
    const ld = JSON.parse(html.match(/<script type="application\/ld\+json">([\s\S]*?)<\/script>/)[1]);
    const description = html.match(/<meta name="description" content="([^"]*)"/)[1];
    assert.equal(ld["@type"], "SoftwareApplication", file);
    assert.equal(ld.softwareVersion, version, file);
    assert.equal(ld.license, `https://spdx.org/licenses/${license}.html`, file);
    assert.equal(ld.fileSize, "374 KB", file);
    assert.equal(ld.description, description, file);
    assert.equal(ld.inLanguage, lang, file);
  }
});

test("the social tags agree with the page and describe the image that exists", () => {
  const png = readFileSync(join(docs, "images", "social.png"));
  const [width, height] = [png.readUInt32BE(16), png.readUInt32BE(20)];
  const meta = (html, prop) => html.match(new RegExp(`<meta property="${prop}" content="([^"]*)"`))?.[1];
  for (const [file, other] of [["index.html", "ru_RU"], ["ru/index.html", "en_US"]]) {
    const html = readFileSync(join(docs, file), "utf8");
    const canonical = html.match(/<link rel="canonical" href="([^"]+)"/)[1];
    assert.equal(meta(html, "og:url"), canonical, file);
    assert.equal(meta(html, "og:title"), html.match(/<title>(.*?)<\/title>/)[1], file);
    assert.equal(meta(html, "og:type"), "website", file);
    assert.equal(meta(html, "og:site_name"), "Goby", file);
    assert.equal(meta(html, "og:locale:alternate"), other, file);
    assert.equal(meta(html, "og:image:width"), String(width), file);
    assert.equal(meta(html, "og:image:height"), String(height), file);
    assert.ok(meta(html, "og:image:alt"), file);
    assert.equal(meta(html, "og:image"), "https://goby.azer.one/images/social.png", file);
  }
});

// ---- The Markdown checks ----

test("heading anchors are made the way GitHub makes them", () => {
  assert.equal(slug("How it works"), "how-it-works");
  assert.equal(slug("Size, and why LTO is off"), "size-and-why-lto-is-off");
  assert.equal(slug("The `grain` shifter"), "the-grain-shifter");
  assert.equal(slug("Read [the docs](x.md)"), "read-the-docs");
  assert.equal(slug("Ctrl+Alt+G"), "ctrlaltg");
  assert.equal(slug("A - B"), "a---b");
  assert.equal(slug("Café *now*"), "café-now");
});

test("a repeated heading gets -1, -2 like GitHub, and code blocks have no headings", () => {
  const md = "# Numbers\n## Numbers\n```\n# not a heading\n```\n### Numbers\n<a id=\"x\"></a>";
  const anchors = anchorsOf(md);
  assert.deepEqual([...anchors].sort(), ["numbers", "numbers-1", "numbers-2", "x"]);
});

test("prose lines know which ones sit inside a fence", () => {
  const lines = proseLines("a\n```sh\ncargo test\n```\nb");
  assert.deepEqual(lines.map(l => l.code), [false, true, true, true, false]);
});

test("links and images are found, and links inside code spans are not", () => {
  assert.deepEqual(linksIn("[a](docs/x.md#y) ![b](images/z.webp \"t\") `[c](no.md)`"), ["docs/x.md#y", "images/z.webp"]);
  assert.deepEqual(linksIn('<img src="docs/a.png"> <a href="LICENSE">'), ["docs/a.png", "LICENSE"]);
  assert.deepEqual(linksIn("[ref]: https://example.com/"), ["https://example.com/"]);
});

test("the path extractor takes slashed paths and source files, not artefacts or prose", () => {
  assert.deepEqual(
    repoPathsIn("in `src/voices.rs`, `dsp.rs`, `lang/en.lang`, `crates/dsp/` and `.cargo/config.toml`"),
    ["src/voices.rs", "dsp.rs", "lang/en.lang", "crates/dsp/", ".cargo/config.toml"],
  );
  assert.deepEqual(repoPathsIn("`goby.exe`, `SHA256SUMS.txt`, `lang=auto`, `cargo test` and `1.88+`"), []);
});

test("cargo lines name their packages, targets and test filters", () => {
  assert.deepEqual(namesIn("cargo test --release -p goby-dsp formant_cost -- --ignored --nocapture"), {
    packages: ["goby-dsp"], tests: [], bins: [], filters: ["formant_cost"],
  });
  assert.deepEqual(namesIn("cargo test --test resources --package=goby"), {
    packages: ["goby"], tests: ["resources"], bins: [], filters: [],
  });
  assert.deepEqual(namesIn("cargo build --release"), { packages: [], tests: [], bins: [], filters: [] });
  assert.deepEqual(cargoCommandsIn("```\ncargo test x # a comment\n```\nand `cargo build`"), ["cargo test x", "cargo build"]);
});

// A small repository: a Cargo package with one test, a README and a docs folder.
function repo(files) {
  const dir = mkdtempSync(join(tmpdir(), "goby-md-check-"));
  const base = {
    "Cargo.toml": 'name = "goby"\nversion = "1.0.0"\n',
    "src/main.rs": "fn main() {}\n#[test]\nfn sample_check() {}\n",
    "tests/resources.rs": "",
    "docs/guide.md": "# Guide\n## Part one\n",
    "images/a.webp": "",
    "README.md": "# Goby\n",
  };
  try {
    for (const [name, text] of Object.entries({ ...base, ...files })) {
      mkdirSync(dirname(join(dir, name)), { recursive: true });
      writeFileSync(join(dir, name), text);
    }
    return checkMarkdown(dir);
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
}

test("a clean repository's documents pass", () => {
  assert.deepEqual(
    repo({ "README.md": "# Goby\nSee [the guide](docs/guide.md#part-one), `src/main.rs`, `goby.exe` and ![a](images/a.webp).\n\n```\ncargo test -p goby --test resources sample_check -- --ignored\n```\n" }),
    [],
  );
});

const docProblems = [
  ["a link to a missing file", "See [x](docs/nope.md).", /link docs\/nope\.md names a missing file/],
  ["an anchor that is not a heading", "See [x](docs/guide.md#nowhere).", /not a heading/],
  ["an image that is missing", '<img src="images/none.webp">', /images\/none\.webp names a missing file/],
  ["a repository path that is missing", "Edit `src/gone.rs`.", /path src\/gone\.rs is not in the repository/],
  ["a package that is not in the workspace", "```\ncargo test -p nope\n```", /names package nope/],
  ["a test target that does not exist", "```\ncargo test --test missing\n```", /names test target missing/],
  ["a test filter that is no test", "```\ncargo test not_a_test\n```", /filters on not_a_test/],
  ["an external link that is not a URL", "See [x](https://[bad).", /is not a valid URL/],
];

for (const [name, text, expected] of docProblems) {
  test(`the Markdown check names ${name}`, () => {
    const found = repo({ "README.md": `# Goby\n${text}\n` });
    assert.ok(found.some(f => expected.test(f)), `expected ${expected}, got:\n${found.join("\n")}`);
  });
}

test("the project's own Markdown passes", () => {
  assert.deepEqual(checkMarkdown(fileURLToPath(new URL("..", import.meta.url))), []);
});

test("the command line says what it expects instead of a stack trace", () => {
  const script = fileURLToPath(new URL("./check.mjs", import.meta.url));
  const bare = mkdtempSync(join(tmpdir(), "site-check-"));
  mkdirSync(join(bare, "site"));
  try {
    for (const arg of ["/no/such/folder", "--help", join(bare, "site")]) {
      const run = spawnSync(process.execPath, [script, arg], { encoding: "utf8" });
      assert.equal(run.status, 1, arg);
      assert.match(run.stderr, /is not the docs folder of a repository/, arg);
      assert.doesNotMatch(run.stderr, /\n\s+at /, arg);
    }
  } finally {
    rmSync(bare, { recursive: true, force: true });
  }
});

// ---- The figures the documents repeat: each must be the same wherever it is written ----
// The generated pages take the voice count from the code, so they stand for the code where the
// figure comes from the build. Each test lists the places a figure is written and fails when any
// of them disagree.

const repoRoot = fileURLToPath(new URL("..", import.meta.url));
const read = rel => readFileSync(join(repoRoot, rel), "utf8");
const figures = (text, re) => [...text.matchAll(re)].map(m => m[1]);
const same = (name, values) => {
  assert.ok(values.length > 0, `${name}: no figure found`);
  assert.equal(new Set(values).size, 1, `${name} disagrees: ${JSON.stringify(values)}`);
};

test("the voice count is the same in the README, the pages and the how-it-works page", () => {
  // how-it-works gives the count of the 0.4.0 build as history, which is not the current count.
  const howItWorks = read("docs/how-it-works.md").replace(/when the app had \d+ voices/, "");
  same("voices", [
    ...figures(read("README.md"), /(\d+) voices/g),
    ...figures(howItWorks, /(\d+) voices/g),
    ...figures(read("docs/index.html"), /(\d+) voices/g),
    ...figures(read("docs/ru/index.html"), /(\d+) голоса/g),
  ]);
});

test("the release size is the same in the README, how-it-works, both pages and the generator", () => {
  const bytes = Number(figures(read("docs/how-it-works.md"), /\((\d{1,3}(?:,\d{3})+) bytes\)/g)[0].replace(/,/g, ""));
  same("exe size in KB", [
    ...figures(read("README.md"), /(\d+) KB/g),
    ...figures(read("docs/how-it-works.md"), /\| Executable \| (\d+) KB/g),
    ...figures(read("docs/index.html"), /(\d+) KB/g),
    ...figures(read("docs/ru/index.html"), /(\d+) КБ/g),
    ...figures(read("site/dsp-wasm/src/bin/site.rs"), /SIZE_KB: &str = "(\d+)"/g),
    String(Math.round(bytes / 1024)),
  ]);
});

test("the latency figures agree: the mean, the low-latency mode and the 11 ms it saves", () => {
  const mean = [
    ...figures(read("README.md"), /about (\d+) ms from your mouth/g),
    ...figures(read("docs/how-it-works.md"), /(\d+) ms on average/g),
    ...figures(read("docs/index.html"), /About <b>(\d+) ms<\/b>/g),
    ...figures(read("docs/ru/index.html"), /Около <b>(\d+) мс<\/b>/g),
  ];
  same("mean latency", mean);
  const low = Number(figures(read("docs/how-it-works.md"), /(\d+) ms with \*Low latency\*/g)[0]);
  const saved = [
    ...figures(read("README.md"), /about (\d+) ms less delay/g),
    ...figures(read("docs/index.html"), /about (\d+) ms less delay/g),
    ...figures(read("docs/ru/index.html"), /примерно на (\d+) мс/g),
    String(Number(mean[0]) - low),
  ];
  same("latency saved by Low latency", saved);
});

test("the CPU share is under 1 % wherever the pages claim it, and every measured share is below it", () => {
  same("CPU bound", [
    ...figures(read("README.md"), /under (\d+) % of one CPU core/g),
    ...figures(read("docs/index.html"), /Under <b>(\d+) %<\/b> of one core/g),
    ...figures(read("docs/ru/index.html"), /Меньше <b>(\d+) %<\/b> одного ядра/g),
  ]);
  const measured = figures(read("docs/how-it-works.md"), /(\d+\.\d+) % of one core/g).map(Number);
  assert.ok(measured.length > 0);
  assert.ok(Math.max(...measured) < 1, `a measured share is not under 1 %: ${Math.max(...measured)}`);
});

test("the update check runs once a day in the README, install.md and the code", () => {
  assert.match(read("README.md"), /at most once a day/);
  assert.match(read("docs/install.md"), /At most once a day/);
  assert.match(read("src/settings.rs"), /once a day/);
});

test("the signature algorithm is the same in install.md and the updater", () => {
  assert.match(read("docs/install.md"), /ECDSA P-256/);
  assert.match(read("src/update.rs"), /ECDSA P-256/);
  assert.match(read("src/update.rs"), /BCRYPT_ECDSA_P256_ALGORITHM/);
});

test("the version in the download names is the crate's version", () => {
  const version = read("Cargo.toml").match(/^version = "(.*)"$/m)[1];
  same("release file version", [
    ...figures(read("README.md"), /goby-(\d+\.\d+\.\d+)-windows-x64/g),
    ...figures(read("docs/install.md"), /goby-(\d+\.\d+\.\d+)-windows-x64/g),
    ...figures(read("docs/install.md"), /goby-(\d+\.\d+\.\d+)-windows-x64\.exe --install/g),
    version,
  ]);
});

test("the Rust version needed to build is the same in the README and install.md", () => {
  same("Rust version", [
    ...figures(read("README.md"), /Rust (\d+\.\d+)/g),
    ...figures(read("docs/install.md"), /Rust (\d+\.\d+)/g),
  ]);
});
