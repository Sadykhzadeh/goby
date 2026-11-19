// Checks the built website in docs/: links and the files they name, the parts of a page that
// lay out or read badly without a browser, the metadata of each page, and that every page has
// its twin in the other language. Plain Node, no dependencies.
//
//   node site/check.mjs [docs]
//
// Exits 1 and lists every problem it finds.
import { createHash } from "node:crypto";
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
    for (const v of all.filter(n => n.tag === "meta" && n.attrs.name === "viewport")) {
      const zoomOff = (v.attrs.content ?? "").split(/[,;]/).some(part => {
        const [key, value = ""] = part.split("=").map(x => x.trim().toLowerCase());
        return (key === "user-scalable" && (value === "no" || value === "0")) || (key === "maximum-scale" && Number(value) < 2);
      });
      if (zoomOff) bad(page, `the viewport stops pinch zoom: "${v.attrs.content}"`);
    }

    const policy = all.find(n => n.tag === "meta" && n.attrs["http-equiv"]?.toLowerCase() === "content-security-policy");
    if (!policy) bad(page, "no Content-Security-Policy meta");
    else {
      const rules = policy.attrs.content ?? "";
      if (/'unsafe-(inline|eval)'/.test(rules)) bad(page, "the Content-Security-Policy allows unsafe inline script or style");
      const allowed = new Set(rules.match(/'sha256-[^']+'/g) ?? []);
      for (const n of all.filter(x => x.tag === "style")) {
        const text = n.children.filter(c => typeof c === "string").join("");
        const hash = `'sha256-${createHash("sha256").update(text).digest("base64")}'`;
        if (!allowed.has(hash)) bad(page, `an inline <style> is not allowed by the Content-Security-Policy: ${hash}`);
      }
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

// ---- The Markdown the project publishes: README.md and docs/*.md ----

// A GitHub heading anchor: lower case, punctuation dropped, each space a hyphen.
export function slug(heading) {
  return heading
    .replace(/!?\[([^\]]*)\]\([^)]*\)/g, "$1")
    .replace(/<[^>]+>/g, "")
    .replace(/[`*~]/g, "")
    .trim()
    .toLowerCase()
    .replace(/[^\p{L}\p{N}\p{M} _-]/gu, "")
    .replace(/ /g, "-");
}

// The lines of a Markdown text, each with whether it is inside a fenced code block.
export function proseLines(md) {
  let fence = null;
  return md.split("\n").map(line => {
    const open = line.match(/^\s*(`{3,}|~{3,})/);
    if (fence) {
      if (open && open[1][0] === fence[0] && open[1].length >= fence.length) fence = null;
      return { text: line, code: true };
    }
    if (open) {
      fence = open[1];
      return { text: line, code: true };
    }
    return { text: line, code: false };
  });
}

// The anchors a Markdown file offers: its headings as GitHub makes them (a repeated heading
// gets -1, -2, ...) and any id or name attribute.
export function anchorsOf(md) {
  const seen = new Map();
  const out = new Set();
  for (const { text, code } of proseLines(md)) {
    if (code) continue;
    const heading = text.match(/^#{1,6}\s+(.+?)\s*#*\s*$/);
    if (heading) {
      const base = slug(heading[1]);
      const n = seen.get(base) ?? 0;
      seen.set(base, n + 1);
      out.add(n ? `${base}-${n}` : base);
    }
    for (const m of text.matchAll(/\s(?:id|name)="([^"]+)"/g)) out.add(m[1]);
  }
  return out;
}

// Link targets on a line of prose: Markdown links and images, src or href attributes, and
// reference definitions.
export function linksIn(line) {
  const prose = line.replace(/`[^`\n]*`/g, "");
  const out = [];
  for (const m of prose.matchAll(/!?\[[^\]]*\]\(\s*<?([^)\s>]+)>?(?:\s+"[^"]*")?\s*\)/g)) out.push(m[1]);
  for (const m of prose.matchAll(/\b(?:src|href)="([^"]+)"/g)) out.push(m[1]);
  const def = prose.match(/^\s*\[[^\]]+\]:\s*(\S+)/);
  if (def) out.push(def[1]);
  return out;
}

const PATH_EXTENSIONS = /\.(rs|md|toml|lang|yml|yaml|sig|bin|webp|svg|png|mp3|html|js|mjs|css|ps1|sh|wasm|lock|ico)$/;

// Backtick spans that name a file or folder of the repository: anything with a slash, or a bare
// file name with a source or asset extension. Release artefacts (.exe, .txt) are not in the
// repository and are not taken for paths.
export function repoPathsIn(line) {
  return [...line.matchAll(/`([^`\n]+)`/g)]
    .map(m => m[1])
    .filter(s => /^[\w.~/-]+$/.test(s) && (s.includes("/") || PATH_EXTENSIONS.test(s)));
}

// The cargo command lines of a text: whole lines in fenced blocks, and backtick spans.
export function cargoCommandsIn(md) {
  const out = [];
  for (const { text, code } of proseLines(md)) {
    if (code) {
      const cmd = text.trim();
      if (cmd.startsWith("cargo ")) out.push(cmd);
    } else {
      for (const m of text.matchAll(/`(cargo [^`\n]+)`/g)) out.push(m[1]);
    }
  }
  return out.map(cmd => cmd.replace(/\s#.*$/, "").trim());
}

// Flags that take a value, so the word after them is not a test name.
const VALUE_FLAGS = new Set([
  "-p", "--package", "--test", "--bin", "--example", "--features", "-F", "--target", "--profile",
  "--manifest-path", "-j", "--jobs", "--exclude", "--color", "--message-format",
]);

// What a cargo command line names: packages, targets, and test filters.
export function namesIn(cmd) {
  const tokens = cmd.split(/\s+/).slice(1);
  const sub = tokens[0];
  const out = { packages: [], tests: [], bins: [], filters: [] };
  for (let i = 1; i < tokens.length; i++) {
    const t = tokens[i];
    if (t === "--") break;
    if (t.startsWith("--") && t.includes("=")) {
      const [flag, value] = t.split("=", 2);
      if (flag === "--package") out.packages.push(value);
      if (flag === "--test") out.tests.push(value);
      if (flag === "--bin") out.bins.push(value);
      continue;
    }
    if (t === "-p" || t === "--package") out.packages.push(tokens[++i]);
    else if (t === "--test") out.tests.push(tokens[++i]);
    else if (t === "--bin") out.bins.push(tokens[++i]);
    else if (VALUE_FLAGS.has(t)) i++;
    else if (!t.startsWith("-") && sub === "test") out.filters.push(t);
  }
  return out;
}

// Every file of the repository, relative to it with forward slashes, without build output.
function repoListing(root) {
  const out = [];
  const walk = dir => {
    for (const e of readdirSync(dir, { withFileTypes: true })) {
      if (e.name === "target" || e.name === ".git" || e.name === "node_modules") continue;
      const p = join(dir, e.name);
      if (e.isDirectory()) walk(p);
      else out.push(relative(root, p).split(sep).join("/"));
    }
  };
  walk(root);
  return out;
}

export function checkMarkdown(repoArg) {
  const root = resolve(repoArg);
  const findings = [];
  const listing = repoListing(root);
  const known = new Set(listing);
  const exists = p => known.has(p) || listing.some(f => f.startsWith(p + "/"));
  const basenames = new Set(listing.map(f => f.split("/").pop()));
  const docs = readdirSync(join(root, "docs")).filter(f => f.endsWith(".md")).map(f => `docs/${f}`);
  const files = ["README.md", ...docs];
  const texts = new Map(files.map(f => [f, readFileSync(join(root, f), "utf8")]));
  const anchors = new Map([...texts].map(([f, t]) => [f, anchorsOf(t)]));

  // The package names, binary targets and test targets of the workspace.
  const packages = new Set();
  const bins = new Set();
  const tests = new Set();
  for (const f of listing.filter(x => x.endsWith("Cargo.toml"))) {
    const toml = readFileSync(join(root, f), "utf8");
    const name = toml.match(/^name = "([^"]+)"/m)?.[1];
    if (!name) continue;
    packages.add(name);
    const dir = f.includes("/") ? f.slice(0, f.lastIndexOf("/") + 1) : "";
    if (known.has(`${dir}src/main.rs`)) bins.add(name);
  }
  for (const f of listing) {
    if (/(^|\/)src\/bin\/[^/]+\.rs$/.test(f)) bins.add(f.split("/").pop().replace(/\.rs$/, ""));
    if (/(^|\/)tests\/[^/]+\.rs$/.test(f)) tests.add(f.split("/").pop().replace(/\.rs$/, ""));
    if (/(^|\/)tests\/[^/]+\/main\.rs$/.test(f)) tests.add(f.split("/").slice(-2)[0]);
  }
  const testFns = new Set();
  for (const f of listing.filter(x => x.endsWith(".rs"))) {
    for (const m of readFileSync(join(root, f), "utf8").matchAll(/\bfn\s+(\w+)/g)) testFns.add(m[1]);
  }

  const bad = (file, msg) => findings.push(`${file}: ${msg}`);

  const checkTarget = (file, target) => {
    if (/^https?:\/\//i.test(target)) {
      try {
        new URL(target);
      } catch {
        bad(file, `link ${target} is not a valid URL`);
      }
      return;
    }
    if (/^mailto:/i.test(target)) return;
    const hash = target.indexOf("#");
    const pathPart = decodeURI(hash < 0 ? target : target.slice(0, hash));
    const frag = hash < 0 ? undefined : decodeURI(target.slice(hash + 1));
    const resolved = pathPart === "" ? file : join(dirname(file), pathPart).split(sep).join("/");
    if (resolved.startsWith("..")) return bad(file, `link ${target} leaves the repository`);
    if (!exists(resolved)) return bad(file, `link ${target} names a missing file`);
    if (frag && resolved.endsWith(".md") && !anchors.get(resolved)?.has(frag)) {
      bad(file, `link ${target} names an anchor that is not a heading of ${resolved}`);
    }
  };

  for (const file of files) {
    for (const { text, code } of proseLines(texts.get(file))) {
      if (code) continue;
      for (const target of linksIn(text)) checkTarget(file, target);
      for (const p of repoPathsIn(text)) {
        // Build output, which a build creates; it is not a file the repository holds.
        if (p.startsWith("target/")) continue;
        const there = p.includes("/") ? exists(p.replace(/\/$/, "")) : basenames.has(p);
        if (!there) bad(file, `path ${p} is not in the repository`);
      }
    }
    for (const cmd of cargoCommandsIn(texts.get(file))) {
      const names = namesIn(cmd);
      for (const p of names.packages) if (!packages.has(p)) bad(file, `\`${cmd}\` names package ${p}, which is not in the workspace`);
      for (const t of names.tests) if (!tests.has(t)) bad(file, `\`${cmd}\` names test target ${t}, which does not exist`);
      for (const b of names.bins) if (!bins.has(b)) bad(file, `\`${cmd}\` names binary ${b}, which does not exist`);
      for (const f of names.filters) if (!testFns.has(f)) bad(file, `\`${cmd}\` filters on ${f}, which is no test function`);
    }
  }
  return findings;
}

if (process.argv[1] && pathToFileURL(resolve(process.argv[1])).href === import.meta.url) {
  const dir = process.argv[2] ?? join(dirname(fileURLToPath(import.meta.url)), "..", "docs");
  const findings = [...checkSite(dir), ...checkMarkdown(join(resolve(dir), ".."))];
  if (findings.length) {
    for (const f of findings) console.error(f);
    console.error(`${findings.length} problem(s) in the site and its documents`);
    process.exit(1);
  }
  console.log(`site check: ok (${dir}, README.md and docs/*.md)`);
}
