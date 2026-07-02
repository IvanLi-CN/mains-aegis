import { readdir, readFile, stat, writeFile } from "node:fs/promises";
import { dirname, join, relative, sep } from "node:path";

const [rootArg, baseArg = "/docs/"] = process.argv.slice(2);

if (!rootArg) {
  console.error("usage: node tools/pages/relativize-docs-html.mjs <docs-root> [base-path]");
  process.exit(2);
}

const docsRoot = rootArg;
const basePath = normalizeBase(baseArg);

for (const file of await listHtmlFiles(docsRoot)) {
  const html = await readFile(file, "utf8");
  const fromDir = dirname(file);
  const prefix = relativePrefix(fromDir);
  const next = sitePathRewrites().reduce(
    (current, rewrite) =>
      rewriteSitePath(current, rewrite.absolutePath, `${prefix}${rewrite.relativePath}`),
    html,
  );
  if (next !== html) await writeFile(file, next);
}

const jsFiles = await listJsFiles(join(docsRoot, "static/js"));
let publicPathRewriteCount = 0;
for (const file of jsFiles) {
  const js = await readFile(file, "utf8");
  const runtimeBase = docsRuntimeBaseExpression(file);
  const fixedPublicPath = escapeRegExp(basePath);
  const publicPathPattern = new RegExp(
    `([A-Za-z_$][\\w$]*\\.p\\s*=\\s*)(["'])${fixedPublicPath}\\2`,
    "g",
  );
  const rewritten = rewriteDocsRuntimeBase(
    js.replace(publicPathPattern, (_match, lhs) => {
      publicPathRewriteCount += 1;
      return `${lhs}${runtimeBase}.href`;
    }),
    runtimeBase,
  );
  const withSearchLinks = rewriteDocsSearchLinks(rewritten);
  const next = withSearchLinks === js ? js : ensureDocsRuntimeBase(withSearchLinks, file);
  if (next !== js) await writeFile(file, next);
}
if (jsFiles.length > 0 && publicPathRewriteCount === 0) {
  console.warn(`No docs JS public path assignment was rewritten for ${basePath}`);
}

for (const file of await listJsonFiles(join(docsRoot, "static"))) {
  const json = await readFile(file, "utf8");
  const next = rewriteDocsJsonRoutePaths(json);
  if (next !== json) await writeFile(file, next);
}

function normalizeBase(value) {
  const raw = String(value || "/").trim();
  const withLeading = raw.startsWith("/") ? raw : `/${raw}`;
  return withLeading.endsWith("/") ? withLeading : `${withLeading}/`;
}

function sitePathRewrites() {
  return [
    { absolutePath: basePath, relativePath: "" },
    { absolutePath: "/", relativePath: "" },
    { absolutePath: "/static/", relativePath: "static/" },
    { absolutePath: "/brand/", relativePath: "brand/" },
    { absolutePath: "/favicon.svg", relativePath: "favicon.svg" },
    { absolutePath: "/index.html", relativePath: "index.html" },
    { absolutePath: "/404.html", relativePath: "404.html" },
    { absolutePath: "/handbook/", relativePath: "handbook/" },
    { absolutePath: "/design/", relativePath: "design/" },
    { absolutePath: "/manual/", relativePath: "manual/" },
  ];
}

function rewriteSitePath(html, absolutePath, relativeReplacement) {
  return rewriteCssUrlAbsolutePath(
    rewriteAttributeAbsolutePath(html, absolutePath, relativeReplacement),
    absolutePath,
    relativeReplacement,
  );
}

function rewriteAttributeAbsolutePath(html, absolutePath, relativeReplacement) {
  return html.replace(
    /(\s(?:action|href|src)=)(["'])([^"']*)(["'])/g,
    (match, name, quote, value, endQuote) => {
      if (!value.startsWith(absolutePath)) return match;
      return `${name}${quote}${relativeReplacement}${value.slice(absolutePath.length)}${endQuote}`;
    },
  );
}

function rewriteCssUrlAbsolutePath(html, absolutePath, relativeReplacement) {
  return html.replace(
    /url\(\s*(["']?)([^"')]+)\1\s*\)/g,
    (match, quote, value) => {
      if (!value.startsWith(absolutePath)) return match;
      return `url(${quote}${relativeReplacement}${value.slice(absolutePath.length)}${quote})`;
    },
  );
}

function escapeRegExp(value) {
  return value.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
}

function rewriteDocsRuntimeBase(js, runtimeBase) {
  const fixedBase = escapeRegExp(basePath);
  const runtimePath = (suffix) =>
    suffix
      ? `new URL(${JSON.stringify(suffix)},${runtimeBase}).pathname`
      : `${runtimeBase}.pathname`;
  const siteRuntimePathPattern = new RegExp(
    `(\\b(?:href|src|path|logo|link)\\s*:\\s*)(["'])(${siteRuntimePathAlternatives()})([^"']*)\\2`,
    "g",
  );
  return js
    .replace(new RegExp(`(base\\s*:\\s*)(["'])${fixedBase}\\2`, "g"), (_match, lhs) => {
      return `${lhs}${runtimePath("")}`;
    })
    .replace(new RegExp(`(base\\s*:\\s*)(["'])/\\2`, "g"), (_match, lhs) => {
      return `${lhs}${runtimePath("")}`;
    })
    .replace(
      new RegExp(`(path\\s*:\\s*)(["'])${fixedBase}([^"']*)\\2`, "g"),
      (_match, lhs, _quote, suffix) => `${lhs}${runtimePath(suffix)}`,
    )
    .replace(
      new RegExp(`(logo\\s*:\\s*)(["'])${fixedBase}([^"']*)\\2`, "g"),
      (_match, lhs, _quote, suffix) => `${lhs}${runtimePath(suffix)}`,
    )
    .replace(
      new RegExp(`(routePath\\s*:\\s*)(["'])${fixedBase}([^"']*)\\2`, "g"),
      (_match, lhs, _quote, suffix) => `${lhs}${JSON.stringify(`/${suffix}`)}`,
    )
    .replace(
      siteRuntimePathPattern,
      (_match, lhs, _quote, absolutePath, suffix) =>
        `${lhs}${runtimePath(`${siteRuntimePathRelative(absolutePath)}${suffix}`)}`,
    );
}

function rewriteDocsSearchLinks(js) {
  return js.replace(
    /\(0,([A-Za-z_$][\w$]*)\.AP\)\(([^)]*?\.routePath)\)/g,
    (_match, runtimeModule, routePath) =>
      `(0,${runtimeModule}.pJ)((0,${runtimeModule}.AP)(${routePath}))`,
  );
}

function siteRuntimePathAlternatives() {
  return sitePathRewrites()
    .map((rewrite) => rewrite.absolutePath)
    .sort((a, b) => b.length - a.length)
    .map(escapeRegExp)
    .join("|");
}

function siteRuntimePathRelative(absolutePath) {
  const rewrite = sitePathRewrites().find((entry) => entry.absolutePath === absolutePath);
  return rewrite?.relativePath ?? "";
}

async function listHtmlFiles(root) {
  const result = [];
  for (const entry of await readdir(root)) {
    const path = join(root, entry);
    const info = await stat(path);
    if (info.isDirectory()) {
      result.push(...(await listHtmlFiles(path)));
    } else if (entry.endsWith(".html")) {
      result.push(path);
    }
  }
  return result;
}

async function listJsFiles(root) {
  return listFiles(root, (entry) => entry.endsWith(".js"));
}

async function listJsonFiles(root) {
  return listFiles(root, (entry) => entry.endsWith(".json"));
}

async function listFiles(root, matches) {
  const result = [];
  let entries;
  try {
    entries = await readdir(root);
  } catch (error) {
    if (error && error.code === "ENOENT") return result;
    throw error;
  }
  for (const entry of entries) {
    const path = join(root, entry);
    const info = await stat(path);
    if (info.isDirectory()) {
      result.push(...(await listFiles(path, matches)));
    } else if (matches(entry)) {
      result.push(path);
    }
  }
  return result;
}

function relativePrefix(fromDir) {
  const path = relative(fromDir, docsRoot).split(sep).join("/");
  return path ? `${path}/` : "./";
}

function docsRuntimeBaseExpression(file) {
  return "globalThis.__mainsAegisDocsBase";
}

function ensureDocsRuntimeBase(js, file) {
  const marker = "globalThis.__mainsAegisDocsBase=";
  if (js.includes(marker)) return js;
  const prefix = relativePrefix(dirname(file));
  return `${marker}new URL(${JSON.stringify(prefix)},document.currentScript&&document.currentScript.src||location.href);${js}`;
}

function rewriteDocsJsonRoutePaths(json) {
  const fixedBase = escapeRegExp(basePath);
  return json.replace(
    new RegExp(`("routePath"\\s*:\\s*)"${fixedBase}([^"]*)"`, "g"),
    (_match, lhs, suffix) => `${lhs}${JSON.stringify(`/${suffix}`)}`,
  );
}
