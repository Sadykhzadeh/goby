// Tests for site/check.mjs: each case breaks one rule on a small two-language site and
// expects the check to name it. Run with `node --test site/`.
import { test } from "node:test";
import assert from "node:assert/strict";
import { mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import vm from "node:vm";
import { fileURLToPath } from "node:url";
import { checkSite, parse } from "./check.mjs";

const page = ({ lang = "en", body = "", head = "" } = {}) => {
  const root = lang === "ru" ? "../" : "";
  return `<!doctype html>
<html lang="${lang}">
<head>
<meta charset="utf-8">
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
function runSiteScript(html, { reduced = false } = {}) {
  const doc = fileURLToPath(new URL("../docs/index.html", import.meta.url));
  const page = readFileSync(doc, "utf8");
  const el = id => ({ id, textContent: "", setAttribute() {}, addEventListener() {}, querySelector: () => el("x") });
  const elements = {
    texts: { textContent: page.match(/id="texts">([\s\S]*?)<\/script>/)[1] },
    "hero-voices": { textContent: page.match(/id="hero-voices">([\s\S]*?)<\/script>/)[1] },
    name: Object.assign(el("name"), { textContent: "Minion" }),
  };
  const observed = [];
  const fetches = [];
  const intervals = [];
  const ctx = {
    document: {
      currentScript: { src: "https://goby.azer.one/site.js" },
      getElementById: id => elements[id] ?? el(id),
      querySelector: () => el("hero"),
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
    setInterval: () => intervals.push(1),
    performance: { now: () => 0 },
    fetch: url => {
      fetches.push(String(url));
      return Promise.resolve({ ok: false, status: 404 });
    },
    URL,
    console,
  };
  vm.createContext(ctx);
  vm.runInContext(html, ctx, { filename: "site.js" });
  return { ctx, observed, fetches, intervals };
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
