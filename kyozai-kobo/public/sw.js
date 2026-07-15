/* 教材工房 PWA service worker. API responses are never cached. */
const CACHE = "kyozai-kobo-shell-v13";
const CORE = [
  "/",
  "/manifest.webmanifest",
  "/icons/icon-128.png",
  "/icons/icon-256.png",
  "/icons/icon-512.png",
  "/icons/apple-touch-icon.png",
  "/mathjax/tex-svg.js",
];

async function cacheApplicationShell() {
  const cache = await caches.open(CACHE);
  const response = await fetch("/", { cache: "no-store" });
  if (!response.ok) throw new Error(`shell fetch failed: ${response.status}`);
  const html = await response.clone().text();
  await cache.put("/", response);
  const urls = new Set(CORE.slice(1));
  for (const match of html.matchAll(/(?:src|href)=["']([^"']+)["']/g)) {
    const url = new URL(match[1], self.location.origin);
    if (url.origin === self.location.origin && !url.pathname.startsWith("/api/")) {
      urls.add(url.pathname + url.search);
    }
  }
  await Promise.allSettled([...urls].map((url) => cache.add(url)));
}

self.addEventListener("install", (event) => {
  event.waitUntil(cacheApplicationShell());
});

self.addEventListener("activate", (event) => {
  event.waitUntil(
    Promise.all([
      caches.keys().then((keys) =>
        Promise.all(keys.filter((key) => key !== CACHE).map((key) => caches.delete(key))),
      ),
      self.clients.claim(),
    ]),
  );
});

self.addEventListener("message", (event) => {
  if (event.data === "SKIP_WAITING") self.skipWaiting();
});

self.addEventListener("fetch", (event) => {
  const url = new URL(event.request.url);
  if (event.request.method !== "GET" || url.origin !== self.location.origin) return;
  if (url.pathname.startsWith("/api/")) return;

  if (event.request.mode === "navigate") {
    event.respondWith(
      fetch(event.request)
        .then((response) => {
          if (response.ok) caches.open(CACHE).then((cache) => cache.put("/", response.clone()));
          return response;
        })
        .catch(() => caches.match("/")),
    );
    return;
  }

  // Hashed Vite assets and icons are immutable. MathJax is network-first so an
  // app upgrade cannot mix an old loader with newly served component files.
  if (
    url.pathname.startsWith("/assets/") ||
    url.pathname.startsWith("/icons/") ||
    url.pathname.startsWith("/pdfjs/")
  ) {
    event.respondWith(
      caches.match(event.request).then(
        (cached) =>
          cached ||
          fetch(event.request).then((response) => {
            if (response.ok) caches.open(CACHE).then((cache) => cache.put(event.request, response.clone()));
            return response;
          }),
      ),
    );
    return;
  }

  event.respondWith(
    fetch(event.request)
      .then((response) => {
        if (response.ok && url.pathname.startsWith("/mathjax/")) {
          caches.open(CACHE).then((cache) => cache.put(event.request, response.clone()));
        }
        return response;
      })
      .catch(async () => {
        const cached = await caches.match(event.request);
        return (
          cached ||
          new Response("オフラインです。PCとの接続を確認してください。", {
            status: 503,
            headers: { "Content-Type": "text/plain; charset=utf-8" },
          })
        );
      }),
  );
});
