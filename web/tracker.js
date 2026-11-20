(() => {
  "use strict";
  const script = document.currentScript;
  if (!script || navigator.doNotTrack === "1" || window.doNotTrack === "1" || navigator.globalPrivacyControl === true)
    return;
  const site = script.dataset.site,
    key = script.dataset.key;
  if (!site || !key) return;
  const endpoint = new URL("collect/" + site, script.src).href;
  let sent = 0,
    clicks = 0,
    moves = 0,
    lastMove = 0,
    lastClick = 0;
  const send = (event) => {
    if (++sent > 100) return;
    const body = JSON.stringify({
      key,
      path: location.pathname,
      viewport: innerWidth < 768 ? "mobile" : "desktop",
      ...event,
    });
    const payload=new Blob([body], { type: "text/plain;charset=UTF-8" });
    if (!navigator.sendBeacon?.(endpoint,payload))
      fetch(endpoint,{method:"POST",body:payload,mode:"no-cors",credentials:"omit",keepalive:true}).catch(()=>{});
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
  for(const name of ["pushState","replaceState"]){const original=history[name];history[name]=function(...args){const before=location.pathname;const result=original.apply(this,args);if(location.pathname!==before)page();return result;};}
  if(script.dataset.pointer === 'true') document.addEventListener('pointermove',event=>{
    if(!event.isTrusted || event.pointerType==='touch' || moves>=20 || Date.now()-lastMove<3000 || event.target?.closest('input,select,textarea,[contenteditable]'))return;
    lastMove=Date.now();moves++;
    send({kind:'move',x:Math.min(999,Math.max(0,Math.round(event.clientX/Math.max(1,innerWidth)*1000))),y:Math.min(999,Math.max(0,Math.round((event.clientY+scrollY)/Math.max(1,document.documentElement.scrollHeight)*1000)))});
  },{passive:true});

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
