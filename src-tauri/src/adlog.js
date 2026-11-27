// KEHITYSVERSION TESTILOKI (ei koskaan julkaistussa Chillyssä): kirjaa mainokset, jumit, laadun
// ja viiveen. Tapahtumat lähetetään Chillylle WebView2-viestinä "chilly:log:{json}".
(() => {
  if (window.top !== window || window.__chillyAdLogOn) return;
  window.__chillyAdLogOn = true;
  const send = (ev, data) => {
    try { window.chrome.webview.postMessage("chilly:log:" + JSON.stringify(Object.assign({ ev, path: location.pathname }, data || {}))); } catch (e) {}
  };

  // 1) Mainoseston (vaft) omat konsoliviestit: mainos havaittu / ohitettu / soitin ladattu uudelleen
  const KEY = /ad|reload|backup|stripp|midroll|preroll|commercial/i;
  let lastMsg = "", lastAt = 0;
  for (const level of ["log", "info", "warn", "error"]) {
    const orig = console[level].bind(console);
    console[level] = (...args) => {
      try {
        const s = args.map((a) => (typeof a === "string" ? a : (a && a.message) || "")).join(" ").slice(0, 220);
        if (s.includes("[AD DEBUG]") || (/vaft|TwitchAdSolutions/i.test(s) && KEY.test(s))) {
          const now = Date.now();
          if (s !== lastMsg || now - lastAt > 5000) { send("vaft", { level, msg: s }); lastMsg = s; lastAt = now; }
        }
      } catch (e) {}
      return orig(...args);
    };
  }

  // 1b) Twitchin omat seurantatapahtumat (spade): video_ad_impression = mainos oikeasti näytetty,
  // minute-watched = Twitch laskee katseluaikaa. Kuunnellaan fetch- ja sendBeacon-kutsuja.
  const trackCounts = {};
  window.__trkCounts = trackCounts;
  function countEvents(txt) {
    const re = /"event"\s*:\s*"([A-Za-z0-9_-]+)"/g;
    let x;
    while ((x = re.exec(txt))) {
      const ev = x[1];
      if (/^(minute-watched|video-play|video_ad_.*|ad_.*)$/i.test(ev)) {
        trackCounts[ev] = (trackCounts[ev] || 0) + 1;
        if (/impression|video_ad_request$/i.test(ev)) send("track", { event: ev });
      }
    }
  }
  async function decode(str) {
    let bin;
    try { bin = atob(str); } catch (e) { return str; }
    if (bin.charCodeAt(0) === 0x1f && bin.charCodeAt(1) === 0x8b && window.DecompressionStream) {
      const bytes = Uint8Array.from(bin, (c) => c.charCodeAt(0));
      const ds = new Blob([bytes]).stream().pipeThrough(new DecompressionStream("gzip"));
      return await new Response(ds).text();
    }
    try { return decodeURIComponent(escape(bin)); } catch (e) { return bin; }
  }
  window.__trk2 = [];
  function scanTrack(body) {
    try {
      const s = typeof body === "string" ? body : (body instanceof URLSearchParams ? body.toString() : "");
      if (!s || !/SendSpadeEvents|"event"|data=/.test(s)) return;
      if (s.includes("SendSpadeEvents")) {
        const j = JSON.parse(s);
        const inp = (j && j.variables && j.variables.input) || {};
        if (window.__trk2.length < 3) window.__trk2.push(JSON.stringify(inp).slice(0, 300));
        countEvents(s);
        for (const v of Object.values(inp)) {
          if (typeof v === "string" && v.length > 40) decode(v).then(countEvents, () => {});
        }
        return;
      }
      const m = s.match(/(?:^|&)data=([^&]+)/);
      if (m) decode(decodeURIComponent(m[1])).then(countEvents, () => {});
      else countEvents(s);
    } catch (e) {}
  }
  window.__trk = [];
  function sample(u, b) {
    try { if (window.__trk.length < 60) window.__trk.push(String(u && u.url || u).slice(0, 80) + " | " + (typeof b) + " | " + String(typeof b === "string" ? b : (b && b.constructor && b.constructor.name)).slice(0, 60)); } catch (e) {}
  }
  const ofetch = window.fetch;
  window.fetch = function (input, init) {
    try { if (init && init.body && init.method && /post/i.test(init.method)) { sample(input, init.body); scanTrack(init.body); } } catch (e) {}
    return ofetch.apply(this, arguments);
  };
  if (navigator.sendBeacon) {
    const ob = navigator.sendBeacon.bind(navigator);
    navigator.sendBeacon = (url, data) => { sample(url, data); scanTrack(data); return ob(url, data); };
  }
  const oxhr = XMLHttpRequest.prototype.send;
  XMLHttpRequest.prototype.send = function (b) { scanTrack(b); return oxhr.apply(this, arguments); };

  // 2) Twitchin oma mainosmerkki soittimessa (näkyy, kun mainos oikeasti pyörii)
  const AD_SEL = '[data-a-target="video-ad-label"], [data-a-target="video-ad-countdown"], [data-test-selector="ad-banner-default-text"]';
  const AD_TXT = /^(ad|ads|mainos|mainokset)(\s*\d+\s*(\/|of|\/)\s*\d+)?\s*[(·:]?\s*\d*:?\d*\)?$/i;
  function adVisible() {
    const p = document.querySelector(".video-player");
    if (!p) return "";
    const el = p.querySelector(AD_SEL);
    if (el) return (el.getAttribute("data-a-target") || el.getAttribute("data-test-selector") || "ad") + ":" + (el.textContent || "").trim().slice(0, 40);
    for (const n of p.querySelectorAll("span, p, div")) {
      if (n.children.length) continue;
      const t = (n.textContent || "").trim();
      if (t.length < 24 && AD_TXT.test(t)) return "text:" + t;
    }
    return "";
  }

  // 3) Videon tila: jumit, laatu, viive
  let mpi = null;
  function latency() {
    try {
      if (!mpi || !mpi.videoElement || !mpi.videoElement.isConnected) {
        mpi = null;
        const el = document.querySelector('[data-a-target="video-player"]');
        const key = el && Object.keys(el).find((k) => k.startsWith("__reactFiber$"));
        for (let f = key ? el[key] : null; f && !mpi; f = f.return) { const p = f.memoizedProps; if (p && p.mediaPlayerInstance) mpi = p.mediaPlayerInstance; }
      }
      const v = mpi && mpi.getLiveLatency ? mpi.getLiveLatency() : null;
      return typeof v === "number" ? Math.round(v * 10) / 10 : null;
    } catch (e) { return null; }
  }
  let ad = "", adSince = 0, lastT = -1, stallSince = 0, stallTotal = 0, stalls = 0, adsSeen = 0, lastBeat = Date.now(), started = false;
  setInterval(() => {
    const v = document.querySelector('[data-a-target="video-player"] video');
    const now = Date.now();
    const a = adVisible();
    if (a && !ad) { ad = a; adSince = now; adsSeen++; send("ad_start", { marker: a }); }
    else if (!a && ad) { send("ad_end", { marker: ad, secs: Math.round((now - adSince) / 1000) }); ad = ""; }
    if (v) {
      const t = v.currentTime;
      if (!started && t > 0) { started = true; send("play_start", { h: v.videoHeight }); }
      const moving = t !== lastT;
      if (started && !v.paused && !moving) {
        if (!stallSince) stallSince = now;
      } else if (stallSince) {
        const secs = Math.round((now - stallSince) / 1000);
        if (secs >= 3) { stalls++; stallTotal += secs; send("stall", { secs, h: v.videoHeight }); }
        stallSince = 0;
      }
      lastT = t;
    }
    if (now - lastBeat >= 60000) {
      lastBeat = now;
      send("beat", { t: v ? Math.round(v.currentTime) : null, paused: v ? v.paused : null, h: v ? v.videoHeight : null,
                     lat: latency(), stalls, stallTotal, adsSeen, track: trackCounts });
    }
  }, 1000);
  // Testiprofiili on kirjautumaton: ohitetaan sisältöluokituksen portti ("Aloita katselu"),
  // hylätään evästeet ja käynnistetään taustalla pysähtynyt video, jotta striimi oikeasti pyörii.
  const clickText = (re) => {
    for (const b of document.querySelectorAll("button")) {
      if (re.test((b.textContent || "").trim())) { b.click(); return true; }
    }
    return false;
  };
  setInterval(() => {
    const gate = document.querySelector('[data-a-target="content-classification-gate-overlay-start-watching-button"]');
    if (gate) { gate.click(); send("gate_clicked", {}); }
    else if (clickText(/^(Aloita katselu|Start Watching)$/i)) send("gate_clicked", { by: "text" });
    if (clickText(/^(Hylkää|Reject)$/i)) send("cookies_rejected", {});
    const v = document.querySelector('[data-a-target="video-player"] video');
    if (v && v.paused && v.readyState >= 2) { v.play().then(() => send("forced_play", {}), () => {}); }
  }, 5000);
  send("load", {});
})();
