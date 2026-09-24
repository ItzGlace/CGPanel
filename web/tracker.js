(() => {
  "use strict";
  const script = document.currentScript;
  if (!script || navigator.doNotTrack === "1" || window.doNotTrack === "1")
    return;
  const site = script.dataset.site,
    key = script.dataset.key;
  if (!site || !key) return;
  const endpoint = new URL("collect/" + site, script.src).href;
  let sent = 0,
    clicks = 0,
    lastClick = 0;
  const send = (event) => {
    if (++sent > 100) return;
    const body = JSON.stringify({
      key,
      path: location.pathname,
      viewport: innerWidth < 768 ? "mobile" : "desktop",
      ...event,
    });
    navigator.sendBeacon(
      endpoint,
      new Blob([body], { type: "application/json" }),
    );
  };
  const page = () => {
    let referrer = "";
    try {
      referrer = new URL(document.referrer).hostname;
    } catch {}
    send({ kind: "view", referrer });
  };
  page();
  addEventListener("popstate", page);
  document.addEventListener(
    "click",
    (event) => {
      if (!event.isTrusted || ++clicks > 60 || Date.now() - lastClick < 400)
        return;
      lastClick = Date.now();
      const element =
        event.target instanceof Element
          ? event.target.closest(
              "[data-cgp-label],a,button,input,select,textarea",
            )
          : null;
      // Never capture text, form values, URLs, IDs or arbitrary DOM content.
      if (element?.matches("input,select,textarea")) return;
      send({
        kind: "click",
        x: Math.min(
          999,
          Math.round((event.clientX / Math.max(1, innerWidth)) * 1000),
        ),
        y: Math.min(
          999,
          Math.round(
            ((event.clientY + scrollY) /
              Math.max(1, document.documentElement.scrollHeight)) *
              1000,
          ),
        ),
        target: (
          element?.getAttribute("data-cgp-label") ||
          element?.tagName?.toLowerCase() ||
          "page"
        ).slice(0, 64),
      });
    },
    { passive: true },
  );
  window.CGPanelAnalytics = { page };
})();
