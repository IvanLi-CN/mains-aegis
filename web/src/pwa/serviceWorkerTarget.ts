const appRouteSegments = new Set(["connect", "devices", "docs"]);

export type ServiceWorkerTarget = {
  scriptUrl: string;
  scope: string;
};

export function resolveServiceWorkerTarget(
  base: string | undefined,
  runtimePathname: string,
): ServiceWorkerTarget {
  const scope = normalizeServiceWorkerScope(base, runtimePathname);
  return {
    scriptUrl: `${scope}sw.js`,
    scope,
  };
}

export function normalizeServiceWorkerScope(
  base: string | undefined,
  runtimePathname: string,
): string {
  const raw = (base ?? "").trim();
  if (!raw || raw === "/") return "/";
  if (raw === "." || raw === "./" || raw.startsWith("./") || raw.startsWith("../"))
    return deriveRuntimeScope(runtimePathname);
  if (/^[a-z][a-z0-9+.-]*:/i.test(raw)) return "/";
  const withLeading = raw.startsWith("/") ? raw : `/${raw}`;
  return withLeading.endsWith("/") ? withLeading : `${withLeading}/`;
}

function deriveRuntimeScope(pathname: string): string {
  const pathnameOnly = pathname.split("?", 1)[0]?.split("#", 1)[0] || "/";
  const withLeading = pathnameOnly.startsWith("/") ? pathnameOnly : `/${pathnameOnly}`;
  const rawSegments = withLeading.split("/").filter(Boolean);
  const lastSegment = rawSegments.at(-1);
  const hasIndexLikeEntry = lastSegment
    ? ["index.html", "404.html"].includes(lastSegment)
    : false;
  const segments = hasIndexLikeEntry ? rawSegments.slice(0, -1) : rawSegments;
  if (segments.length === 0) return "/";

  const routeIndex = segments.findIndex((segment) => appRouteSegments.has(segment));
  if (routeIndex === 0) return "/";
  if (routeIndex > 0) return `/${segments.slice(0, routeIndex).join("/")}/`;
  if (hasIndexLikeEntry) return `/${segments.join("/")}/`;
  if (withLeading.endsWith("/")) return `/${segments.join("/")}/`;
  if (segments.length === 2) return `/${segments.join("/")}/`;
  if (segments.length === 1)
    return withLeading.endsWith("/") ? `/${segments[0]}/` : "/";
  return "/";
}
