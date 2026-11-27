const { invoke } = window.__TAURI__.core;
const { listen } = window.__TAURI__.event;
const $ = (id) => document.getElementById(id);
hydrateIcons();
applyI18n();

let S = null;                        // Chillyn tila (kanavat, asetukset)
let V = { tabs: [], active: null };  // avoimet streamit
let filter = "all";
let openMenu = null;                 // kanava, jonka ⋯-valikko on auki
let settingsOpen = false;
let splashActive = false;
let lastCover = false;
let lastStatus = "";
let lastSide = null;
let webLogged = null;                // twitch.tv-istunto Chillyssä (chat, pisteet)
let hits = [], hitsFor = "";         // Twitch-haun ehdotukset
let selIdx = -1;                     // näppäimistöllä valittu rivi
let searchTimer = null;
const info = {};                     // kanavasivun tiedot loginin mukaan
const clipMode = new Set();          // välilehdet, joilla toistetaan klippiä
const chatOverride = new Set();      // offline-välilehdet, joille chat on avattu käsin
let chatShown = true;
let offShut = false;                 // Offline-osio kokoontaitettu kanavalistassa
try { offShut = localStorage.getItem("offShut") === "1"; } catch {}
try { chatShown = localStorage.getItem("chatShown") !== "0"; } catch {}

const SOUNDS = ["win:Default", "win:Messaging", "win:Email", "win:Calendar", "win:Nudge", "win:Proximity", "win:Chimes", "win:Tada", "none"];

// ---------- Apurit ----------

function esc(s) {
  return String(s ?? "").replace(/[&<>"']/g, (c) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;" }[c]));
}
function fileName(p) { return p.slice(5).split(/[\\/]/).pop(); }
function fmtViewers(n) {
  if (LANG === "en") return n >= 10000 ? (n / 1000).toFixed(0) + "K" : n >= 1000 ? (n / 1000).toFixed(1) + "K" : String(n);
  return n >= 10000 ? (n / 1000).toFixed(0) + " t." : n >= 1000 ? (n / 1000).toFixed(1).replace(".", ",") + " t." : String(n);
}
function soundOptions(current, withDefault) {
  const opts = [];
  if (withDefault) opts.push(["", t("snd.default")]);
  opts.push(...SOUNDS.map((k) => [k, t("snd." + k)]));
  if (current && current.startsWith("file:")) opts.push([current, "♪ " + fileName(current)]);
  opts.push(["__pick", t("snd.file")]);
  return opts.map(([v, t]) => `<option value="${esc(v)}"${v === current ? " selected" : ""}>${esc(t)}</option>`).join("");
}
function toast(msg, icon = "info") {
  const t = $("toast");
  t.innerHTML = ic(icon, "sm") + `<span>${esc(msg)}</span>`;
  t.classList.remove("hidden");
  clearTimeout(toast.timer);
  toast.timer = setTimeout(() => t.classList.add("hidden"), 5000);
}
async function chooseSound(sel, apply) {
  let v = sel.value;
  if (v === "__pick") {
    const f = await invoke("pick_sound_file");
    if (!f) { render(); return; }
    v = f;
  }
  await apply(v);
}

// ---------- Tila ----------

async function refresh() {
  const wasIn = !!(S && S.user);
  S = await invoke("get_state");
  render();
  if (!wasIn && S.user) { checkWeb(); loadMods(); } // juuri kirjauduttu → twitch.tv-istunto ja moderoitavat kanavat
  // Uudelleenkirjautuminen (lupa moderoitujen kanavien listaan) → haetaan lista heti uudelleen
  else if (S.user && S.modView && MOD.needScope && !S.loginPending) loadMods();
  checkWentLive();
}
// Päällä olevat näkymät (Löydä, asetukset, valitsin) suljetaan aina, kun aktiivinen stream vaihtuu
// mistä tahansa (lista, välilehti, ilmoitus, tray). Muuten ne jäävät streamin alle ja pilkistävät
// streamin ja chatin välistä sekä peittävät multistreamin chat-välilehdet.
let lastActive = null;
function syncCover() {
  const on = !!(splashActive || settingsOpen || D.open || pickerOpen);
  if (on !== lastCover) {
    lastCover = on;
    invoke("viewer_cover", { on }).catch(() => {});
  }
}
function closeOverlays() {
  closeDiscover();
  closeSettings();
  closePicker();
  closeClipBar();
  closeMs();
  closeBars();
}
async function refreshViewer() {
  V = await invoke("viewer_state");
  if (V.active !== lastActive) {
    if (lastActive !== null || V.active !== null) closeOverlays();
    lastActive = V.active;
  }
  for (const id of [...clipMode]) if (!V.tabs.some((t) => t.id === id)) clipMode.delete(id);
  renderTabs();
  // Kanavalista (satoja rivejä) piirretään uudelleen vain, jos valittu kanava vaihtui:
  // viewer-state tulee myös jokaisesta asettelun muutoksesta (esim. ikkunan koon muutos).
  const al = activeLogin();
  if (al !== refreshViewer.lastLogin) { refreshViewer.lastLogin = al; renderList(); }
  const cur = V.tabs.find((t) => t.id === V.active);
  if (cur && !info[cur.login]) loadInfo(cur.login);
}

function render() {
  if (!S) return;
  if (setLang(S.lang)) { chanRendered = ""; renderTabs(); if (pickerOpen) renderPicker(); if (D.open) runDiscover(); }
  const loggedIn = !!S.user;
  $("login").classList.toggle("hidden", loggedIn);
  $("app").classList.toggle("hidden", !loggedIn);

  if (!loggedIn) {
    $("noClientId").classList.toggle("hidden", S.clientIdSet);
    $("btnLogin").disabled = !S.clientIdSet;
    $("loginIdle").classList.toggle("hidden", !!S.loginPending);
    $("loginWait").classList.toggle("hidden", !S.loginPending);
    if (S.loginPending) $("userCode").textContent = S.loginPending.code;
    $("loginStatus").textContent = S.status || "";
    $("loginStatus").classList.toggle("hidden", !S.status);
    return;
  }

  const side = S.chatSide || "right";
  if (side === "below" && lastSide !== "below" && lastSide !== null) setCollapsed(true);
  lastSide = side;
  document.body.classList.toggle("chat-left", side === "left");
  document.body.classList.toggle("chat-right", side === "right");
  document.body.classList.toggle("chat-below", side === "below");
  applyChatSize();
  updateCollapseIcon();
  reportStage();

  const liveN = S.channels.filter((c) => c.live).length;
  $("liveCount").lastElementChild.textContent = t("side.liveCount", { n: liveN });
  $("liveCount").classList.toggle("hidden", liveN === 0);
  $("meName").textContent = S.user.name;
  $("meSub").textContent = "@" + S.user.login;
  $("meAv").innerHTML = S.user.image ? `<img src="${esc(S.user.image)}" alt="">` : esc((S.user.name || "?")[0].toUpperCase());
  if (S.status && S.status !== lastStatus) toast(S.status);
  lastStatus = S.status || "";

  // Asetukset
  $("globalSound").innerHTML = soundOptions(S.sound, false);
  $("interval").value = String(S.interval);
  $("popupSecs").value = String(S.popupSecs);
  document.querySelectorAll("#chatSide button").forEach((b) => b.classList.toggle("on", b.dataset.side === S.chatSide));
  document.querySelectorAll("#langSeg button").forEach((b) => b.classList.toggle("on", b.dataset.lang === LANG));
  document.querySelectorAll("#multiAudio button").forEach((b) => b.classList.toggle("on", (b.dataset.v === "1") === !!S.multiAudioBoth));
  document.querySelectorAll("#multiChat button").forEach((b) => b.classList.toggle("on", (b.dataset.v === "1") === !!S.multiChatBoth));
  $("hwAccel").checked = S.hwAccel;
  document.querySelectorAll("#boostSeg button").forEach((b) => b.classList.toggle("on", Number(b.dataset.b) === (S.volumeBoost || 100)));
  $("modView").checked = !!S.modView;
  renderMods();
  $("adblock").checked = !!S.adblock;
  $("unlimitedTabs").checked = !!S.unlimitedTabs;
  $("aboutVer").textContent = "Chilly " + (S.version || "");
  $("autostart").checked = S.autostart;
  $("autoBonus").checked = S.autoBonus;
  $("autoDrops").checked = S.autoDrops;
  $("emoteBttv").checked = S.emoteBttv;
  $("emote7tv").checked = S.emote7tv;
  $("account").textContent = `${S.user.name} (@${S.user.login})`;
  renderWeb();
  renderStreakList();

  renderList();
  renderTabs();
  if (barOpen === "chatBar") renderChatBar();
  checkMultiEnded();
}

// ---------- Kanavalista ----------

function activeLogin() {
  const t = V.tabs.find((t) => t.id === V.active);
  return t ? t.login : null;
}

function renderList() {
  if (!S || !S.user) return;
  const q = $("search").value.trim().toLowerCase();
  const rows = S.channels.filter((c) =>
    (!q || c.name.toLowerCase().includes(q) || c.login.includes(q)) &&
    (filter !== "live" || c.live) &&
    (filter !== "on" || c.enabled));
  rows.sort((a, b) => (!!b.live - !!a.live) || (b.live && a.live ? b.live.viewer_count - a.live.viewer_count : 0) || a.name.localeCompare(b.name, "fi"));
  const act = activeLogin();
  const openTabs = new Set(V.tabs.map((t) => t.login));

  const rowHtml = (c) => {
    const l = c.live;
    const img = c.image ? `<img src="${esc(c.image)}" alt="" loading="lazy">` : "";
    const tip = l ? `${c.name} – ${l.title}` : c.name;
    const flags = (c.auto ? `<span class="fl">${ic("streak")}</span>` : "") +
                  (c.enabled ? `<span class="bl">${ic("bell", "fill")}</span>` : "");
    const extra = openMenu === c.id ? `
      <div class="ch-extra">
        <select data-sound="${c.id}" title="${t("row.sound")}">${soundOptions(c.sound, true)}</select>
        <button class="ibtn" data-test="${c.id}" title="${t("row.test")}">${ic("play", "sm")}</button>
        ${c.manual ? `<button class="ibtn" data-remove="${c.id}" title="${t("row.remove")}">${ic("trash-2", "sm")}</button>` : ""}
      </div>` : "";
    return `
      <div class="ch${l ? " live" : " off"}${c.login === act ? " active" : ""}${openMenu === c.id ? " open" : ""}">
        <div class="ch-row" data-open="${esc(c.login)}" title="${esc(tip)}">
          <span class="av">${img}</span>
          <div class="info">
            <div class="l1"><div class="name">${esc(c.name)}</div>${c.manual ? `<span class="badge">${t("side.added")}</span>` : ""}<span class="flags">${flags}</span>
              ${l ? `<span class="viewers"><span class="dot"></span>${fmtViewers(l.viewer_count)}</span>` : ""}</div>
            ${l ? `<div class="sub">${esc(l.game_name || t("live"))}</div>` : ""}
          </div>
          <span class="acts">
            <button class="rowbtn auto${c.auto ? " on" : ""}" data-auto="${c.id}" data-on="${c.auto ? 1 : 0}"
              title="${c.auto ? t("row.autoOn") : t("row.autoOff")}">${ic("streak", "sm")}</button>
            <button class="rowbtn${c.enabled ? " on" : ""}" data-bell="${c.id}" data-on="${c.enabled ? 1 : 0}"
              title="${c.enabled ? t("row.bellOn") : t("row.bellOff")}">${c.enabled ? ic("bell", "sm fill") : ic("bell-off", "sm")}</button>
            <button class="rowbtn" data-more="${c.id}" title="${t("row.more")}">${ic("ellipsis", "sm")}</button>
          </span>
        </div>
        ${extra}
      </div>`;
  };
  const liveRows = rows.filter((c) => c.live), offRows = rows.filter((c) => !c.live);
  $("list").innerHTML = q
    ? rows.map(rowHtml).join("")
    : (liveRows.length ? `<div class="sec">${t("live")}</div>` + liveRows.map(rowHtml).join("") : "")
      + (offRows.length ? `<div class="sec fold${offShut ? " shut" : ""}" data-fold="1">${ic("chevron-down")}${t("offline")} <span class="cnt">${offRows.length}</span></div>`
        + (offShut ? "" : offRows.map(rowHtml).join("")) : "");

  // Twitchin ehdotukset haun alle (ei jo listalla olevia)
  if (q.length >= 2 && hitsFor === q) {
    const known = new Set(S.channels.map((c) => c.login));
    const extra = hits.filter((h) => !known.has(h.login));
    if (extra.length) {
      let html = `<div class="sec">${t("side.others")}</div>`;
      html += extra.map((h) => `
        <div class="ch${h.live ? " live" : " off"}">
          <div class="ch-row" data-open="${esc(h.login)}" data-name="${esc(h.name)}" data-image="${esc(h.image)}" title="${esc(h.name)}">
            <span class="av">${h.image ? `<img src="${esc(h.image)}" alt="" loading="lazy">` : ""}</span>
            <div class="info">
              <div class="name">${esc(h.name)}</div>
              <div class="sub">${h.live ? esc(h.game || t("live")) : t("offline")}</div>
            </div>
            ${h.live && h.viewers != null ? `<span class="viewers"><span class="dot"></span>${fmtViewers(h.viewers)}</span>` : ""}
            <button class="rowbtn on" data-add="${esc(h.login)}" title="${t("row.add")}">${ic("plus", "sm")}</button>
          </div>
        </div>`).join("");
      if (rows.length) html = `<div class="sec">${t("side.followed")}</div>` + $("list").innerHTML + html;
      $("list").innerHTML = html;
    }
  }
  markSel();

  const empty = $("empty");
  if ($("list").children.length) empty.classList.add("hidden");
  else {
    empty.classList.remove("hidden");
    empty.textContent = !S.loaded ? t("side.loading")
      : q ? (hitsFor === q ? t("side.noResults") : t("side.searching"))
      : filter === "live" ? t("side.noneLive")
      : filter === "on" ? t("side.noneNotified")
      : t("side.noneFollowed");
  }
}

$("list").addEventListener("click", (e) => {
  if (e.target.closest("[data-fold]")) {
    offShut = !offShut;
    try { localStorage.setItem("offShut", offShut ? "1" : "0"); } catch {}
    renderList();
    return;
  }
  const add = e.target.closest("[data-add]");
  if (add) {
    e.stopPropagation();
    invoke("add_channel", { login: add.dataset.add })
      .then(() => toast(t("row.added"), "check"))
      .catch((err) => toast(String(err)));
    return;
  }
  const b = e.target.closest("[data-bell],[data-auto],[data-more],[data-test],[data-remove]");
  if (b) {
    e.stopPropagation();
    if (b.dataset.bell) invoke("set_channel", { id: b.dataset.bell, enabled: b.dataset.on !== "1" });
    if (b.dataset.auto) invoke("set_channel", { id: b.dataset.auto, auto: b.dataset.on !== "1" });
    if (b.dataset.more) { openMenu = openMenu === b.dataset.more ? null : b.dataset.more; renderList(); }
    if (b.dataset.test) invoke("test_notification", { id: b.dataset.test });
    if (b.dataset.remove) { openMenu = null; invoke("remove_channel", { id: b.dataset.remove }); }
    return;
  }
  const row = e.target.closest("[data-open]");
  if (row) openRow(row);
});
$("list").addEventListener("change", (e) => {
  const t = e.target;
  if (t.dataset.sound) chooseSound(t, (v) => invoke("set_channel", { id: t.dataset.sound, sound: v }));
});

document.querySelectorAll("#filter button").forEach((b) => b.onclick = () => {
  filter = b.dataset.f;
  document.querySelectorAll("#filter button").forEach((x) => x.classList.toggle("on", x === b));
  renderList();
});
function openRow(row) {
  closeOverlays();
  invoke("open_channel", { login: row.dataset.open, name: row.dataset.name || null, image: row.dataset.image || null })
    .catch((err) => toast(String(err)));
}
function markSel() {
  const rows = [...$("list").querySelectorAll(".ch-row[data-open]")];
  rows.forEach((r, i) => r.parentElement.classList.toggle("sel", i === selIdx));
  if (rows[selIdx]) rows[selIdx].scrollIntoView({ block: "nearest" });
}
$("search").addEventListener("input", () => {
  selIdx = -1;
  renderList();
  clearTimeout(searchTimer);
  const q = $("search").value.trim().toLowerCase();
  if (q.length < 2) { hits = []; hitsFor = ""; return; }
  searchTimer = setTimeout(async () => {
    try {
      const res = await invoke("search_channels", { query: q });
      if ($("search").value.trim().toLowerCase() !== q) return; // vanhentunut vastaus
      hits = res; hitsFor = q;
      renderList();
    } catch (e) { /* haku epäonnistui – näytetään vain seuratut */ }
  }, 300);
});
$("search").addEventListener("keydown", (e) => {
  const rows = $("list").querySelectorAll(".ch-row[data-open]");
  if (e.key === "ArrowDown") { e.preventDefault(); selIdx = Math.min(selIdx + 1, rows.length - 1); markSel(); }
  else if (e.key === "ArrowUp") { e.preventDefault(); selIdx = Math.max(selIdx - 1, 0); markSel(); }
  else if (e.key === "Enter") {
    e.preventDefault();
    const r = rows[selIdx >= 0 ? selIdx : 0];
    if (r) openRow(r);
  } else if (e.key === "Escape") {
    $("search").value = ""; hits = []; hitsFor = ""; selIdx = -1; renderList();
  }
});



// Sivupalkin pienennys (muistetaan tällä koneella)
function setCollapsed(on) {
  $("side").classList.toggle("collapsed", on);
  try { localStorage.setItem("sideCollapsed", on ? "1" : "0"); } catch {}
  updateCollapseIcon();
}
function updateCollapseIcon() {
  const collapsed = $("side").classList.contains("collapsed");
  const left = !document.body.classList.contains("chat-left"); // lista vasemmalla
  $("btnCollapse").innerHTML = ic((collapsed === left) ? "chevrons-right" : "chevrons-left", "sm");
  $("btnCollapse").title = collapsed ? t("side.expand") : t("side.collapse");
}
$("btnCollapse").onclick = () => setCollapsed(!$("side").classList.contains("collapsed"));
try { if (localStorage.getItem("sideCollapsed") === "1") $("side").classList.add("collapsed"); } catch {}

// ---------- Välilehdet ----------

function renderTabs() {
  if (tabDrag && tabDrag.on) return; // ei piirretä raahauksen aikana (raahauksen lopussa tulee uusi tila)
  for (const t of V.tabs) if (!t.image && info[t.login]) t.image = info[t.login].image;
  const av = (t) => `<span class="av">${t.image ? `<img src="${esc(t.image)}" alt="">` : ""}</span>`;
  $("tabs").innerHTML = V.tabs.filter((t) => !t.child).map((t) => {
    const p = partnerOf(t);
    const [x, y] = p && t.swapped ? [p, t] : [t, p]; // näytetään samassa järjestyksessä kuin videot
    return `
    <div class="tab${t.id === V.active ? " on" : ""}" data-id="${t.id}" title="${esc(x.name)}${p ? " + " + esc(y.name) : ""}${t.popped ? ` (${window.t("tab.popped")})` : ""}">
      ${p ? `<span class="duo">${av(x)}${av(y)}</span>` : av(t)}
      <span class="nm">${esc(x.name)}${p ? " + " + esc(y.name) : ""}</span>
      ${t.popped ? `<span class="ico">${ic("picture-in-picture-2", "sm")}</span>` : ""}
      ${t.muted ? `<span class="ico">${ic("volume-x", "sm")}</span>` : ""}
      <button class="x" data-close="${t.id}" title="${esc(window.t("close"))}">${ic("x", "sm")}</button>
    </div>`;
  }).join("");
  // Paljon välilehtiä (raja poistettu): aktiivinen välilehti pidetään näkyvissä
  const onTab = $("tabs").querySelector(".tab.on");
  if (onTab && $("tabs").scrollWidth > $("tabs").clientWidth) onTab.scrollIntoView({ block: "nearest", inline: "nearest" });

  const cur = V.tabs.find((t) => t.id === V.active);
  const off = !!(cur && isOffline(cur));
  if (cur && cur.offline !== off) invoke("viewer_offline", { id: cur.id, offline: off });
  $("chanPage").classList.toggle("hidden", !(cur && off && !cur.popped));
  if (cur && off) renderChanPage(cur);
  $("emptyStage").classList.toggle("hidden", !!cur);
  $("poppedStage").classList.toggle("hidden", !(cur && cur.popped));
  const pt = cur && partnerOf(cur);
  renderMulti(cur, pt, off);
  $("btnChatMenu").classList.toggle("hidden", !cur);
  $("btnPop").classList.toggle("hidden", !cur || !!pt);
  $("btnSub").classList.toggle("hidden", !cur || off);
  $("btnMute").classList.toggle("hidden", !cur || off);
  const bst = (S && S.volumeBoost) || 100;
  $("btnBoost").classList.toggle("hidden", !cur || off);
  $("btnBoost").classList.toggle("on", bst > 100);
  $("btnBoost").title = t("bar.boost", { p: bst });
  const bk = `<span class="pct">${bst}%</span>`;
  if ($("btnBoost").dataset.k !== bk) { $("btnBoost").innerHTML = bk; $("btnBoost").dataset.k = bk; }
  $("btnModView").classList.toggle("hidden", !cur || !(S && S.modView) || !canMod(focusTab(cur).login));
  $("btnReload").classList.toggle("hidden", !cur || off || clipMode.has(cur && cur.id));
  $("btnReload").title = t(pt ? "bar.reloadBoth" : "bar.reload");
  setBtn("btnReload", "rotate-cw", t(pt ? "more.reloadBoth" : "more.reload"));
  $("btnClip").classList.toggle("hidden", !cur || off || clipMode.has(cur && cur.id));
  $("btnClip").title = cur ? t("bar.clipTitle", { n: focusTab(cur).name }) : "";
  $("btnBack").classList.toggle("hidden", !(cur && clipMode.has(cur.id)));
  const ft = cur && focusTab(cur);
  if (ft && !info[ft.login]) loadInfo(ft.login);
  const ci = ft && info[ft.login];
  $("btnFollow").classList.toggle("hidden", !cur || off || !ci || ci.following == null);
  if (ci) {
    setBtn("btnFollow", "heart", ci.following ? t("followed") : t("follow"), ci.following ? "fill" : "");
    $("btnFollow").title = ci.following ? t("followed.title") : t("follow.title");
  }
  const quiet = off || clipMode.has(cur && cur.id); // offline-kanava (myös sen klipit) → chat piiloon
  const chatOn = chatShown && !(quiet && !chatOverride.has(cur && cur.id));
  $("btnChatMenu").classList.toggle("off", !chatOn);
  setBtn("btnChat", chatOn ? "message-square" : "message-square", t(chatOn ? "cb.hide" : "cb.show"));
  $("btnChat").classList.toggle("on", !chatOn);
  const noChat = !cur || !chatOn;
  if (document.body.classList.contains("no-chat") !== noChat) {
    document.body.classList.toggle("no-chat", noChat);
    reportStage();
  }
  if (cur) {
    setBtn("btnMute", cur.muted ? "volume-x" : "volume-2");
    $("btnMute").classList.toggle("muted", cur.muted);
    $("btnPop").title = cur.popped ? t("bar.popIn") : t("bar.popOut");
    setBtn("btnPop", "picture-in-picture-2", t(cur.popped ? "more.popIn" : "more.popOut"));
  }
  $("btnMore").classList.toggle("hidden", ["btnReload", "btnPop", "btnModView"].every((id) => $(id).classList.contains("hidden")));
  if (barOpen === "moreBar" && $("btnMore").classList.contains("hidden")) closeBars();
  if (barOpen === "chatBar") renderChatBar();
  updateSeps();
}

// Erottimet näkyvät vain, jos molemmin puolin on näkyviä nappeja
function updateSeps() {
  const vis = (sel) => [...document.querySelectorAll(sel)].some((b) => !b.classList.contains("hidden"));
  const g = [...$("bar").querySelectorAll(".grp")].map((el) => [...el.children].some((b) => !b.classList.contains("hidden")));
  $("sep1").classList.toggle("hidden", !(g[0] && (g[1] || g[2])));
  $("sep2").classList.toggle("hidden", !(g[1] && g[2]));
}

// Välilehtien järjestys raahaamalla. Järjestys pysyy Chillyssä (Rust), kunnes välilehti suljetaan.
let tabDrag = null, tabDragged = false;
$("tabs").addEventListener("pointerdown", (e) => {
  const tab = e.target.closest(".tab");
  if (!tab || e.button !== 0 || e.target.closest("[data-close]")) return;
  tabDrag = { tab, x0: e.clientX, on: false, id: e.pointerId };
});
$("tabs").addEventListener("pointermove", (e) => {
  if (!tabDrag) return;
  const d = tabDrag;
  if (!d.on) {
    if (Math.abs(e.clientX - d.x0) < 6) return;
    d.on = true;
    try { d.tab.setPointerCapture(d.id); } catch {}
    d.tab.classList.add("drag");
    document.body.classList.add("tab-dragging");
  }
  // Siirretään raahattavaa välilehteä sen naapurin ohi, kun hiiri ylittää naapurin puolivälin
  const tabs = [...$("tabs").querySelectorAll(".tab")];
  const i = tabs.indexOf(d.tab);
  const prev = tabs[i - 1], next = tabs[i + 1];
  if (next) { const r = next.getBoundingClientRect(); if (e.clientX > r.left + r.width / 2) next.after(d.tab); }
  if (prev) { const r = prev.getBoundingClientRect(); if (e.clientX < r.left + r.width / 2) prev.before(d.tab); }
});
function endTabDrag() {
  if (!tabDrag) return;
  const d = tabDrag;
  tabDrag = null;
  if (!d.on) return;
  d.tab.classList.remove("drag");
  document.body.classList.remove("tab-dragging");
  tabDragged = true;
  setTimeout(() => (tabDragged = false), 0);
  const ids = [...$("tabs").querySelectorAll(".tab")].map((t) => Number(t.dataset.id));
  invoke("viewer_reorder", { ids });
}
// Hiiren rulla vierittää välilehtipalkkia sivuttain, kun välilehtiä on enemmän kuin mahtuu
$("tabs").addEventListener("wheel", (e) => {
  const el = $("tabs");
  if (el.scrollWidth <= el.clientWidth || Math.abs(e.deltaX) > Math.abs(e.deltaY)) return;
  e.preventDefault();
  el.scrollLeft += e.deltaY;
}, { passive: false });
$("tabs").addEventListener("pointerup", endTabDrag);
$("tabs").addEventListener("pointercancel", endTabDrag);

$("tabs").addEventListener("click", (e) => {
  if (tabDragged) return;
  const x = e.target.closest("[data-close]");
  if (x) { invoke("viewer_close", { id: Number(x.dataset.close) }); return; }
  const t = e.target.closest(".tab");
  if (t) { closeOverlays(); invoke("viewer_activate", { id: Number(t.dataset.id) }); }
});
$("tabs").addEventListener("auxclick", (e) => {
  const t = e.target.closest(".tab");
  if (t && e.button === 1) invoke("viewer_close", { id: Number(t.dataset.id) });
});

$("btnMute").onclick = () => V.active && invoke("viewer_mute", { id: V.active });
$("btnReload").onclick = () => {
  const cur = V.tabs.find((t) => t.id === V.active);
  if (!cur) return;
  const b = $("btnReload");
  b.classList.add("spin");
  setTimeout(() => b.classList.remove("spin"), 700);
  for (const t of [cur, partnerOf(cur)]) if (t) invoke("viewer_reload_tab", { id: t.id });
};

$("btnPop").onclick = () => {
  const cur = V.tabs.find((t) => t.id === V.active);
  if (!cur) return;
  if (cur.popped) invoke("viewer_popin", { id: cur.id });
  else invoke("viewer_popout", { id: cur.id }).catch((e) => toast(String(e)));
};
$("btnPopIn").onclick = () => V.active && invoke("viewer_popin", { id: V.active });
// ---------- Kanavasivu, seuraaminen, tilaus, klippi ----------

function liveFromList(login) {
  const c = S && S.channels.find((c) => c.login === login);
  return c ? !!c.live : null; // null = ei listalla
}
function isOffline(tab) {
  if (clipMode.has(tab.id) || tab.partner) return false;
  const l = liveFromList(tab.login);
  if (l !== null) return !l;
  const ci = info[tab.login];
  return ci ? !ci.live : false; // tuntematon → oletetaan live, kunnes tiedot tulevat
}
async function loadInfo(login) {
  try {
    info[login] = await invoke("channel_info", { login });
    renderTabs();
  } catch (e) { /* näytetään mitä on */ }
}
// Offline-kanava meni liveen → vaihdetaan streamiin
let wasOffline = {};
function checkWentLive() {
  for (const t of V.tabs) {
    const off = isOffline(t);
    if (wasOffline[t.id] && !off && !clipMode.has(t.id)) invoke("viewer_reload_tab", { id: t.id });
    wasOffline[t.id] = off;
  }
  renderTabs();
}
setInterval(async () => {
  // listan ulkopuolisten offline-kanavien tila tarkistetaan erikseen
  for (const t of V.tabs) {
    if (liveFromList(t.login) === null && isOffline(t)) { await loadInfo(t.login); checkWentLive(); }
  }
}, 60000);

function ago(iso) {
  const m = Math.floor((Date.now() - Date.parse(iso)) / 60000);
  if (m < 60) return t("ago.min", { n: m });
  const h = Math.floor(m / 60);
  if (h < 24) return t("ago.h", { n: h });
  const d = Math.floor(h / 24);
  if (d < 30) return t("ago.d", { n: d });
  const mo = Math.floor(d / 30);
  return mo < 12 ? t("ago.mo", { n: mo }) : t("ago.y", { n: Math.floor(mo / 12) });
}
function fmtCount(n) { return n == null ? "" : n.toLocaleString(LANG === "en" ? "en-US" : "fi-FI"); }
function fmtDur(s) { s = Math.round(s); return `${Math.floor(s / 60)}:${String(s % 60).padStart(2, "0")}`; }

let chanRendered = "";
function renderChanPage(tab) {
  const ci = info[tab.login];
  const key = JSON.stringify([tab.id, ci]);
  if (key === chanRendered) return;
  chanRendered = key;
  if (!ci) {
    $("chanPage").innerHTML = `<div class="stage-msg"><span class="spinner"></span></div>`;
    return;
  }
  const banner = ci.banner ? `style="background-image:linear-gradient(to bottom, transparent 40%, var(--bg)), url('${esc(ci.banner)}')"` : "";
  const clips = ci.clips.length
    ? `<div class="clips">${ci.clips.map((c) => `
        <div class="clip" data-clip="${esc(c.url)}" title="${esc(c.title)}">
          <div class="th" style="background-image:url('${esc(c.thumbnail)}')">
            <span class="dur">${fmtDur(c.duration)}</span><span class="vw">${t("views", { n: fmtViewers(c.views) })}</span>
          </div>
          <div class="tt">${esc(c.title)}</div>
        </div>`).join("")}</div>`
    : `<p class="cp-empty">${t("cp.noClips")}</p>`;
  const follow = ci.following == null ? "" : `<button class="btn twitch" data-follow="${ci.following ? 0 : 1}"
      title="${ci.following ? t("followed.title") : t("follow.title")}">${ic("heart", ci.following ? "fill" : "")}${ci.following ? t("followed") : t("follow")}</button>`;
  const newTab = renderChanPage.lastId !== tab.id;
  renderChanPage.lastId = tab.id;
  $("chanPage").innerHTML = `
    <div class="cp-banner" ${banner}></div>
    <div class="cp-head">
      <img class="cp-av" src="${esc(ci.image)}" alt="">
      <div class="cp-title">
        <h1>${esc(ci.name)}</h1>
        <div class="cp-meta">
          <span class="cp-off">${t("cp.offline")}</span>
          ${ci.lastLive ? `<span>${t("cp.lastLive", { t: ago(ci.lastLive) })}</span>` : ""}
          ${ci.followers != null ? `<span>·</span><span>${t("followers", { n: fmtCount(ci.followers) })}</span>` : ""}
        </div>
      </div>
      <div class="cp-actions">
        <button class="btn ghost" data-sub>${ic("star")}${t("bar.sub")}</button>
        ${follow}
      </div>
    </div>
    <div class="cp-body">
      <div><p class="cp-h">${t("cp.clips")}</p>${clips}</div>
      <div class="about">
        <p class="cp-h">${t("cp.about")}</p>
        ${ci.description ? `<p>${esc(ci.description)}</p>` : ""}
        ${ci.followers != null ? `<div class="fact">${ic("users", "sm")}${t("followers", { n: fmtCount(ci.followers) })}</div>` : ""}
        ${ci.lastLive ? `<div class="fact">${ic("clock", "sm")}${t("cp.lastLive", { t: ago(ci.lastLive) })}</div>` : ""}
      </div>
    </div>`;
  if (newTab) $("chanPage").scrollTop = 0;
}

$("chanPage").addEventListener("click", (e) => {
  const cur = V.tabs.find((t) => t.id === V.active);
  if (!cur) return;
  const clip = e.target.closest("[data-clip]");
  if (clip) {
    clipMode.add(cur.id);
    invoke("viewer_play", { id: cur.id, url: clip.dataset.clip }).catch((err) => toast(String(err)));
    renderTabs();
    return;
  }
  const f = e.target.closest("[data-follow]");
  if (f) doFollow(cur, f.dataset.follow === "1");
  if (e.target.closest("[data-sub]")) invoke("open_subscribe", { login: cur.login }).catch((err) => toast(String(err)));
});

async function doFollow(tab, follow) {
  if (webLogged === false) {
    toast(t("toast.loginToFollow"), "user-round");
    webLogin();
    return;
  }
  if (!follow && !confirm(t("confirm.unfollow", { n: tab.name }))) return;
  toast(follow ? t("toast.following") : t("toast.unfollowing"), "heart");
  try {
    const now = await invoke("follow_channel", { id: tab.id, login: tab.login, follow });
    if (info[tab.login]) info[tab.login].following = now;
    chanRendered = "";
    renderTabs();
    toast(now === follow ? (follow ? t("toast.nowFollowing", { n: tab.name }) : t("toast.notFollowing", { n: tab.name }))
      : t("toast.followFailed"), now === follow ? "check" : "info");
  } catch (e) { toast(String(e)); }
}

$("btnFollow").onclick = () => {
  const cur = V.tabs.find((t) => t.id === V.active);
  const ft = cur && focusTab(cur);
  const ci = ft && info[ft.login];
  if (ci) doFollow(ft, !ci.following);
};
$("btnSub").onclick = () => {
  const cur = V.tabs.find((t) => t.id === V.active);
  if (cur) invoke("open_subscribe", { login: focusTab(cur).login }).catch((e) => toast(String(e)));
};
// Klippi: Klippaa avaa yläpalkkiin rivin (otsikko + pituus). Pituuteen lisätään aika, joka kului
// Klippaa-painalluksesta, jotta otsikon kirjoittaminen ei siirrä hetkeä ohi (enintään 60 s).
let clipOpen = null;
function openClipBar() {
  closeBars();
  const c0 = V.tabs.find((t) => t.id === V.active);
  if (!c0) return;
  closeMs();
  const cur = focusTab(c0);
  clipOpen = { login: cur.login, at: Date.now(), len: clipOpen ? clipOpen.len : 30 };
  $("clipWho").textContent = t("clip.who", { n: cur.name });
  $("clipTitle").value = "";
  setClipLen(clipOpen.len);
  $("clipBar").classList.remove("hidden");
  setTimeout(() => $("clipTitle").focus(), 30);
}
function closeClipBar() {
  if (!clipOpen) return;
  clipOpen = null;
  $("clipBar").classList.add("hidden");
}
function setClipLen(sec) {
  if (clipOpen) clipOpen.len = sec;
  document.querySelectorAll("#clipLen button").forEach((b) => b.classList.toggle("on", Number(b.dataset.s) === sec));
  $("clipHint").textContent = t("clip.last", { s: sec });
}
$("clipLen").addEventListener("click", (e) => {
  const b = e.target.closest("[data-s]");
  if (b) setClipLen(Number(b.dataset.s));
});
async function makeClip() {
  if (!clipOpen) return;
  const { login, at, len } = clipOpen;
  const title = $("clipTitle").value.trim();
  const duration = Math.min(60, len + (Date.now() - at) / 1000);
  closeClipBar();
  toast(t("toast.clipping"), "scissors");
  try {
    await invoke("create_clip", { login, title: title || null, duration });
  } catch (e) {
    if (String(e) === "NEED_SCOPE") {
      if (confirm(t("confirm.clipScope"))) {
        invoke("start_login").catch((err) => toast(String(err)));
      }
    } else toast(String(e));
  }
}
$("btnClip").onclick = () => (clipOpen ? closeClipBar() : openClipBar());
$("clipGo").onclick = makeClip;
$("clipCancel").onclick = closeClipBar;
$("clipTitle").addEventListener("keydown", (e) => {
  if (e.key === "Enter") { e.preventDefault(); makeClip(); }
  if (e.key === "Escape") { e.stopPropagation(); closeClipBar(); }
});

// Äänenvahvistin: yläpalkin pikasäätö kiertää 100 → 150 → 200 → 300 %
const BOOSTS = [100, 150, 200, 300];
$("btnBoost").onclick = () => {
  const cur = (S && S.volumeBoost) || 100;
  invoke("set_settings", { volumeBoost: BOOSTS[(BOOSTS.indexOf(cur) + 1) % BOOSTS.length] });
};
document.querySelectorAll("#boostSeg button").forEach((b) => b.onclick = () => invoke("set_settings", { volumeBoost: Number(b.dataset.b) }));
// Mod View vain omalla kanavalla ja kanavilla, joilla käyttäjä on moderaattori (Twitchin lista,
// päivitetään käynnistyessä, asetuksen kytkeytyessä ja 10 min välein).
let MOD = { needScope: false, logins: null };
function canMod(login) {
  if (!S || !S.user || !login) return false;
  return login === S.user.login || !!(MOD.logins && MOD.logins.includes(login));
}
async function loadMods(force) {
  if (!S || !S.user || (force !== true && !S.modView)) return;
  try { MOD = await invoke("mod_channels"); } catch (e) { /* pidetään edellinen lista */ }
  renderMods();
  renderTabs();
}
function renderMods() {
  $("modScopeRow").classList.toggle("hidden", !(S && S.modView && MOD.needScope));
  $("modListRow").classList.toggle("hidden", !(S && S.modView && !MOD.needScope && MOD.logins));
  if (MOD.logins) $("modList").textContent = MOD.logins.length ? MOD.logins.join(", ") : t("mod.none");
}
setInterval(loadMods, 600000);
$("btnModScope").onclick = () => invoke("start_login").catch((e) => toast(String(e)));
$("btnModRefresh").onclick = () => loadMods(true);
$("btnModView").onclick = () => {
  const cur = V.tabs.find((t) => t.id === V.active);
  if (cur) invoke("open_mod_view", { login: focusTab(cur).login }).catch((e) => toast(String(e)));
};
$("modView").onchange = (e) => invoke("set_settings", { modView: e.target.checked }).then(() => e.target.checked && loadMods(true));
$("btnBack").onclick = () => {
  const cur = V.tabs.find((t) => t.id === V.active);
  if (!cur) return;
  clipMode.delete(cur.id);
  invoke("viewer_play", { id: cur.id, url: null });
  chanRendered = "";
  renderTabs();
};

// ---------- Multistream ----------

function setBtn(id, icon, label, cls = "") {
  const html = ic(icon, cls) + (label ? `<span class="lbl">${esc(label)}</span>` : "");
  if ($(id).dataset.k !== html) { $(id).innerHTML = html; $(id).dataset.k = html; }
}
// Kuvakenapit, kun tilaa on vähän (multistream tai kapea ikkuna)
function updateCompact() {
  const cur = V.tabs.find((t) => t.id === V.active);
  // Leveällä ikkunalla tekstit näkyvät myös multistreamissa (säätimet ovat Multistream-napin takana)
  $("bar").classList.toggle("compact", window.innerWidth < (cur && cur.partner ? 1250 : 1100));
}
window.addEventListener("resize", updateCompact);
function partnerOf(t) { return t && t.partner ? V.tabs.find((x) => x.id === t.partner) : null; }
// Stream, johon Seuraa / Tilaa / Klippaa kohdistuvat: se, jonka ääni kuuluu
function focusTab(t) { const p = partnerOf(t); return p && t.audio_partner ? p : t; }

function renderMulti(cur, pt, off) {
  updateCompact();
  $("btnAdd").classList.toggle("hidden", !cur || !!pt || off || cur.popped || clipMode.has(cur.id));
  for (const id of ["btnMs", "audioSeg"]) $(id).classList.toggle("hidden", !pt);
  const tabsEl = $("chatTabs");
  if (!pt) {
    closeMs();
    if (!tabsEl.classList.contains("hidden")) { tabsEl.classList.add("hidden"); reportStage(); }
    return;
  }
  if (msOpen) renderMs(cur, pt);
  if (barOpen === "chatBar") renderChatBar();
  // Chat: molemmat yhtä aikaa tai välilehdet
  const chatBoth = !!cur.chat_both;
  // Leveä chat → rinnakkain (nimet otsikkoriville), kapea → allekkain (ei otsikkoriviä)
  const wide = $("chat").getBoundingClientRect().width >= 560;
  const wantTabs = !chatBoth || wide;
  tabsEl.classList.toggle("both", chatBoth);
  const img = (t) => (t.image ? `<img src="${esc(t.image)}" alt="" style="width:16px;height:16px;border-radius:50%">` : "");
  // Ääni: yhdestä (valitse kummasta) tai molemmista (kumpikin mykistettävissä erikseen)
  const both = !!cur.audio_both;
  const order = cur.swapped ? [[pt, 1], [cur, 0]] : [[cur, 0], [pt, 1]]; // videoiden järjestys
  const html = order.map(([t, i]) => {
    if (both) {
      const on = !t.mute_self;
      return `<button data-self="${t.id}" class="${on ? "on" : ""}" title="${window.t(on ? "bar.muteOne" : "bar.unmuteOne", { n: esc(t.name) })}">${ic(on ? "volume-2" : "volume-x", "sm")}${img(t)}</button>`;
    }
    const on = (i === 1) === !!cur.audio_partner;
    return `<button data-audio="${i}" class="${on ? "on" : ""}" title="${on ? window.t("bar.audioOn", { n: esc(t.name) }) : window.t("bar.audioListen", { n: esc(t.name) })}">${ic(on ? "volume-2" : "volume-x", "sm")}${img(t)}</button>`;
  }).join("") + `<span class="aseg-sep"></span><button data-both="1" class="${both ? "on" : ""}" title="${window.t(both ? "bar.audioOneT" : "bar.audioBothT")}">${ic("audio-lines", "sm")}</button>`;
  if ($("audioSeg").dataset.k !== html) { $("audioSeg").innerHTML = html; $("audioSeg").dataset.k = html; }
  tabsEl.innerHTML = order.map(([t, i]) => {
    const on = chatBoth || (i === 1) === !!cur.chat_partner;
    return `<button ${chatBoth ? "" : `data-chat="${i}"`} class="${on ? "on" : ""}" title="${esc(t.name)}">${img(t)}<span>${esc(t.name)}</span></button>`;
  }).join("");
  if (tabsEl.classList.contains("hidden") === wantTabs) { tabsEl.classList.toggle("hidden", !wantTabs); reportStage(); }
}

// Yläpalkin päälle avautuvat rivit: Chat ja Lisää (kuten Multistream ja Klippi). Vain yksi kerrallaan.
let barOpen = null;
function closeBars() {
  if (!barOpen) return;
  $(barOpen).classList.add("hidden");
  barOpen = null;
}
function openBar(id) {
  closeMs(); closeClipBar(); closeBars();
  barOpen = id;
  $(id).classList.remove("hidden");
  if (id === "chatBar") renderChatBar();
}
function renderChatBar() {
  const cur = V.tabs.find((t) => t.id === V.active), pt = cur && partnerOf(cur);
  document.querySelectorAll("#cbSide button").forEach((b) => b.classList.toggle("on", b.dataset.side === (S.chatSide || "right")));
  $("cbMulti").classList.toggle("hidden", !pt);
  if (pt) document.querySelectorAll("#msChat button").forEach((b) => b.classList.toggle("on", (b.dataset.c === "1") === !!cur.chat_both));
  document.querySelectorAll("#cbEmotes button").forEach((b) => b.classList.toggle("on", b.dataset.e === "bttv" ? !!S.emoteBttv : !!S.emote7tv));
}
$("btnChatMenu").onclick = () => (barOpen === "chatBar" ? closeBars() : openBar("chatBar"));
$("btnMore").onclick = () => (barOpen === "moreBar" ? closeBars() : openBar("moreBar"));
document.querySelectorAll("[data-close-bar]").forEach((b) => b.onclick = closeBars);
$("cbSide").addEventListener("click", (e) => {
  const b = e.target.closest("[data-side]");
  if (b) invoke("set_settings", { chatSide: b.dataset.side });
});
$("cbEmotes").addEventListener("click", (e) => {
  const b = e.target.closest("[data-e]");
  if (!b) return;
  if (b.dataset.e === "bttv") invoke("set_settings", { emoteBttv: !S.emoteBttv });
  else invoke("set_settings", { emoteSeventv: !S.emote7tv });
});
// Lisää-rivin toiminnot sulkevat rivin
for (const id of ["btnReload", "btnPop", "btnModView"]) $(id).addEventListener("click", () => setTimeout(closeBars, 150));

// Multistream-säätimet yläpalkin päällä (asettelu, paikkojen vaihto, chat, kummankin sulkeminen).
// Pudotusvalikko jäisi natiivien striimien alle, joten säätimet avautuvat palkkiin.
let msOpen = false;
function openMs() {
  closeClipBar();
  closeBars();
  const cur = V.tabs.find((t) => t.id === V.active), pt = partnerOf(cur);
  if (!pt) return;
  msOpen = true;
  $("msBar").classList.remove("hidden");
  renderMs(cur, pt);
}
function closeMs() {
  if (!msOpen) return;
  msOpen = false;
  $("msBar").classList.add("hidden");
}
function renderMs(cur, pt) {
  const lay = cur.layout || "side";
  document.querySelectorAll("#msLayout button").forEach((b) => b.classList.toggle("on", b.dataset.l === lay));
  const order = cur.swapped ? [pt, cur] : [cur, pt];
  const html = order.map((x) => `<button class="tbtn danger" data-close-slot="${x.id}">${ic("x", "sm")}<span>${esc(t("ms.close", { n: x.name }))}</span></button>`).join("");
  if ($("msClose").dataset.k !== html) { $("msClose").innerHTML = html; $("msClose").dataset.k = html; }
}
$("btnMs").onclick = () => (msOpen ? closeMs() : openMs());
$("msDone").onclick = closeMs;
$("msLayout").addEventListener("click", (e) => {
  const b = e.target.closest("[data-l]");
  if (b && V.active) invoke("viewer_multi", { id: V.active, layout: b.dataset.l });
});
$("msSwap").onclick = () => V.active && invoke("viewer_swap", { id: V.active }).catch((e) => toast(String(e)));
$("msChat").addEventListener("click", (e) => {
  const b = e.target.closest("[data-c]");
  if (b && V.active) invoke("viewer_multi", { id: V.active, chatBoth: b.dataset.c === "1" });
});
$("msClose").addEventListener("click", (e) => {
  const b = e.target.closest("[data-close-slot]");
  if (!b) return;
  closeMs();
  invoke("viewer_close_slot", { id: Number(b.dataset.closeSlot) });
});

// Multistream: jos toinen striimaaja lopettaa eikä raidaa (raidissa striimi siirtyy uudelle kanavalle
// ja on taas livenä), sen striimi suljetaan hetken päästä ja toinen täyttää tilan.
const MS_END_GRACE = 90000;
const msGone = {};
function slotLive(x) {
  const l = liveFromList(x.login);
  if (l !== null) return l;
  const ci = info[x.login];
  return ci ? !!ci.live : null;
}
function checkMultiEnded() {
  if (!S || !V.tabs.length) return;
  const now = Date.now();
  for (const tab of V.tabs) {
    const p = tab.partner ? partnerOf(tab) : null;
    if (!p) continue;
    for (const x of [tab, p]) {
      if (slotLive(x) === false) {
        const key = x.id + ":" + x.login;
        msGone[key] = msGone[key] || now;
        if (now - msGone[key] >= MS_END_GRACE) {
          delete msGone[key];
          toast(t("ms.ended", { n: x.name }), "radio");
          invoke("viewer_close_slot", { id: x.id });
          return;
        }
      } else {
        delete msGone[x.id + ":" + x.login];
      }
    }
  }
}
setInterval(async () => {
  // Listan ulkopuolisten multistream-kanavien tila haetaan erikseen
  for (const t of V.tabs) {
    const p = t.partner ? partnerOf(t) : null;
    if (!p) continue;
    for (const x of [t, p]) if (liveFromList(x.login) === null) await loadInfo(x.login);
  }
  checkMultiEnded();
}, 30000);
$("audioSeg").addEventListener("click", (e) => {
  const cur = V.tabs.find((t) => t.id === V.active);
  if (!cur) return;
  const b = e.target.closest("button");
  if (!b) return;
  if (b.dataset.both) invoke("viewer_multi", { id: cur.id, audioBoth: !cur.audio_both });
  else if (b.dataset.self) invoke("viewer_self_mute", { id: Number(b.dataset.self) });
  else if (b.dataset.audio) invoke("viewer_multi", { id: cur.id, audioPartner: b.dataset.audio === "1" });
});
$("chatTabs").addEventListener("click", (e) => {
  const b = e.target.closest("[data-chat]");
  if (b && V.active) invoke("viewer_multi", { id: V.active, chatPartner: b.dataset.chat === "1" });
});

// Toisen streamin valitsin
let pickHits = [], pickTimer = null, pickerOpen = false;
function openPicker() {
  closeSettings(); closeDiscover();
  pickerOpen = true;
  $("pickSearch").value = "";
  pickHits = [];
  $("picker").classList.remove("hidden");
  syncCover();
  renderPicker();
  setTimeout(() => $("pickSearch").focus(), 50);
}
function closePicker() {
  if (!pickerOpen) return;
  pickerOpen = false;
  $("picker").classList.add("hidden");
  syncCover();
}
function renderPicker() {
  const cur = V.tabs.find((t) => t.id === V.active);
  const q = $("pickSearch").value.trim().toLowerCase();
  const here = new Set(cur ? [cur.login] : []);
  const live = S.channels.filter((c) => c.live && !here.has(c.login) && (!q || c.name.toLowerCase().includes(q) || c.login.includes(q)))
    .sort((a, b) => b.live.viewer_count - a.live.viewer_count);
  const known = new Set(S.channels.map((c) => c.login));
  const extra = pickHits.filter((h) => !known.has(h.login) && !here.has(h.login));
  const row = (login, name, image, sub, viewers, isLive) => `
    <div class="ch${isLive ? " live" : ""}">
      <div class="ch-row" data-pick="${esc(login)}" data-name="${esc(name)}" data-image="${esc(image)}">
        <span class="av">${image ? `<img src="${esc(image)}" alt="" loading="lazy">` : ""}</span>
        <div class="info"><div class="name">${esc(name)}</div><div class="sub">${esc(sub)}</div></div>
        ${viewers != null ? `<span class="viewers"><span class="dot"></span>${fmtViewers(viewers)}</span>` : ""}
      </div>
    </div>`;
  let html = "";
  if (live.length) html += `<div class="sec">${t("pick.liveFollowed")}</div>` + live.map((c) => row(c.login, c.name, c.image, c.live.game_name || t("live"), c.live.viewer_count, true)).join("");
  if (extra.length) html += `<div class="sec">${t("side.others")}</div>` + extra.map((h) => row(h.login, h.name, h.image, h.live ? h.game || t("live") : t("offline"), h.live ? h.viewers : null, h.live)).join("");
  if (!html) html = `<div class="empty">${q ? t("side.noResults") : t("pick.none")}</div>`;
  $("pickList").innerHTML = html;
}
$("btnAdd").onclick = openPicker;
$("btnClosePicker").onclick = closePicker;
$("pickSearch").addEventListener("input", () => {
  renderPicker();
  clearTimeout(pickTimer);
  const q = $("pickSearch").value.trim().toLowerCase();
  if (q.length < 2) { pickHits = []; return; }
  pickTimer = setTimeout(async () => {
    try {
      const res = await invoke("search_channels", { query: q });
      if ($("pickSearch").value.trim().toLowerCase() === q) { pickHits = res; renderPicker(); }
    } catch {}
  }, 300);
});
$("picker").addEventListener("click", (e) => { if (e.target === $("picker")) closePicker(); });
$("pickSearch").addEventListener("keydown", (e) => {
  if (e.key === "Escape") closePicker();
  if (e.key === "Enter") { const r = $("pickList").querySelector("[data-pick]"); if (r) r.click(); }
});
$("pickList").addEventListener("click", (e) => {
  const r = e.target.closest("[data-pick]");
  if (!r || !V.active) return;
  invoke("viewer_add_partner", { id: V.active, login: r.dataset.pick, name: r.dataset.name || null, image: r.dataset.image || null })
    .then(closePicker)
    .catch((err) => toast(String(err)));
});

// ---------- Löydä (uusien striimaajien etsiminen) ----------

const D = { open: false, game: null, lang: "", sort: "most", live: true, notFollowed: false, query: "", cursor: null, top: null, seq: 0, all: [], shown: 0 };
try { Object.assign(D, JSON.parse(localStorage.getItem("discover") || "{}"), { open: false, cursor: null, top: null, seq: 0, all: [], shown: 0 }); } catch {}
const saveD = () => { try { localStorage.setItem("discover", JSON.stringify({ game: D.game, lang: D.lang, sort: D.sort, live: D.live, notFollowed: D.notFollowed })); } catch {} };
const PAGE = 60;
// Kanavat, joita käyttäjä seuraa (Ei seuratut -suodatin)
const followedLogins = () => new Set(S ? S.channels.filter((c) => !c.manual).map((c) => c.login) : []);

function openDiscover() {
  closeSettings(); closePicker();
  D.open = true;
  $("discover").classList.remove("hidden");
  syncCover();
  $("discLang").value = D.lang;
  $("discLive").checked = D.live;
  $("discNotFollowed").checked = D.notFollowed;
  document.querySelectorAll("#discSort button").forEach((b) => b.classList.toggle("on", b.dataset.sort === D.sort));
  renderDiscFilters();
  if (!D.top) invoke("discover_top_games").then((r) => { D.top = r; renderDiscTop(); }).catch(() => {});
  runDiscover();
}
function closeDiscover() {
  if (!D.open) return;
  D.open = false;
  $("discover").classList.add("hidden");
  syncCover();
}
$("btnDiscover").onclick = () => (D.open ? closeDiscover() : openDiscover());
$("btnCloseDiscover").onclick = closeDiscover;

function renderDiscFilters() {
  $("discGameBox").classList.toggle("hidden", !!D.game);
  $("discGameSel").classList.toggle("hidden", !D.game);
  if (D.game) {
    $("discGameSel").innerHTML = `${D.game.art ? `<img src="${esc(D.game.art)}" alt="">` : ""}<b>${esc(D.game.name)}</b>
      <button class="ibtn" id="discGameClear" title="${t("close")}">${ic("x", "sm")}</button>`;
    $("discGameClear").onclick = () => { D.game = null; saveD(); renderDiscFilters(); runDiscover(); };
  }
  $("discSort").classList.toggle("hidden", !D.live);
  $("discQueryRow").classList.toggle("hidden", D.live);
  renderDiscTop();
}
function renderDiscTop() {
  const show = !D.game && D.live && D.top && D.top.length;
  $("discTop").classList.toggle("hidden", !show);
  if (!show) return;
  $("discTop").innerHTML = D.top.slice(0, 12).map((c, i) =>
    `<button data-top="${i}" title="${esc(c.name)}"><img src="${esc(c.art)}" alt="" loading="lazy"><span>${esc(c.name)}</span></button>`).join("");
}
$("discTop").addEventListener("click", (e) => {
  const b = e.target.closest("[data-top]");
  if (b) pickGame(D.top[Number(b.dataset.top)]);
});
function pickGame(c) {
  D.game = c; saveD();
  $("discGame").value = ""; $("discSug").classList.add("hidden");
  renderDiscFilters(); runDiscover();
}

// Peli / kategoria: ehdotukset kirjoittaessa
let gameTimer = null, sugList = [];
$("discGame").addEventListener("input", () => {
  clearTimeout(gameTimer);
  const q = $("discGame").value.trim();
  if (q.length < 2) { $("discSug").classList.add("hidden"); return; }
  gameTimer = setTimeout(async () => {
    try {
      sugList = await invoke("discover_categories", { query: q });
      if ($("discGame").value.trim() !== q) return;
      $("discSug").innerHTML = sugList.length
        ? sugList.map((c, i) => `<button data-sug="${i}">${c.art ? `<img src="${esc(c.art)}" alt="">` : ""}<span>${esc(c.name)}</span></button>`).join("")
        : `<div class="empty">${t("side.noResults")}</div>`;
      $("discSug").classList.remove("hidden");
    } catch {}
  }, 250);
});
$("discGame").addEventListener("keydown", (e) => {
  if (e.key === "Enter" && sugList[0]) { e.preventDefault(); pickGame(sugList[0]); }
  if (e.key === "Escape") $("discSug").classList.add("hidden");
});
$("discSug").addEventListener("click", (e) => {
  const b = e.target.closest("[data-sug]");
  if (b) pickGame(sugList[Number(b.dataset.sug)]);
});
document.addEventListener("click", (e) => { if (!e.target.closest(".catbox")) $("discSug").classList.add("hidden"); });

$("discLang").onchange = (e) => { D.lang = e.target.value; saveD(); runDiscover(); };
document.querySelectorAll("#discSort button").forEach((b) => b.onclick = () => {
  D.sort = b.dataset.sort; saveD();
  document.querySelectorAll("#discSort button").forEach((x) => x.classList.toggle("on", x === b));
  runDiscover();
});
$("discLive").onchange = (e) => { D.live = e.target.checked; saveD(); renderDiscFilters(); runDiscover(); };
$("discNotFollowed").onchange = (e) => { D.notFollowed = e.target.checked; saveD(); runDiscover(); };
let queryTimer = null;
$("discQuery").addEventListener("input", () => { clearTimeout(queryTimer); queryTimer = setTimeout(runDiscover, 300); });

function discCard(f) {
  const th = f.live && f.thumbnail
    ? `<div class="th" style="background-image:url('${esc(f.thumbnail)}')"><span class="lv">LIVE</span>${f.viewers != null ? `<span class="vw">${t("viewers", { n: fmtViewers(f.viewers) })}</span>` : ""}</div>`
    : `<div class="th noimg">${f.image ? `<img src="${esc(f.image)}" alt="">` : ""}${f.live ? '<span class="lv">LIVE</span>' : ""}</div>`;
  return `<button class="dcard${f.live ? "" : " off"}" data-login="${esc(f.login)}" data-name="${esc(f.name)}" data-image="${esc(f.image)}" title="${esc(f.title)}">
    ${th}
    <div class="meta"><span class="av">${f.image ? `<img src="${esc(f.image)}" alt="" loading="lazy">` : ""}</span>
      <div class="info"><div class="name">${esc(f.name)}</div>${f.title ? `<div class="t">${esc(f.title)}</div>` : ""}<div class="g">${esc(f.game || (f.live ? t("live") : t("offline")))}</div></div></div>
  </button>`;
}
$("discGrid").addEventListener("click", (e) => {
  const c = e.target.closest(".dcard");
  if (!c) return;
  closeDiscover();
  invoke("open_channel", { login: c.dataset.login, name: c.dataset.name || null, image: c.dataset.image || null })
    .catch((err) => toast(String(err)));
});

// Näyttää seuraavan erän koko joukosta (vähiten katsojia -tila) ja hakee erän kanavakuvat.
async function showNextBatch() {
  const batch = D.all.slice(D.shown, D.shown + PAGE);
  D.shown += batch.length;
  $("discGrid").insertAdjacentHTML("beforeend", batch.map(discCard).join(""));
  $("discMoreRow").classList.toggle("hidden", D.shown >= D.all.length);
  const need = batch.filter((f) => !f.image).map((f) => f.id);
  if (!need.length) return;
  const seq = D.seq;
  try {
    const imgs = await invoke("discover_images", { ids: need });
    if (seq !== D.seq) return;
    for (const f of batch) {
      if (!imgs[f.id]) continue;
      f.image = imgs[f.id];
      const card = $("discGrid").querySelector(`.dcard[data-login="${CSS.escape(f.login)}"]`);
      if (!card) continue;
      card.dataset.image = f.image;
      const av = card.querySelector(".meta .av");
      if (av) av.innerHTML = `<img src="${esc(f.image)}" alt="" loading="lazy">`;
    }
  } catch {}
}

async function runDiscover(more = false) {
  const empty = $("discEmpty");
  // Vähiten katsojia: koko joukko on jo haettu, näytetään seuraava erä
  if (more && D.live && D.sort === "fewest") { await showNextBatch(); return; }
  const seq = ++D.seq;
  if (!more) { $("discGrid").innerHTML = ""; D.cursor = null; D.all = []; D.shown = 0; }
  $("discMoreRow").classList.add("hidden");
  empty.classList.remove("hidden");
  empty.textContent = t("side.searching");
  try {
    const skip = D.notFollowed ? followedLogins() : new Set();
    let items = [];
    if (D.live) {
      const fewest = D.sort === "fewest";
      const r = await invoke("discover_streams", { gameId: D.game ? D.game.id : null, language: D.lang || null, fewest, cursor: more ? D.cursor : null });
      if (seq !== D.seq) return;
      items = r.items.filter((f) => !skip.has(f.login));
      D.cursor = r.cursor;
      if (fewest) {
        D.all = items;
        empty.classList.toggle("hidden", items.length > 0);
        empty.textContent = t("side.noResults");
        await showNextBatch();
        return;
      }
    } else {
      const q = $("discQuery").value.trim();
      if (!q) { empty.textContent = t("disc.typeName"); return; }
      items = (await invoke("discover_channels", { query: q, gameId: D.game ? D.game.id : null, language: D.lang || null }))
        .filter((f) => !skip.has(f.login));
      if (seq !== D.seq) return;
    }
    $("discGrid").insertAdjacentHTML("beforeend", items.map(discCard).join(""));
    empty.classList.toggle("hidden", $("discGrid").children.length > 0);
    empty.textContent = t("side.noResults");
    $("discMoreRow").classList.toggle("hidden", !(D.live && D.cursor));
  } catch (e) {
    if (seq === D.seq) empty.textContent = String(e);
  }
}
$("discMore").onclick = () => runDiscover(true);

// ---------- twitch.tv-istunto (chat, pisteet) ----------
async function checkWeb() {
  try { webLogged = await invoke("twitch_web_status"); } catch { webLogged = null; }
  renderWeb();
}
function renderWeb() {
  let dismissed = false;
  try { dismissed = localStorage.getItem("webPromptDismissed") === "1"; } catch {}
  $("webPrompt").classList.toggle("hidden", !(S && S.user && webLogged === false && !dismissed));
  $("webStatus").textContent = webLogged ? t("acc.webOn")
    : webLogged === false ? t("acc.webOff") : "";
  $("btnWebLogin2").classList.toggle("hidden", webLogged !== false);
}
const webLogin = () => invoke("viewer_twitch_login").catch((e) => toast(String(e)));
$("btnWebLogin").onclick = webLogin;
$("btnWebLogin2").onclick = webLogin;
$("btnWebLater").onclick = () => {
  try { localStorage.setItem("webPromptDismissed", "1"); } catch {}
  renderWeb();
};

// Kerrotaan Rustille, mihin kohtaan stream piirretään
// Ilmoitetaan vain kun paikka tai koko oikeasti muuttui. ResizeObserver ei huomaa pelkkää
// siirtymää (esim. kanavalista vaihtaa puolta), joten tarkistetaan myös asettelun muutosten jälkeen.
let lastStage = "";
function rectOf(el) {
  const r = el.getBoundingClientRect();
  return [r.left, r.top, r.width, r.height];
}
function reportStage() {
  const video = rectOf($("video"));
  if (video[2] <= 0 || video[3] <= 0) return;
  const chatVisible = !document.body.classList.contains("no-chat");
  const chat = chatVisible ? rectOf($("chatView")) : null;
  const key = JSON.stringify([video, chat].map((r) => r && r.map(Math.round)));
  if (key === lastStage) return;
  lastStage = key;
  invoke("viewer_layout", { video, chat });
}
new ResizeObserver(reportStage).observe($("stage"));
new ResizeObserver(reportStage).observe($("video"));
new ResizeObserver(reportStage).observe($("chatView"));

// ---------- Chatin koko (raahattava raja) ----------
const CHAT_DEF = { w: 340, h: 300 };
function chatSize() {
  try { return JSON.parse(localStorage.getItem("chatSize")) || { ...CHAT_DEF }; } catch { return { ...CHAT_DEF }; }
}
// Chatin koko rajataan aina niin, että videolle jää tilaa (esim. pienemmällä näytöllä)
function applyChatSize() {
  const s = chatSize();
  const below = document.body.classList.contains("chat-below");
  const st = $("stage").getBoundingClientRect();
  const h = st.height > 0 ? Math.min(s.h, Math.max(120, st.height * 0.6)) : s.h;
  const w = st.width > 0 ? Math.min(s.w, Math.max(220, st.width * 0.6)) : s.w;
  $("chat").style.width = below ? "" : w + "px";
  $("chat").style.height = below ? h + "px" : "";
}
window.addEventListener("resize", () => { applyChatSize(); reportStage(); });
$("splitter").addEventListener("pointerdown", (e) => {
  const sp = $("splitter");
  sp.setPointerCapture(e.pointerId);
  sp.classList.add("drag");
  const below = document.body.classList.contains("chat-below");
  const left = document.body.classList.contains("chat-left");
  const st = $("stage").getBoundingClientRect();
  const s = chatSize();
  const move = (ev) => {
    if (below) s.h = Math.round(Math.min(Math.max(st.bottom - ev.clientY, 120), st.height - 160));
    else if (left) s.w = Math.round(Math.min(Math.max(ev.clientX - st.left, 220), st.width - 260));
    else s.w = Math.round(Math.min(Math.max(st.right - ev.clientX, 220), st.width - 260));
    try { localStorage.setItem("chatSize", JSON.stringify(s)); } catch {}
    applyChatSize();
    reportStage();
  };
  const up = () => {
    sp.classList.remove("drag");
    sp.removeEventListener("pointermove", move);
    sp.removeEventListener("pointerup", up);
  };
  sp.addEventListener("pointermove", move);
  sp.addEventListener("pointerup", up);
});
$("btnChat").onclick = () => {
  const cur = V.tabs.find((t) => t.id === V.active);
  if (cur && (isOffline(cur) || clipMode.has(cur.id))) {
    if (chatOverride.has(cur.id)) chatOverride.delete(cur.id); else chatOverride.add(cur.id);
    if (!chatShown) chatShown = true;
    renderTabs();
    return;
  }
  chatShown = !chatShown;
  try { localStorage.setItem("chatShown", chatShown ? "1" : "0"); } catch {}
  renderTabs();
};
new ResizeObserver(reportStage).observe($("side"));
$("side").addEventListener("transitionend", reportStage);

// ---------- Asetukset ----------

function openSettings() {
  closeDiscover(); closePicker();
  settingsOpen = true;
  $("settings").classList.remove("hidden");
  syncCover();
}
function closeSettings() {
  if (!settingsOpen) return;
  settingsOpen = false;
  $("settings").classList.add("hidden");
  syncCover();
}
$("btnSettings").onclick = () => (settingsOpen ? closeSettings() : openSettings());
document.querySelectorAll(".snav button").forEach((b) => b.onclick = () => {
  document.querySelectorAll(".snav button").forEach((x) => x.classList.toggle("on", x === b));
  document.querySelectorAll(".sbody section").forEach((sec) => sec.classList.toggle("hidden", sec.dataset.sec !== b.dataset.sec));
});
document.addEventListener("keydown", (e) => {
  if (e.key !== "Escape") return;
  if (pickerOpen) closePicker(); else if (settingsOpen) closeSettings(); else if (D.open) closeDiscover();
  else if (clipOpen) closeClipBar(); else if (msOpen) closeMs(); else if (barOpen) closeBars();
});
$("btnCloseSettings").onclick = closeSettings;

$("globalSound").onchange = (e) => chooseSound(e.target, (v) => invoke("set_settings", { sound: v }));
$("btnTest").onclick = () => invoke("test_notification", {});
$("interval").onchange = (e) => invoke("set_settings", { interval: Number(e.target.value) });
$("popupSecs").onchange = (e) => invoke("set_settings", { popupSecs: Number(e.target.value) });
document.querySelectorAll("#chatSide button").forEach((b) => b.onclick = () => invoke("set_settings", { chatSide: b.dataset.side }));
document.querySelectorAll("#langSeg button").forEach((b) => b.onclick = () => invoke("set_settings", { lang: b.dataset.lang }));
document.querySelectorAll("#multiAudio button").forEach((b) => b.onclick = () => invoke("set_settings", { multiAudioBoth: b.dataset.v === "1" }));
document.querySelectorAll("#multiChat button").forEach((b) => b.onclick = () => invoke("set_settings", { multiChatBoth: b.dataset.v === "1" }));
// Muutosteksti voi olla kaksikielinen: rivit "FI: ..." ja "EN: ..." → näytetään oman kielen rivit
function pickNotes(notes, lang) {
  const lines = String(notes || "").split(/\r?\n/);
  if (!lines.some((l) => /^(FI|EN):/i.test(l))) return notes || "";
  const want = (lang || "fi").toUpperCase() + ":";
  return lines.filter((l) => l.toUpperCase().startsWith(want)).map((l) => l.slice(3).trim()).join("\n");
}
$("btnWebsite").onclick = () => invoke("open_website");
$("btnLogDir").onclick = () => invoke("open_log_dir").catch((e) => toast(String(e)));

// Katseluputki: raahattava tärkeysjärjestys Asetuksissa
let skDrag = null;
function renderStreakList() {
  if (!S || (skDrag && skDrag.on)) return;
  const el = $("streakList");
  if (!el) return;
  const byId = new Map(S.channels.map((c) => [c.id, c]));
  const ids = (S.streakOrder && S.streakOrder.length ? S.streakOrder : S.channels.filter((c) => c.auto).map((c) => c.id))
    .filter((id) => byId.has(id) && byId.get(id).auto);
  if (!ids.length) {
    el.innerHTML = `<div class="sk-empty">${esc(t("streak.empty"))}</div>`;
    return;
  }
  el.innerHTML = ids.map((id, idx) => {
    const c = byId.get(id);
    const l = c.live;
    const img = c.image ? `<img src="${esc(c.image)}" alt="" loading="lazy">` : "";
    return `
      <div class="sk-row${l ? " live" : ""}" data-sk="${esc(c.id)}">
        <span class="sk-grip" title="${esc(t("streak.drag"))}">${ic("grip-vertical", "sm")}</span>
        <span class="sk-rank${idx < 2 ? " top" : ""}">#${idx + 1}</span>
        <span class="av">${img}</span>
        <div class="sk-info">
          <div class="sk-name">${esc(c.name)}</div>
          <div class="sk-sub">${l ? `<span class="dot"></span>${esc(l.game_name || t("live"))}` : esc(t("offline"))}</div>
        </div>
        <button class="ibtn" data-sk-off="${esc(c.id)}" title="${esc(t("streak.off"))}">${ic("x", "sm")}</button>
      </div>`;
  }).join("");
}
$("streakList").addEventListener("click", (e) => {
  const b = e.target.closest("[data-sk-off]");
  if (!b) return;
  e.stopPropagation();
  invoke("set_channel", { id: b.dataset.skOff, auto: false });
});
$("streakList").addEventListener("pointerdown", (e) => {
  const row = e.target.closest(".sk-row");
  if (!row || e.button !== 0 || e.target.closest("[data-sk-off]")) return;
  skDrag = { row, y0: e.clientY, on: false, id: e.pointerId };
});
$("streakList").addEventListener("pointermove", (e) => {
  if (!skDrag) return;
  const d = skDrag;
  if (!d.on) {
    if (Math.abs(e.clientY - d.y0) < 5) return;
    d.on = true;
    try { d.row.setPointerCapture(d.id); } catch {}
    d.row.classList.add("drag");
    document.body.classList.add("sk-dragging");
  }
  const rows = [...$("streakList").querySelectorAll(".sk-row")];
  const i = rows.indexOf(d.row);
  const prev = rows[i - 1], next = rows[i + 1];
  if (next) { const r = next.getBoundingClientRect(); if (e.clientY > r.top + r.height / 2) next.after(d.row); }
  if (prev) { const r = prev.getBoundingClientRect(); if (e.clientY < r.top + r.height / 2) prev.before(d.row); }
  // Päivitetään #1/#2-numerot heti raahauksen aikana
  [...$("streakList").querySelectorAll(".sk-row")].forEach((el, idx) => {
    const rk = el.querySelector(".sk-rank");
    if (rk) { rk.textContent = "#" + (idx + 1); rk.classList.toggle("top", idx < 2); }
  });
});
function endSkDrag() {
  if (!skDrag) return;
  const d = skDrag;
  skDrag = null;
  if (!d.on) return;
  d.row.classList.remove("drag");
  document.body.classList.remove("sk-dragging");
  const ids = [...$("streakList").querySelectorAll(".sk-row")].map((r) => r.dataset.sk);
  invoke("set_streak_order", { ids });
}
$("streakList").addEventListener("pointerup", endSkDrag);
$("streakList").addEventListener("pointercancel", endSkDrag);

// Päivitykset (Asetukset → Tietoja)
$("btnCheckUpd").onclick = async () => {
  const b = $("btnCheckUpd");
  b.disabled = true;
  $("updStatus").textContent = t("upd.checking");
  try {
    const u = await invoke("check_update");
    $("updStatus").textContent = u.available ? t("upd.available", { v: u.version }) : t("upd.latest", { v: u.current });
    $("btnInstallUpd").classList.toggle("hidden", !u.available);
    $("updNotesRow").classList.toggle("hidden", !(u.available && u.notes));
    $("updNotes").textContent = pickNotes(u.notes, S.lang);
  } catch (e) {
    $("updStatus").textContent = t("upd.error");
  }
  b.disabled = false;
};
$("btnInstallUpd").onclick = async () => {
  const b = $("btnInstallUpd");
  b.disabled = true;
  $("updStatus").textContent = t("upd.installing");
  try { await invoke("install_update"); }
  catch (e) { b.disabled = false; $("updStatus").textContent = t("upd.error") + (e ? ` (${String(e).slice(0, 120)})` : ""); }
};

$("btnChatSettings").onclick = () => { closeBars(); invoke("open_chat_settings").catch((e) => toast(String(e))); };
$("btnChatSettings2").onclick = () => invoke("open_chat_settings").catch((e) => toast(String(e)));
$("unlimitedTabs").onchange = async (e) => {
  const on = e.target.checked;
  if (on && !confirm(t("confirm.unlimited"))) { e.target.checked = false; return; }
  await invoke("set_settings", { unlimitedTabs: on });
};
$("adblock").onchange = async (e) => {
  await invoke("set_settings", { adblock: e.target.checked });
  // Koskee uusia striimejä heti; auki olevat vaihtuvat uudelleenkäynnistyksessä
  if (V && V.tabs && V.tabs.length && confirm(t("confirm.adblockRestart"))) invoke("restart_app");
};
$("hwAccel").onchange = async (e) => {
  await invoke("set_settings", { hwAccel: e.target.checked });
  // Selainmoottorin asetus luetaan käynnistyksessä → kysytään uudelleenkäynnistystä
  if (e.target.checked !== S.hwAccelActive && confirm(t("confirm.restart"))) invoke("restart_app");
};
$("autoBonus").onchange = (e) => invoke("set_settings", { autoBonus: e.target.checked });
$("autoDrops").onchange = (e) => invoke("set_settings", { autoDrops: e.target.checked });
$("emoteBttv").onchange = (e) => invoke("set_settings", { emoteBttv: e.target.checked });
$("emote7tv").onchange = (e) => invoke("set_settings", { emoteSeventv: e.target.checked });
$("autostart").onchange = async (e) => {
  try { await invoke("set_autostart", { on: e.target.checked }); }
  catch (err) { toast(String(err)); }
};
$("btnLogout").onclick = () => { if (confirm(t("confirm.logout"))) invoke("logout"); };

// ---------- Kirjautuminen ----------

$("btnLogin").onclick = async () => {
  try { await invoke("start_login"); } catch (e) { S.status = String(e); render(); }
};
$("btnCancelLogin").onclick = () => invoke("cancel_login");
$("openVerify").onclick = (e) => { e.preventDefault(); invoke("open_login_page"); };

// ---------- Käynnistys ----------

listen("state", refresh);
listen("viewer-state", refreshViewer);
listen("viewer-error", (e) => toast(e.payload, "info"));
listen("web-login-changed", checkWeb);
window.__claimLog = [];
listen("auto-claimed", (e) => {
  const p = e.payload || {};
  window.__claimLog.push({ ...p, at: new Date().toISOString() });
  if (p.kind === "bonus") {
    const c = S && S.channels.find((c) => c.login === p.login);
    toast(t("toast.bonus", { n: c ? c.name : p.login }), "gem");
  } else if (p.kind === "drop") {
    toast(t("toast.drop", { n: p.name }), "gift");
  }
});
setInterval(() => renderList(), 60000);
// ---------- Aloitusanimaatio (vain kun Chilly avataan näkyvänä) ----------
// v2: logo ponnahtaa esiin, sitten logo ja nimi lentävät kaarena sivupalkin logon ja nimen paikalle,
// ja sovellus paljastuu logon ympäriltä aukeavasta reiästä. Klikkaus ohittaa.
async function splash() {
  let show = false;
  try { show = await invoke("take_splash"); } catch {}
  if (!show) return;
  const sp = $("splash"), name = $("spName"), logo = $("spLogo"), mark = $("spMark"), bg = $("spBg");
  [..."Chilly"].forEach((ch) => { const el = document.createElement("span"); el.textContent = ch; name.appendChild(el); });
  sp.classList.remove("hidden");
  // Natiivit striimit (esim. katseluputken taustastriimi) piirtyisivät animaation päälle
  splashActive = true;
  syncCover();
  const reduce = matchMedia("(prefers-reduced-motion: reduce)").matches;
  let anims = [], finished = false;
  const A = (el, kf, o) => { const a = el.animate(kf, Object.assign({ fill: "both" }, o)); anims.push(a); return a; };
  const finish = () => {
    if (finished) return;
    finished = true;
    sp.style.pointerEvents = "none";
    sp.animate([{ opacity: 1 }, { opacity: 0 }], { duration: 180, fill: "forwards" }).onfinish = () => {
      anims.forEach((a) => a.cancel());
      sp.remove();
      splashActive = false;
      syncCover();
    };
  };
  sp.onclick = finish;
  // Varmistus: striimit eivät saa jäädä piiloon, vaikka animaatio keskeytyisi virheeseen
  setTimeout(() => { if (!finished) finish(); }, 6000);
  if (reduce) { setTimeout(finish, 600); return; }
  await document.fonts.ready;
  const ease = { pop: "cubic-bezier(.2,1.4,.4,1)", inout: "cubic-bezier(.65,0,.25,1)", out: "cubic-bezier(.2,.8,.2,1)" };
  const img = logo.querySelector("img");
  const t0 = performance.now();
  // Alku: logo ponnahtaa, hehkuu, kiiltoviiru, kirjaimet nousevat, jäähiutaleet sinkoutuvat
  A(img, [{ opacity: 0, transform: "scale(.55) rotate(-8deg)" }, { opacity: 1, transform: "scale(1.06) rotate(2deg)", offset: .6 }, { opacity: 1, transform: "none" }], { duration: 700, delay: 150, easing: ease.pop });
  A(img, [{ filter: "drop-shadow(0 0 0 rgba(34,211,238,0))" }, { filter: "drop-shadow(0 0 26px rgba(34,211,238,.75))", offset: .45 }, { filter: "drop-shadow(0 0 10px rgba(34,211,238,.35))" }], { duration: 1000, delay: 550 });
  A($("spShine"), [{ opacity: 1, backgroundPosition: "-150% 0" }, { opacity: 1, backgroundPosition: "150% 0" }], { duration: 650, delay: 800, easing: "ease-in-out" });
  [...name.children].forEach((el, i) => A(el, [{ opacity: 0, transform: "translateY(14px)" }, { opacity: 1, transform: "none" }], { duration: 420, delay: 750 + i * 60, easing: ease.pop }));
  const lr = logo.getBoundingClientRect();
  for (let i = 0; i < 22; i++) {
    const f = document.createElement("div"); f.className = "flake";
    const a = (i / 22) * Math.PI * 2 + Math.random() * .4, r = 120 + Math.random() * 170, z = 3 + Math.random() * 5;
    Object.assign(f.style, { width: z + "px", height: z + "px", left: lr.left + lr.width / 2 + "px", top: lr.top + lr.height / 2 + "px" });
    sp.appendChild(f);
    A(f, [{ opacity: 0, transform: "rotate(45deg) scale(.4)" }, { opacity: .9, offset: .25 }, { opacity: 0, transform: `translate(${Math.cos(a) * r}px, ${Math.sin(a) * r}px) rotate(225deg) scale(1)` }], { duration: 1500, delay: 250 + Math.random() * 250, easing: "ease-out" });
  }
  // Odotetaan, että sovellus on piirretty (kirjautumistieto haetaan samaan aikaan)
  await Promise.race([window.__boot, new Promise((r) => setTimeout(r, 1200))]);
  await new Promise((r) => setTimeout(r, 30));
  if (finished) return;
  // Laskeutumiskohde: sivupalkin logo ja nimi. Jos ne eivät näy (sivupalkki pienennetty, ei kirjautunut),
  // käytetään vanhaa loppua: koko kuva häipyy.
  const tLogo = document.querySelector("#app:not(.hidden) .side-head .logo");
  const tName = document.querySelector("#app:not(.hidden) .side-head .brand");
  const to = tLogo && tLogo.getBoundingClientRect(), nTo = tName && tName.getBoundingClientRect();
  if (!to || !to.width || !nTo || !nTo.width) {
    A(sp, [{ opacity: 1, transform: "none" }, { opacity: 0, transform: "scale(1.04)" }], { duration: 500, delay: Math.max(0, 1900 - (performance.now() - t0)), easing: "ease-in" })
      .onfinish = () => { finished = true; sp.remove(); splashActive = false; syncCover(); };
    return;
  }
  const T = Math.max(0, 1350 - (performance.now() - t0)), D = 850; // laskeutumisen alku (tästä hetkestä) ja kesto
  // Kääre x-liikkeelle (kaari: x ja y eri käyrillä). Mitataan vasta kääreen lisäämisen jälkeen.
  const wrap = document.createElement("div");
  logo.parentNode.insertBefore(wrap, logo); wrap.appendChild(logo);
  const lr2 = logo.getBoundingClientRect(), nFrom = name.getBoundingClientRect();
  const fs = (el) => parseFloat(getComputedStyle(el).fontSize) || 1;
  const sl = to.width / lr2.width, sn = fs(tName) / fs(name);
  const fx = lr2.left + lr2.width / 2, fy = lr2.top + lr2.height / 2, tx = to.left + to.width / 2, ty = to.top + to.height / 2;
  bg.style.setProperty("--cx", fx + "px");
  bg.style.setProperty("--cy", fy + "px");
  // Tausta aukeaa logon kohdalta koko ikkunan yli
  const hole = Math.hypot(innerWidth, innerHeight) + 200;
  A(bg, [{ "--hole": "0px" }, { "--hole": "120px", offset: .3 }, { "--hole": hole + "px" }], { duration: D + 150, delay: T, easing: ease.inout });
  A(wrap, [{ transform: "translateX(0)" }, { transform: `translateX(${tx - fx}px)` }], { duration: D, delay: T, easing: "cubic-bezier(.55,0,.3,1)" });
  A(logo, [{ transform: "translateY(0) scale(1)" }, { transform: `translateY(${ty - fy}px) scale(${sl})` }], { duration: D, delay: T, easing: "cubic-bezier(.7,0,.2,1)" });
  // Nimi kutistuu sivupalkin nimen paikalle
  // Nimen keskipiste sivupalkin nimen keskipisteeseen (skaalaus keskeltä)
  const ndx = (nTo.left + nTo.width / 2) - (nFrom.left + nFrom.width / 2), ndy = (nTo.top + nTo.height / 2) - (nFrom.top + nFrom.height / 2);
  A(name, [{ transform: "none" }, { transform: `translate(${ndx}px, ${ndy}px) scale(${sn})` }], { duration: D, delay: T + 60, easing: ease.inout });
  [...name.children].forEach((el) => A(el, [{ filter: "drop-shadow(0 0 1px #0a3a44) drop-shadow(0 0 10px rgba(34,211,238,.35))" }, { filter: "drop-shadow(0 0 0 rgba(0,0,0,0))" }], { duration: D, delay: T, easing: "ease-in" }));
  // Sovellus tarkentuu ja asettuu; sivupalkin oma logo ja nimi piilossa, kunnes animaatio laskeutuu
  A($("app"), [{ transform: "scale(1.035)", filter: "blur(6px) brightness(.55)" }, { transform: "none", filter: "none" }], { duration: D + 250, delay: T, easing: ease.out });
  for (const el of [tLogo, tName]) A(el, [{ opacity: 0 }, { opacity: 0, offset: .99 }, { opacity: 1 }], { duration: T + D + 60 });
  // Laskeutuminen: pieni jäärengas ja kipinät
  const land = T + D - 40;
  const ring = document.createElement("div"); ring.className = "ring";
  Object.assign(ring.style, { left: tx - 14 + "px", top: ty - 14 + "px", width: "28px", height: "28px" });
  sp.appendChild(ring);
  A(ring, [{ opacity: 0, transform: "scale(.6)" }, { opacity: .9, offset: .2 }, { opacity: 0, transform: "scale(2.6)" }], { duration: 650, delay: land, easing: "ease-out" });
  for (let i = 0; i < 7; i++) {
    const f = document.createElement("div"); f.className = "flake";
    const a = -Math.PI / 2 + (i - 3) * 0.55, r = 16 + Math.random() * 14, z = 2 + Math.random() * 2;
    Object.assign(f.style, { width: z + "px", height: z + "px", left: tx + "px", top: ty + "px" });
    sp.appendChild(f);
    A(f, [{ opacity: 0, transform: "rotate(45deg)" }, { opacity: 1, offset: .2 }, { opacity: 0, transform: `translate(${Math.cos(a) * r}px, ${Math.sin(a) * r}px) rotate(180deg)` }], { duration: 600, delay: land, easing: "ease-out" });
  }
  A(mark, [{ opacity: 1 }, { opacity: 0 }], { duration: 160, delay: T + D + 40 });
  setTimeout(() => {
    if (finished) return;
    finished = true;
    anims.forEach((a) => a.cancel());
    sp.remove();
    splashActive = false;
    syncCover();
  }, T + D + 700);
}
splash();

window.__boot = refresh().then(refreshViewer).then(reportStage);
window.__boot.then(checkWeb);
