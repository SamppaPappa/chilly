// Ajetaan Twitchin chat-sivulla (popout) Chillyssä: ladataan FrankerFaceZ samalla tavalla
// kuin FFZ:n virallinen userscript, ja BTTV- + 7TV-emojit päälle oletuksena.
(() => {
  if (window.top !== window || window.__chillyChat) return;
  window.__chillyChat = true;

  // Chilly asettaa BTTV- ja 7TV-lisäosat asetusten mukaan (window.__chillyAddons tulee Rustista).
  // Käyttäjän FFZ-valikosta itse lisäämät muut lisäosat säilyvät.
  try {
    const KEY = "FFZ:setting:addons.enabled";
    const want = window.__chillyAddons || { "ffzap-bttv": true, "7tv-emotes": true };
    let list = [];
    try { list = JSON.parse(localStorage.getItem(KEY)) || []; } catch (e) {}
    for (const [id, on] of Object.entries(want)) {
      list = list.filter((x) => x !== id);
      if (on) list.push(id);
    }
    localStorage.setItem(KEY, JSON.stringify(list));
  } catch (e) {}

  // Chatin täydennys (FFZ) valmiiksi 7TV:n tapaan: Tab täydentää emojit ilman kaksoispistettä,
  // alusta alkavat osumat ja suosikit ensin, emojit (Unicode) pois listasta ja lähde näkyviin.
  // FFZ tallentaa asetukset profiileittain omaan tallennukseensa, joten ne asetetaan FFZ:n
  // rajapinnan kautta, kun FFZ on latautunut. Vain kerran ja vain asetukset, joita käyttäjä ei
  // ole itse muuttanut: omat muutokset chatin lisäasetuksissa säilyvät.
  const FFZ_DEFAULTS = {
    "chat.tab-complete.emotes-without-colon": true,
    "chat.tab-complete.prioritize-prefix-matches": true,
    "chat.tab-complete.prioritize-favorites": true,
    "chat.tab-complete.emoji": false,
    "chat.tab-complete.show-source": 1,
  };
  const DEFAULTS_DONE = "chilly:ffzDefaults";
  const applyFfzDefaults = (tries) => {
    try {
      if (Number(localStorage.getItem(DEFAULTS_DONE) || 0) >= 1) return;
      const ffz = window.FrankerFaceZ && window.FrankerFaceZ.get && window.FrankerFaceZ.get();
      const st = ffz && ffz.resolve && ffz.resolve("settings");
      const p0 = st && st.enabled !== false && st.profile && st.profile(0);
      if (!p0) { if (tries > 0) setTimeout(() => applyFfzDefaults(tries - 1), 1000); return; }
      for (const [k, v] of Object.entries(FFZ_DEFAULTS)) if (!p0.has(k)) p0.set(k, v);
      localStorage.setItem(DEFAULTS_DONE, "1");
    } catch (e) {}
  };
  setTimeout(() => applyFfzDefaults(60), 2000);

  // Linkit: WebView2:n "uusi ikkuna" ei välity Chillylle näistä näkymistä, joten klikkaus siepataan
  // tässä ja Chilly avaa linkin itse (Twitch-kanava → Chillyn välilehti, muu → oma selain).
  const openLink = (href) => {
    try {
      const u = new URL(href, location.href);
      if (u.protocol !== "https:" && u.protocol !== "http:") return false;
      window.chrome.webview.postMessage("chilly:open:" + u.href);
      return true;
    } catch (e) { return false; }
  };
  const onLink = (e) => {
    if (e.type === "auxclick" && e.button !== 1) return;
    const a = e.target && e.target.closest ? e.target.closest("a[href]") : null;
    if (!a) return;
    let u;
    try { u = new URL(a.href, location.href); } catch (err) { return; }
    const twitch = /(^|\.)twitch\.tv$/.test(u.hostname);
    // Chatin omat sisäiset linkit (samassa näkymässä) saavat toimia normaalisti
    if (twitch && a.target !== "_blank" && e.type === "click") return;
    if (openLink(u.href)) { e.preventDefault(); e.stopPropagation(); }
  };
  document.addEventListener("click", onLink, true);
  document.addEventListener("auxclick", onLink, true);
  const origOpen = window.open;
  window.open = function (url, ...rest) {
    // FFZ:n "Ohjauskeskus" avaisi asetukset uuteen selainikkunaan → Chilly avaa ne omaan ikkunaansa
    if (url && /[?&]ffz-settings/.test(String(url))) {
      try { window.chrome.webview.postMessage("chilly:ffzsettings"); } catch (e) {}
      return null;
    }
    if (url && openLink(url)) return null;
    return origOpen.call(window, url, ...rest);
  };

  const load = () => {
    if (document.getElementById("ffz-loader") || (document.body && document.body.dataset.ffzSource)) return;
    const s = document.createElement("script");
    s.id = "ffz-loader";
    s.crossOrigin = "anonymous";
    s.src = "https://cdn2.frankerfacez.com/script/script.min.js?_=" + Date.now();
    (document.head || document.documentElement).appendChild(s);
  };
  if (document.readyState === "loading") document.addEventListener("DOMContentLoaded", load);
  else load();

  // Chillyn tyyli Twitchin chattiin. Twitchin CSP estää <style>-tagit, mutta ohjelmallisesti
  // lisätty tyylitiedosto (adoptedStyleSheets) on sallittu. Twitch käyttää värimuuttujia,
  // joten teema vaihdetaan niiden kautta; korostuskortit (äänestykset, ennustukset, tavoitteet,
  // hype train) saavat Chillyn korttityylin. Mainokset (drops-tarjoukset, lahjatilausten
  // tulostaulu) piilotetaan kokonaan.
  const CSS = `
    html, :root, .tw-root--theme-dark {
      --color-background-base: #101218 !important; --color-background-body: #101218 !important;
      --color-background-alt: #1a1e29 !important; --color-background-alt-2: #1f2431 !important;
      --color-background-float: #161a24 !important; --color-border-base: #2e3446 !important;
      --color-text-base: #eef1f7 !important; --color-text-alt-2: #a7afc2 !important;
      --font-base: "Segoe UI Variable Text", "Segoe UI", system-ui, sans-serif !important;
    }
    body { background: #101218 !important; }
    .stream-chat-header { background: #12151c !important; border-color: #232836 !important; }
    /* Kortin reunus piirretään varjolla eikä borderilla/paddingilla: Twitch mittaa kortin korkeuden
       itse (esim. raid-kortti), ja ylimääräinen tila leikkasi kortin ylä- ja alareunan. */
    .community-highlight {
      background: #161a24 !important; border-radius: 12px !important;
      box-shadow: inset 0 0 0 1px #2a3042, 0 8px 22px rgba(0,0,0,.35) !important;
    }
    .chat-wysiwyg-input__box, .chat-input__textarea textarea { background: #0d0f14 !important; border-radius: 10px !important; }
    .community-highlight-stack__card:has([class*="dropsHighlight"]),
    [class*="channel-leaderboard"] { display: none !important; }
  `;
  try {
    const sheet = new CSSStyleSheet();
    sheet.replaceSync(CSS);
    document.adoptedStyleSheets = [...document.adoptedStyleSheets, sheet];
  } catch (e) {}
})();
