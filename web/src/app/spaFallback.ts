export function resolveSpaFallbackInitialPath(
  searchParams: URLSearchParams,
): string | undefined {
  const path = searchParams.get("spa_path")?.trim();
  if (!path || !path.startsWith("/")) return undefined;
  if (path.startsWith("/docs/") && path.endsWith(".html"))
    return path.slice(0, -".html".length);
  return path;
}

export function restoreSpaFallbackHash(searchParams: URLSearchParams): void {
  restoreSpaFallbackLocation(searchParams);
}

export function restoreSpaFallbackLocation(searchParams: URLSearchParams): void {
  const path = resolveSpaFallbackInitialPath(searchParams);
  const search = searchParams.get("spa_search");
  const hash = searchParams.get("spa_hash");
  if (!path || typeof window === "undefined") return;
  const nextUrl = new URL(window.location.href);
  nextUrl.pathname = withCurrentDeploymentRoot(path);
  if (search) {
    nextUrl.search = search;
  } else {
    nextUrl.searchParams.delete("spa_path");
    nextUrl.searchParams.delete("spa_search");
    nextUrl.searchParams.delete("spa_hash");
  }
  nextUrl.searchParams.delete("spa_path");
  nextUrl.searchParams.delete("spa_search");
  nextUrl.searchParams.delete("spa_hash");
  if (hash) nextUrl.hash = hash;
  window.history.replaceState(window.history.state, "", nextUrl);
}

function withCurrentDeploymentRoot(path: string): string {
  const root = currentDeploymentRoot();
  const cleanPath = path.startsWith("/") ? path.slice(1) : path;
  return `${root}${cleanPath}`;
}

function currentDeploymentRoot(): string {
  const pathname = window.location.pathname || "/";
  if (pathname.endsWith("/")) return pathname;
  const slash = pathname.lastIndexOf("/");
  return slash >= 0 ? pathname.slice(0, slash + 1) : "/";
}
