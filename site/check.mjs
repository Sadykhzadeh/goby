// Checks the built website in docs/: links and the files they name, the parts of a page that
// lay out or read badly without a browser, the metadata of each page, and that every page has
// its twin in the other language. Plain Node, no dependencies.
//
//   node site/check.mjs [docs]
//
// Exits 1 and lists every problem it finds.
import { readdirSync, readFileSync, existsSync, statSync } from "node:fs";
import { dirname, join, relative, resolve, sep } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";

// The address the site is published at. Absolute links to it are checked as internal links.
const SITE = "https://goby.azer.one/";
const VOID = new Set(["area", "base", "br", "col", "embed", "hr", "img", "input", "link", "meta", "source", "track", "wbr"]);
const RAW = new Set(["script", "style"]);

// A small tolerant HTML tree: elements with lower-case names and attributes, text as strings.
export function parse(html) {
  const root = { tag: "#root", attrs: {}, children: [], parent: null };
  let cur = root;
  const re = /<!--[\s\S]*?-->|<![^>]*>|<\/([a-zA-Z][\w-]*)\s*>|<([a-zA-Z][\w-]*)((?:[^>"']|"[^"]*"|'[^']*')*?)\s*(\/?)>|[^<]+|</g;
  let m;
  while ((m = re.exec(html))) {
    const [token, close, open, attrText, selfClosing] = m;
    if (token.startsWith("<!")) continue;
    if (close !== undefined) {
      const name = close.toLowerCase();
      for (let n = cur; n.parent; n = n.parent) {
        if (n.tag === name) {
          cur = n.parent;
          break;
        }
      }
      continue;
    }
    if (open === undefined) {
      cur.children.push(token);
      continue;
    }
    const tag = open.toLowerCase();
    const attrs = {};
    for (const a of attrText.matchAll(/([^\s"'>/=]+)(?:\s*=\s*(?:"([^"]*)"|'([^']*)'|([^\s"'=<>`]+)))?/g)) {
      const key = a[1].toLowerCase();
      if (!(key in attrs)) attrs[key] = a[2] ?? a[3] ?? a[4] ?? "";
    }
    const node = { tag, attrs, children: [], parent: cur };
    cur.children.push(node);
    if (VOID.has(tag) || selfClosing) continue;
    if (RAW.has(tag)) {
      const end = html.toLowerCase().indexOf("</" + tag, re.lastIndex);
      const stop = end < 0 ? html.length : end;
      node.children.push(html.slice(re.lastIndex, stop));
      re.lastIndex = end < 0 ? html.length : html.indexOf(">", end) + 1;
      continue;
    }
    cur = node;
  }
  return root;
}

const elementsOf = node => node.children.filter(c => typeof c !== "string");

function each(node, fn) {
  for (const child of elementsOf(node)) {
    fn(child);
    each(child, fn);
  }
}

// The text a visitor or a screen reader gets from a node, without what is hidden from them.
function textOf(node) {
  return node.children
    .map(c => (typeof c === "string" ? c : c.attrs["aria-hidden"] === "true" ? "" : textOf(c)))
    .join("")
    .replace(/\s+/g, " ")
    .trim();
}

// The file a link names: under the published address, or relative to the page or the site root.
function resolveRef(root, page, value) {
  if (/^(mailto:|tel:|data:|javascript:)/i.test(value)) return undefined;
  if (value.startsWith("#")) return { file: page, frag: value.slice(1) };
  let path;
  if (value.startsWith(SITE)) path = join(root, value.slice(SITE.length));
  else if (/^([a-z][a-z0-9+.-]*:|\/\/)/i.test(value)) return null;
  else if (value.startsWith("/")) path = join(root, value);
  else path = join(dirname(page), value);
  const hash = path.indexOf("#");
  let file = hash < 0 ? path : path.slice(0, hash);
  const frag = hash < 0 ? undefined : path.slice(hash + 1);
  if (/[\\/]$/.test(file) || file === root) file = join(file, "index.html");
  return { file, frag };
}

function listFiles(dir) {
  return readdirSync(dir, { withFileTypes: true }).flatMap(e => {
    const p = join(dir, e.name);
    return e.isDirectory() ? listFiles(p) : [p];
  });
}

const isFile = p => existsSync(p) && statSync(p).isFile();

// The url() targets of a stylesheet, or of a style element's text.
function cssUrls(text) {
  const out = [];
  for (const m of text.matchAll(/url\(\s*(?:"([^"]*)"|'([^']*)'|([^)'"\s]+))\s*\)/g)) {
    const v = (m[1] ?? m[2] ?? m[3]).trim();
    if (!/^(data:|https?:|\/\/)/i.test(v) && v) out.push(v);
  }
  return out;
}

export function checkSite(dirArg) {
  const root = resolve(dirArg);
  const findings = [];
  const rel = f => relative(root, f).split(sep).join("/");
  const bad = (file, msg) => findings.push(`${rel(file)}: ${msg}`);

  const files = listFiles(root);
  const pages = files.filter(f => f.endsWith(".html")).sort();
  const trees = new Map(pages.map(f => [f, parse(readFileSync(f, "utf8"))]));
  const elements = new Map(pages.map(f => {
    const all = [];
    each(trees.get(f), n => all.push(n));
    return [f, all];
  }));
  const htmlOf = f => elements.get(f).find(n => n.tag === "html");
  const langOf = f => htmlOf(f)?.attrs.lang;
  const idsOf = f => new Set(elements.get(f).map(n => n.attrs.id).filter(Boolean));

  // Other sites are not fetched; a link to this site must name a file and, if it has one, an anchor.
  const checkRef = (page, value, attr, node) => {
    const target = resolveRef(root, page, value);
    if (!target) return;
    if (!isFile(target.file)) return bad(page, `<${node.tag} ${attr}="${value}"> names a missing file`);
    if (target.frag && trees.has(target.file) && !idsOf(target.file).has(target.frag)) {
      bad(page, `<${node.tag} ${attr}="${value}"> names an anchor that does not exist`);
    }
  };

  const cssFile = f => {
    for (const u of cssUrls(readFileSync(f, "utf8"))) {
      const clean = u.split(/[?#]/)[0];
      const file = clean.startsWith("/") ? join(root, clean) : join(dirname(f), clean);
      if (!isFile(file)) bad(f, `url(${u}) names a missing file`);
    }
  };

  for (const f of files.filter(x => x.endsWith(".css"))) cssFile(f);

  for (const page of pages) {
    const all = elements.get(page);
    const html = htmlOf(page);
    if (!html) {
      bad(page, "no <html> element");
      continue;
    }
    if (!html.attrs.lang) bad(page, "<html> has no lang");

    const titles = all.filter(n => n.tag === "title");
    if (titles.length !== 1 || !textOf(titles[0])) bad(page, `expected one non-empty <title>, found ${titles.length}`);
    const descriptions = all.filter(n => n.tag === "meta" && n.attrs.name === "description");
    if (descriptions.length !== 1 || !descriptions[0].attrs.content?.trim()) {
      bad(page, `expected one non-empty meta description, found ${descriptions.length}`);
    }

    const h1s = all.filter(n => n.tag === "h1");
    if (h1s.length !== 1) bad(page, `expected one <h1>, found ${h1s.length}`);
    let level = 0;
    for (const n of all.filter(x => /^h[1-6]$/.test(x.tag))) {
      const next = Number(n.tag[1]);
      if (level && next > level + 1) bad(page, `<${n.tag}> follows a level ${level} heading`);
      level = next;
    }

    const ids = new Map();
    for (const n of all) {
      const id = n.attrs.id;
      if (id === undefined) continue;
      if (!id) bad(page, `empty id on <${n.tag}>`);
      else ids.set(id, (ids.get(id) ?? 0) + 1);
    }
    for (const [id, count] of ids) if (count > 1) bad(page, `duplicate id "${id}"`);

    for (const n of all) {
      if (n.tag === "img") {
        if (n.attrs.alt === undefined) bad(page, `<img src="${n.attrs.src}"> has no alt`);
        if (!/^\d+$/.test(n.attrs.width ?? "") || !/^\d+$/.test(n.attrs.height ?? "")) {
          bad(page, `<img src="${n.attrs.src}"> has no width and height`);
        }
      }
      if (n.tag === "button") {
        const name = textOf(n) || n.attrs["aria-label"] || n.attrs.title || labelledBy(n);
        if (!name?.trim()) bad(page, `<button id="${n.attrs.id ?? ""}"> has no accessible name`);
      }
      if (["input", "select", "textarea"].includes(n.tag) && !["hidden", "submit", "button", "reset", "image"].includes(n.attrs.type)) {
        const labelled = n.attrs.id !== undefined && all.some(x => x.tag === "label" && x.attrs.for === n.attrs.id);
        let inLabel = false;
        for (let p = n.parent; p; p = p.parent) if (p.tag === "label") inLabel = true;
        if (!labelled && !inLabel && !n.attrs["aria-label"] && !labelledBy(n) && !n.attrs.title) {
          bad(page, `<${n.tag} id="${n.attrs.id ?? ""}"> has no label`);
        }
      }
      if (n.tag === "a" && n.attrs.target === "_blank") {
        if (!(n.attrs.rel ?? "").split(/\s+/).includes("noopener")) {
          bad(page, `<a href="${n.attrs.href}" target="_blank"> lacks rel="noopener"`);
        }
      }
      for (const attr of ["href", "src", "poster"]) {
        if (n.attrs[attr] !== undefined) checkRef(page, n.attrs[attr], attr, n);
      }
      if (n.attrs.srcset !== undefined) {
        for (const candidate of n.attrs.srcset.split(",")) {
          const [url] = candidate.trim().split(/\s+/);
          if (url) checkRef(page, url, "srcset", n);
        }
      }
      if (n.tag === "style") {
        for (const c of n.children) {
          if (typeof c !== "string") continue;
          for (const u of cssUrls(c)) {
            const clean = u.split(/[?#]/)[0];
            const file = clean.startsWith("/") ? join(root, clean) : join(dirname(page), clean);
            if (!isFile(file)) bad(page, `url(${u}) names a missing file`);
          }
        }
      }
    }

    const canonicals = all.filter(n => n.tag === "link" && n.attrs.rel === "canonical");
    if (canonicals.length !== 1) bad(page, `expected one canonical link, found ${canonicals.length}`);
    else {
      const target = resolveRef(root, page, canonicals[0].attrs.href ?? "");
      if (!target || target.file !== page) bad(page, `canonical ${canonicals[0].attrs.href} is not this page`);
    }
  }

  // Every page has its twin in each language: the alternates list every language and point back.
  const alternatesOf = page => {
    const map = new Map();
    for (const n of elements.get(page)) {
      if (n.tag !== "link" || n.attrs.rel !== "alternate" || !n.attrs.hreflang) continue;
      const target = resolveRef(root, page, n.attrs.href ?? "");
      map.set(n.attrs.hreflang, target ? target.file : null);
    }
    return map;
  };
  const languages = new Set(pages.map(langOf).filter(Boolean));
  for (const page of pages) {
    const alts = alternatesOf(page);
    for (const lang of languages) {
      if (!alts.has(lang)) bad(page, `no hreflang="${lang}" alternate`);
    }
    for (const [lang, target] of alts) {
      if (lang === "x-default") {
        if (target && !trees.has(target)) bad(page, "x-default names a page that is not in the site");
        continue;
      }
      if (!target) bad(page, `hreflang="${lang}" does not name a page of this site`);
      else if (!trees.has(target)) bad(page, `hreflang="${lang}" names a missing page`);
      else {
        if (langOf(target) !== lang) bad(page, `hreflang="${lang}" names a page in lang="${langOf(target)}"`);
        if (alternatesOf(target).get(langOf(page)) !== page) bad(page, `hreflang="${lang}" points at ${rel(target)}, which does not point back`);
      }
    }
  }

  return findings;
}

function labelledBy(n) {
  return n.attrs["aria-labelledby"] ? n.attrs["aria-labelledby"] : "";
}

if (process.argv[1] && pathToFileURL(resolve(process.argv[1])).href === import.meta.url) {
  const dir = process.argv[2] ?? join(dirname(fileURLToPath(import.meta.url)), "..", "docs");
  const findings = checkSite(dir);
  if (findings.length) {
    for (const f of findings) console.error(f);
    console.error(`${findings.length} problem(s) in the site`);
    process.exit(1);
  }
  console.log(`site check: ok (${dir})`);
}
