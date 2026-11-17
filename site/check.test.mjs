// Tests for site/check.mjs: each case breaks one rule on a small two-language site and
// expects the check to name it. Run with `node --test site/`.
import { test } from "node:test";
import assert from "node:assert/strict";
import { mkdirSync, mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
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
