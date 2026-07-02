import { mkdir, writeFile } from "node:fs/promises";
import { dirname } from "node:path";

const [outArg, repoArg = "", baseArg = "./"] = process.argv.slice(2);

if (!outArg) {
  console.error("usage: node tools/pages/write-spa-fallback.mjs <out-file> [repo-name]");
  process.exit(2);
}

const repoName = repoArg.trim();
const repoPrefix = repoName ? `/${escapeScriptString(repoName)}/` : "";
const configuredBase = normalizeBase(baseArg);
const configuredRoot =
  configuredBase && configuredBase !== "./" ? escapeScriptString(configuredBase) : "";

const html = `<!doctype html>
<html lang="en">
  <head>
    <meta charset="UTF-8" />
    <meta name="viewport" content="width=device-width, initial-scale=1.0" />
    <title>Mains Aegis</title>
  </head>
  <body>
    <script>
      (function () {
        var repoPrefix = "${repoPrefix}";
        var path = window.location.pathname || "/";
        var isGithubProjectHost = /\\.github\\.io$/i.test(window.location.hostname);
        var configuredRoot = "${configuredRoot}";
        var configuredRootNoSlash = configuredRoot.replace(/\\/$/, "");
        var appRoot = configuredRoot || "/";
        if (configuredRoot && (path === configuredRootNoSlash || path.indexOf(configuredRoot) === 0)) {
          path = path === configuredRootNoSlash ? "/" : path.slice(configuredRoot.length - 1) || "/";
        } else if (repoPrefix && (path === repoPrefix.slice(0, -1) || path.indexOf(repoPrefix) === 0)) {
          if (!configuredRoot && (isGithubProjectHost || path.indexOf(repoPrefix) === 0)) {
            appRoot = repoPrefix;
          }
          path = path.slice(repoPrefix.length - 1) || "/";
        }
        if (path === "/docs" || path === "/docs/" || path.indexOf("/docs/") === 0) {
          var docsPath = path === "/docs" || path === "/docs/" ? "/docs/index.html" : path;
          if (docsPath.slice(-1) === "/") {
            var docsPathWithoutSlash = docsPath.replace(/\\/+$/, "");
            if (/^\\/docs\\/(handbook|design|manual)$/i.test(docsPathWithoutSlash)) {
              docsPath = docsPathWithoutSlash + "/index.html";
            } else {
              docsPath = docsPathWithoutSlash + ".html";
            }
          } else if (/^\\/docs\\/(handbook|design|manual)$/i.test(docsPath)) {
            docsPath += "/index.html";
          } else if (!/\\/[^/]*\\.[^/]+$/i.test(docsPath)) {
            docsPath += ".html";
          } else {
            docsPath = "/docs/404.html";
          }
          window.location.replace(appRoot.replace(/\\/$/, "") + docsPath + window.location.search + window.location.hash);
          return;
        }
        var params = new URLSearchParams(window.location.search);
        var search = params.toString();
        params.set("spa_path", path);
        if (search) params.set("spa_search", search);
        if (window.location.hash) params.set("spa_hash", window.location.hash.slice(1));
        window.location.replace(appRoot + "?" + params.toString());
      })();
    </script>
  </body>
</html>
`;

await mkdir(dirname(outArg), { recursive: true });
await writeFile(outArg, html);

function escapeScriptString(value) {
  return value.replaceAll("\\", "\\\\").replaceAll("\"", "\\\"");
}

function normalizeBase(value) {
  const raw = String(value || "./").trim();
  if (!raw || raw === "." || raw === "./") return "./";
  const withLeading = raw.startsWith("/") ? raw : `/${raw}`;
  return withLeading.endsWith("/") ? withLeading : `${withLeading}/`;
}
