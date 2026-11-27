// Ajetaan twitch.tv-kanavasivulla Chillyn katseluikkunassa.
// Nostaa videosoittimen koko näkymän kokoiseksi. Muu sivu (myös chat, jossa
// kanavapisteiden bonusarkku on) jää alle ladattuna, jotta pisteet toimivat.
// Tyylit asetetaan suoraan elementteihin, koska Twitchin CSP estää lisätyt <style>-tagit.
(() => {
  if (window.top !== window || window.__chilly) return;
  window.__chilly = true;

  const set = (el, props) => {
    for (const [k, v] of Object.entries(props)) {
      if (el.style.getPropertyValue(k) !== v || el.style.getPropertyPriority(k) !== "important") {
        el.style.setProperty(k, v, "important");
      }
    }
  };

  const PLAYER = {
    position: "fixed", top: "0", left: "0", right: "0", bottom: "0",
    width: "100vw", height: "100vh", "max-width": "none", "max-height": "none",
    margin: "0", "z-index": "2147483646", background: "#000",
  };
  // Yläelementit nostetaan muun sivun päälle, muuten niiden pinoamisjärjestys jättää soittimen alle.
  const ANCESTOR = { transform: "none", contain: "none", filter: "none", "will-change": "auto", perspective: "none",
                     "z-index": "2147483646", overflow: "visible" };

  const FILL = { width: "100%", height: "100%", "max-width": "none", "max-height": "none", "padding-bottom": "0" };
  const VIDEO = { position: "absolute", top: "0", left: "0", width: "100%", height: "100%", "object-fit": "contain" };

  let last = null;
  function fix() {
    const root = document.documentElement;
    if (root) set(root, { overflow: "hidden", background: "#000" });
    if (document.body) set(document.body, { overflow: "hidden", background: "#000" });

    const v = document.querySelector('[data-a-target="video-player"]');
    if (!v) return;
    const p = v.closest(".video-player") || v;
    if (last && last !== p) last.removeAttribute("style");
    last = p;
    set(p, PLAYER);
    for (let a = p.parentElement; a && a !== root; a = a.parentElement) set(a, ANCESTOR);

    // Soittimen ja videon väliset kääreet täyttämään tila, video keskelle oikeassa kuvasuhteessa
    const vid = p.querySelector("video");
    if (vid) {
      for (let a = vid.parentElement; a && a !== p; a = a.parentElement) set(a, FILL);
      set(vid, VIDEO);
    }
  }

  // Kanavasivun oma chat-palsta on soittimen alla näkymättömissä mutta pyörisi silti
  // (yhteys chattiin, viestien piirto). Chillyllä on oma chat-ikkuna, joten palsta suljetaan.
  let chatCollapsed = false;
  function collapseChat() {
    if (chatCollapsed) return;
    const col = document.querySelector(".channel-root__right-column--expanded");
    const btn = document.querySelector('[data-a-target="right-column__toggle-collapse-btn"]');
    if (col && btn) { btn.click(); chatCollapsed = true; }
    else if (document.querySelector(".channel-root__right-column") && !col) chatCollapsed = true;
  }

  // Twitch näyttää korostuskortit (drops-mainokset, äänestykset, tavoitteet) videon päällä, kun
  // kanavasivun chat on suljettu. Ne näkyvät jo Chillyn chatissa, joten videon päältä ne piilotetaan.
  // Ohjelmallinen tyylitiedosto ohittaa Twitchin CSP:n, joka estää <style>-tagit.
  try {
    const sheet = new CSSStyleSheet();
    sheet.replaceSync(`
      [class*="community-highlight-stack"], [class*="dropsHighlight"], .community-highlight,
      [class*="pinned-chat__highlight-card"] { display: none !important; }
      /* Soittimen asetusvalikko (laatu ym.) ja muut Twitchin ponnahdusikkunat piirretään sivun
         juureen omaan kerrokseensa. Soitin on nostettu sivun päällimmäiseksi, joten nämä nostetaan
         vielä sen yläpuolelle – muuten valikko jää videon alle näkymättömiin. */
      .ReactModalPortal, [class*="ScReactModalBase"], .ReactModal__Overlay,
      [class*="tw-dialog-layer"], [data-popper-placement] { z-index: 2147483647 !important; }
    `);
    document.adoptedStyleSheets = [...document.adoptedStyleSheets, sheet];
  } catch (e) {}

  // Multistream "ääni molemmissa": oma mykistysnappi videon nurkassa. Chilly kertoo tilan
  // (window.__chillyMuteBtn), napin painallus lähetetään WebView2:n viestinä Chillylle, joka
  // mykistää streamin WebView2-tasolla (Twitch laskee katselun silti).
  const SVG = "http://www.w3.org/2000/svg";
  const ICONS = {
    on: ["M11 4.702a.705.705 0 0 0-1.203-.498L6.413 7.587A1.4 1.4 0 0 1 5.416 8H3a1 1 0 0 0-1 1v6a1 1 0 0 0 1 1h2.416a1.4 1.4 0 0 1 .997.413l3.383 3.384A.705.705 0 0 0 11 19.298z",
         "M16 9a5 5 0 0 1 0 6", "M19.364 18.364a9 9 0 0 0 0-12.728"],
    off: ["M11 4.702a.705.705 0 0 0-1.203-.498L6.413 7.587A1.4 1.4 0 0 1 5.416 8H3a1 1 0 0 0-1 1v6a1 1 0 0 0 1 1h2.416a1.4 1.4 0 0 1 .997.413l3.383 3.384A.705.705 0 0 0 11 19.298z",
          "M22 9l-6 6", "M16 9l6 6"],
  };
  // Napit näkyvät vain, kun hiiri liikkuu videon päällä, ja häipyvät ~2 s kuluttua (kuten Twitchin säätimet).
  let hoverOn = false, hoverTimer = null;
  const BTN = {
    position: "fixed", top: "10px", right: "10px", width: "38px", height: "38px", padding: "0",
    "border-radius": "10px", border: "1px solid rgba(255,255,255,.18)", background: "rgba(13,15,20,.72)",
    color: "#fff", cursor: "pointer", "z-index": "2147483647", display: "flex",
    "align-items": "center", "justify-content": "center", "backdrop-filter": "blur(6px)",
    transition: "opacity .25s ease",
  };
  function mkBtn(msg) {
    const b = document.createElement("button");
    set(b, BTN);
    b.addEventListener("click", (e) => {
      e.stopPropagation();
      try { window.chrome.webview.postMessage(msg); } catch (err) {}
    });
    // Hiiri napin päällä → pysyy näkyvissä
    b.addEventListener("mouseenter", () => { clearTimeout(hoverTimer); hoverOn = true; drawButtons(); });
    b.addEventListener("mouseleave", () => pokeHover());
    document.body.appendChild(b);
    return b;
  }
  function svgIcon(paths, stroke) {
    const svg = document.createElementNS(SVG, "svg");
    for (const [k, v] of Object.entries({ width: "20", height: "20", viewBox: "0 0 24 24", fill: "none", stroke,
      "stroke-width": "2", "stroke-linecap": "round", "stroke-linejoin": "round" })) svg.setAttribute(k, v);
    for (const d of paths) { const p = document.createElementNS(SVG, "path"); p.setAttribute("d", d); svg.appendChild(p); }
    return svg;
  }
  let muteBtn = null, closeBtn = null;
  function drawButtons() {
    drawMute(window.__chillyMuteState);
    drawClose(!!window.__chillyCloseBtn);
  }
  function drawClose(on) {
    if (!on) { if (closeBtn) closeBtn.style.setProperty("display", "none", "important"); return; }
    if (!document.body) return;
    if (!closeBtn || !closeBtn.isConnected) {
      closeBtn = mkBtn("chilly:close");
      closeBtn.title = window.__chillyLang === "en" ? "Close this stream" : "Sulje tämä striimi";
      closeBtn.replaceChildren(svgIcon(["M18 6 6 18", "m6 6 12 12"], "currentColor"));
    }
    const c = corner();
    set(closeBtn, { top: Math.round(c.top) + "px", right: Math.round(c.right) + "px", display: "flex", opacity: hoverOn ? "1" : "0",
                    "pointer-events": hoverOn ? "auto" : "none" });
  }
  // Videokuvan oikea yläkulma (ei mustalle reunalle: siellä nappi ei piirry videon päälle)
  function corner() {
    let top = 10, right = 10;
    const vid = document.querySelector('[data-a-target="video-player"] video');
    if (vid && vid.videoWidth && vid.videoHeight) {
      const r = vid.getBoundingClientRect();
      const s = Math.min(r.width / vid.videoWidth, r.height / vid.videoHeight);
      const w = vid.videoWidth * s, h = vid.videoHeight * s;
      top = Math.max(0, r.top + (r.height - h) / 2) + 10;
      right = Math.max(0, innerWidth - (r.left + (r.width + w) / 2)) + 10;
    }
    return { top, right };
  }
  function drawMute(state) {
    if (state === null || state === undefined) { if (muteBtn) muteBtn.style.setProperty("display", "none", "important"); return; }
    if (!document.body) return;
    if (!muteBtn || !muteBtn.isConnected) muteBtn = mkBtn("chilly:mute");
    const key = state ? "off" : "on";
    if (muteBtn.dataset.k !== key) {
      muteBtn.dataset.k = key;
      muteBtn.title = window.__chillyLang === "en" ? (state ? "Unmute" : "Mute") : (state ? "Poista mykistys" : "Mykistä");
      muteBtn.replaceChildren(svgIcon(ICONS[key], state ? "#ff4757" : "currentColor"));
    }
    // Sulkunappi kulmassa → mykistys sen vasemmalle puolelle
    const c = corner();
    const right = c.right + (window.__chillyCloseBtn ? 46 : 0);
    // Vain hiiren alla (päätetty: ✕ ja mykistys eivät jää pysyvästi videon päälle)
    const show = hoverOn;
    set(muteBtn, { top: Math.round(c.top) + "px", right: Math.round(right) + "px", display: "flex", opacity: show ? "1" : "0",
                   "pointer-events": show ? "auto" : "none" });
  }
  function pokeHover() {
    hoverOn = true;
    drawButtons();
    clearTimeout(hoverTimer);
    hoverTimer = setTimeout(() => { hoverOn = false; drawButtons(); }, 2000);
  }
  window.__chillyMuteBtn = () => drawButtons();

  // Striimin tiedot (nimi, otsikko, peli, katsojat) videon vasempaan yläkulmaan, kun hiiri liikkuu
  // kuvan päällä. Tiedot luetaan kanavasivulta soittimen alta, joten ne päivittyvät livenä.
  let info = null, infoTimer = null, infoOn = false;
  const txt = (sel) => { const e = document.querySelector(sel); return e ? e.textContent.trim() : ""; };
  function videoBox() {
    const vid = document.querySelector('[data-a-target="video-player"] video');
    if (!vid || !vid.videoWidth || !vid.videoHeight) return { x: 0, y: 0, w: innerWidth, h: innerHeight };
    const r = vid.getBoundingClientRect();
    const s = Math.min(r.width / vid.videoWidth, r.height / vid.videoHeight);
    const w = vid.videoWidth * s, h = vid.videoHeight * s;
    return { x: r.left + (r.width - w) / 2, y: Math.max(0, r.top + (r.height - h) / 2), w, h };
  }
  function buildInfo() {
    const el = (tag, props) => { const e = document.createElement(tag); set(e, props); return e; };
    const root = el("div", {
      // Soittimen päälle mutta Twitchin valikkojen ja mykistysnapin alle
      position: "fixed", "z-index": "2147483646", "pointer-events": "none", "box-sizing": "border-box",
      display: "flex", gap: "11px", "align-items": "flex-start", padding: "12px 64px 34px 14px",
      background: "linear-gradient(rgba(8,10,14,.86), rgba(8,10,14,.55) 60%, transparent)",
      "font-family": "'Segoe UI Variable Text','Segoe UI',system-ui,sans-serif", color: "#fff",
      opacity: "0", transition: "opacity .18s ease", "text-shadow": "0 1px 2px rgba(0,0,0,.6)",
    });
    const av = el("img", { width: "38px", height: "38px", "border-radius": "50%", flex: "none", "object-fit": "cover",
                           border: "2px solid #ff4757", "box-sizing": "border-box" });
    const col = el("div", { display: "flex", "flex-direction": "column", gap: "2px", "min-width": "0" });
    const row = el("div", { display: "flex", "align-items": "center", gap: "10px", "font-size": "15px", "font-weight": "700" });
    const name = el("span", { "white-space": "nowrap" });
    const views = el("span", { display: "inline-flex", "align-items": "center", gap: "5px", "font-size": "12.5px",
                               "font-weight": "600", color: "#ff8a95", "white-space": "nowrap" });
    const dot = el("span", { width: "7px", height: "7px", "border-radius": "50%", background: "#ff4757", display: "inline-block" });
    const vnum = el("span", {});
    views.append(dot, vnum);
    const up = el("span", { "font-size": "12.5px", "font-weight": "600", "white-space": "nowrap", color: "rgba(255,255,255,.75)" });
    const lat = el("span", { "font-size": "12.5px", "font-weight": "600", "white-space": "nowrap" });
    row.append(name, views, up, lat);
    const title = el("div", { "font-size": "13.5px", "line-height": "1.35", color: "rgba(255,255,255,.92)", overflow: "hidden",
                              display: "-webkit-box", "-webkit-line-clamp": "2", "-webkit-box-orient": "vertical", "word-break": "break-word" });
    const game = el("div", { "font-size": "12.5px", "font-weight": "600", color: "#22d3ee", "white-space": "nowrap",
                             overflow: "hidden", "text-overflow": "ellipsis" });
    col.append(row, title, game);
    root.append(av, col);
    document.body.appendChild(root);
    return { root, av, name, views, vnum, up, lat, title, game };
  }
  function updateInfo() {
    if (!document.body) return;
    if (!info || !info.root.isConnected) info = buildInfo();
    const t = txt('[data-a-target="stream-title"]');
    const show = infoOn && !!t;
    const b = videoBox();
    set(info.root, { left: Math.round(b.x) + "px", top: Math.round(b.y) + "px", width: Math.round(b.w) + "px",
                     opacity: show ? "1" : "0" });
    if (!show) return;
    info.name.textContent = txt(".channel-info-content h1") || txt("h1.tw-title");
    info.title.textContent = t;
    info.game.textContent = txt('[data-a-target="stream-game-link"]');
    const v = txt('[data-a-target="animated-channel-viewers-count"]');
    info.vnum.textContent = v;
    info.views.style.setProperty("display", v ? "inline-flex" : "none", "important");
    // Kuinka kauan striimi on ollut livenä: Twitch näyttää sen kanavasivulla muodossa h:mm:ss (fi: h.mm.ss)
    const u = uptime();
    if (!u) info.up.style.setProperty("display", "none", "important");
    else {
      info.up.textContent = (window.__chillyLang === "en" ? "Live " : "Livenä ") + u;
      set(info.up, { display: "inline" });
    }
    // Viive lähetykseen Twitchin soittimelta: normaali / keltainen yli 10 s / punainen yli 30 s
    const l = liveLatency();
    if (l == null) info.lat.style.setProperty("display", "none", "important");
    else {
      const en = window.__chillyLang === "en";
      info.lat.textContent = (en ? "Delay " : "Viive ") + (en ? l.toFixed(1) : l.toFixed(1).replace(".", ",")) + " s";
      set(info.lat, { display: "inline", color: l > 30 ? "#ff4757" : l > 10 ? "#ffd166" : "rgba(255,255,255,.75)" });
    }
    const img = document.querySelector(".channel-info-content img.tw-image-avatar");
    if (img && img.src && info.av.src !== img.src) info.av.src = img.src;
    info.av.style.setProperty("display", img ? "block" : "none", "important");
  }
  function uptime() {
    const s = txt('.live-time span[aria-hidden="true"]') || txt(".live-time");
    const n = (s.match(/\d+/g) || []).map(Number);
    if (n.length < 2) return "";
    const [h, m] = n.length >= 3 ? [n[0], n[1]] : [0, n[0]];
    return h ? h + " h " + m + " min" : m + " min";
  }
  // Twitchin soitin-olio löytyy Reactin sisältä; haetaan kerran ja uudelleen vain, jos soitin vaihtuu.
  let mpi = null;
  function liveLatency() {
    try {
      if (!mpi || !mpi.videoElement || !mpi.videoElement.isConnected) {
        mpi = null;
        const el = document.querySelector('[data-a-target="video-player"]');
        const key = el && Object.keys(el).find((k) => k.startsWith("__reactFiber$"));
        for (let f = key ? el[key] : null; f && !mpi; f = f.return) {
          const p = f.memoizedProps;
          if (p && p.mediaPlayerInstance) mpi = p.mediaPlayerInstance;
        }
      }
      const v = mpi && typeof mpi.getLiveLatency === "function" ? mpi.getLiveLatency() : null;
      return typeof v === "number" && isFinite(v) && v > 0 ? v : null;
    } catch (e) { return null; }
  }

  // Taustalla pyörivä striimi (Chilly asettaa window.__chillyBg) käännetään matalimmalle laadulle,
  // jotta katseluaika kertyy kevyesti. Kun välilehti tulee näkyviin, palautetaan entinen laatu.
  let bgSaved = null, bgCheck = 0;
  function bgQuality() {
    try {
      liveLatency(); // päivittää mpi:n
      if (!mpi || typeof mpi.getQualities !== "function") return;
      const bg = !!window.__chillyBg;
      if (bg) {
        const qs = (mpi.getQualities() || []).filter((q) => q && q.height > 0);
        if (!qs.length) return;
        const low = qs.reduce((a, b) => (b.height < a.height ? b : a));
        const cur = mpi.getQuality && mpi.getQuality();
        if (!bgSaved || bgSaved.mpi !== mpi) {
          bgSaved = { mpi, auto: mpi.isAutoQualityMode ? mpi.isAutoQualityMode() : true, q: cur };
        }
        const now = Date.now();
        if ((!cur || cur.group !== low.group) && now - bgCheck > 3000) { bgCheck = now; mpi.setQuality(low); }
      } else if (bgSaved) {
        const s = bgSaved;
        bgSaved = null;
        if (s.mpi !== mpi) return;
        if (s.auto || !s.q) { if (mpi.setAutoQualityMode) mpi.setAutoQualityMode(true); }
        else mpi.setQuality(s.q);
      }
    } catch (e) {}
  }

  // Äänenvahvistin: Chilly kertoo kertoimen (window.__chillyGain, 1 = pois). Video ohjataan WebAudion
  // kautta: vahvistus + rajoitin (limiter), joka estää säröilyn. Kytkentä tehdään vasta, kun vahvistusta
  // tarvitaan ensimmäisen kerran; sen jälkeen sama ketju pysyy (createMediaElementSource on kertaluonteinen).
  let ac = null, gainNode = null, boostVid = null;
  function boost() {
    try {
      const g = Number(window.__chillyGain) || 1;
      const vid = document.querySelector('[data-a-target="video-player"] video');
      if (!vid) return;
      if (vid !== boostVid) {
        if (g <= 1) return; // ei vahvistusta → ei kosketa ääneen lainkaan
        if (vid.__chillySrc) { boostVid = vid; return; }
        if (!ac) ac = new AudioContext();
        const src = ac.createMediaElementSource(vid);
        vid.__chillySrc = src;
        gainNode = ac.createGain();
        const lim = ac.createDynamicsCompressor();
        lim.threshold.value = -1; lim.knee.value = 0; lim.ratio.value = 20;
        lim.attack.value = 0.003; lim.release.value = 0.15;
        src.connect(gainNode).connect(lim).connect(ac.destination);
        boostVid = vid;
      }
      if (ac && ac.state === "suspended") ac.resume();
      if (gainNode && Math.abs(gainNode.gain.value - g) > 0.001) gainNode.gain.value = g;
    } catch (e) {}
  }

  function pokeInfo() {
    infoOn = true;
    updateInfo();
    clearTimeout(infoTimer);
    infoTimer = setTimeout(() => { infoOn = false; updateInfo(); }, 2500);
  }
  document.addEventListener("mousemove", () => { pokeInfo(); pokeHover(); }, true);
  // Hiiri poistui sivulta (relatedTarget puuttuu). Huom: skripti ajetaan ennen kuin <html> on olemassa,
  // joten kuuntelijat vain window/document-tasolle.
  window.addEventListener("mouseout", (e) => {
    if (e.relatedTarget) return;
    clearTimeout(infoTimer); infoOn = false; updateInfo();
    clearTimeout(hoverTimer); hoverOn = false; drawButtons();
  });

  // Raid (tai muu siirtyminen toisen kanavan sivulle) tapahtuu sivun sisällä ilman uutta latausta.
  // Kerrotaan Chillylle uusi kanava, jotta välilehti, chat ja napit seuraavat mukana.
  const RESERVED = new Set(["directory", "search", "settings", "subscriptions", "inventory", "drops", "wallet",
    "friends", "messages", "downloads", "turbo", "prime", "jobs", "p", "store", "videos", "popout", "moderator", "u"]);
  function channelOfPage() {
    if (location.hostname !== "www.twitch.tv" && location.hostname !== "twitch.tv") return "";
    const m = location.pathname.match(/^\/([A-Za-z0-9_]{2,25})\/?$/);
    const l = m ? m[1].toLowerCase() : "";
    return RESERVED.has(l) ? "" : l;
  }
  // Tyhjä alussa: myös kokonaan uudelleen latautunut sivu ilmoittaa kanavansa (Chilly ohittaa, jos sama)
  let lastChannel = "";
  function checkChannel() {
    const c = channelOfPage();
    if (!c || c === lastChannel) return;
    if (lastChannel) chatCollapsed = false; // uuden kanavan sivulla chat-palsta on taas auki → suljetaan uudelleen
    lastChannel = c;
    try { window.chrome.webview.postMessage("chilly:nav:" + c); } catch (e) {}
  }

  let helloSent = false;
  setInterval(() => {
    checkChannel();
    fix(); collapseChat();
    drawButtons();
    if (infoOn) updateInfo();
    bgQuality();
    boost();
    // Sivu ladattu (myös vaihdon tai uudelleenlatauksen jälkeen) → pyydetään Chillyltä napin tila
    if (!helloSent && document.body) { helloSent = true; try { window.chrome.webview.postMessage("chilly:hello"); } catch (e) {} }
  }, 500);
  document.addEventListener("DOMContentLoaded", fix);
})();
