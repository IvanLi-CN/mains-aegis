self.addEventListener("fetch", (event) => {
  if (event.request.mode !== "navigate") return;

  const requestUrl = new URL(event.request.url);
  const scopeUrl = new URL(self.registration.scope);
  if (requestUrl.origin !== scopeUrl.origin) return;
  if (!requestUrl.pathname.startsWith(scopeUrl.pathname)) return;

  const scopePath = scopeUrl.pathname.endsWith("/")
    ? scopeUrl.pathname.slice(0, -1)
    : scopeUrl.pathname;
  const pathInScope = requestUrl.pathname.slice(scopePath.length) || "/";
  if (
    pathInScope === "/" ||
    pathInScope === "/index.html" ||
    pathInScope.startsWith("/api/") ||
    pathInScope === "/api" ||
    pathInScope.startsWith("/events/") ||
    pathInScope === "/events" ||
    pathInScope === "/docs" ||
    pathInScope.startsWith("/docs/")
  ) {
    return;
  }

  const params = new URLSearchParams(requestUrl.search);
  const originalSearch = params.toString();
  params.set("spa_path", pathInScope);
  if (originalSearch) params.set("spa_search", originalSearch);
  event.respondWith(Response.redirect(`${scopeUrl.href}?${params.toString()}`, 302));
});
