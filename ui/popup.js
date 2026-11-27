const { invoke } = window.__TAURI__.core;
const { listen } = window.__TAURI__.event;
const stack = document.getElementById("stack");
const MAX = 4;

function esc(s) {
  return String(s ?? "").replace(/[&<>"']/g, (c) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;" }[c]));
}

// Huom: ei requestAnimationFrame – se ei aina laukea tyhjässä läpinäkyvässä ikkunassa.
function layout() {
  const h = stack.children.length ? Math.ceil(stack.getBoundingClientRect().height) : 0;
  invoke("popup_layout", { height: h });
}

function remove(card) {
  if (card.classList.contains("out")) return;
  card.classList.add("out");
  setTimeout(() => { card.remove(); layout(); }, 250);
}

/** Yhteinen kortti: vasen osa (kuva tai ikoni), teksti, sulkunappi ja aikapalkki. */
function makeCard(n, cls, left, body) {
  const card = document.createElement("div");
  card.className = "card " + cls;
  card.innerHTML = `${left}<div class="body">${body}</div>
    <button class="x" title="${t("close")}">${ic("x", "sm")}</button>
    ${n.secs > 0 ? '<div class="bar"></div>' : ""}`;
  card.querySelector(".x").onclick = (e) => { e.stopPropagation(); remove(card); };
  return card;
}

/** Lisää kortin pinoon; hiiri kortin päällä pysäyttää automaattisen sulkeutumisen. */
function show(card, n) {
  if (n.secs > 0) {
    const anim = card.querySelector(".bar").animate(
      [{ transform: "scaleX(1)" }, { transform: "scaleX(0)" }],
      { duration: n.secs * 1000, fill: "forwards" });
    anim.onfinish = () => remove(card);
    card.onmouseenter = () => anim.pause();
    card.onmouseleave = () => anim.play();
  }
  stack.appendChild(card);
  while (stack.children.length > MAX) stack.firstElementChild.remove();
  const im = card.querySelector("img");
  if (im) im.onload = layout;
  layout();
}

function addLive(n) {
  const av = n.image
    ? `<div class="av"><img src="${esc(n.image)}" alt=""></div>`
    : `<div class="av ph">${esc((n.name || "?")[0].toUpperCase())}</div>`;
  const meta = [n.game, n.viewers != null ? t("viewers", { n: n.viewers.toLocaleString(LANG === "en" ? "en-US" : "fi-FI") }) : ""].filter(Boolean).join(" · ");
  const card = makeCard(n, "live", av, `
    <div class="kicker"><span class="dot"></span>${t("pop.live")}</div>
    <div class="name">${esc(n.name)}</div>
    ${n.title ? `<div class="title">${esc(n.title)}</div>` : ""}
    ${meta ? `<div class="meta">${esc(meta)}</div>` : ""}`);
  card.onclick = () => { if (n.login) invoke("open_channel", { login: n.login }).catch(() => {}); remove(card); };
  show(card, n);
}

function addClip(n) {
  const card = makeCard(n, "tool", `<div class="tile ice">${ic("scissors")}</div>`, `
    <div class="kicker">${esc(n.name)}</div>
    <div class="name">${t("pop.clip")}</div>
    <div class="acts">
      <button class="act primary" data-act="copy">${ic("copy", "sm")}<span>${t("pop.copy")}</span></button>
      <button class="act" data-act="open">${ic("external-link", "sm")}<span>${t("pop.open")}</span></button>
    </div>`);
  const copy = card.querySelector('[data-act="copy"]');
  copy.onclick = async (e) => {
    e.stopPropagation();
    try {
      await invoke("copy_text", { text: n.clip_url });
      copy.innerHTML = `${ic("check", "sm")}<span>${t("pop.copied")}</span>`;
    } catch { copy.querySelector("span").textContent = t("pop.copyFail"); }
  };
  // Klippi avataan katsottavaksi selaimeen (Twitchin klippieditori poistettu: otsikko ja pituus valitaan jo Chillyssä)
  card.querySelector('[data-act="open"]').onclick = (e) => {
    e.stopPropagation();
    invoke("open_clip", { url: n.clip_url }).catch(() => {});
    remove(card);
  };
  show(card, n);
}

function addDrop(n) {
  const card = makeCard(n, "tool", `<div class="tile magenta">${ic("gift")}</div>`, `
    <div class="kicker">${t("pop.auto")}</div>
    <div class="name">${t("pop.drop")}</div>
    <div class="meta">${esc(n.name)}</div>`);
  show(card, n);
}

// Muutosteksti voi olla kaksikielinen: rivit "FI: ..." ja "EN: ..." → näytetään oman kielen rivit
function pickNotes(notes, lang) {
  const lines = String(notes || "").split(/\r?\n/);
  if (!lines.some((l) => /^(FI|EN):/i.test(l))) return notes || "";
  const want = (lang || "fi").toUpperCase() + ":";
  return lines.filter((l) => l.toUpperCase().startsWith(want)).map((l) => l.slice(3).trim()).join("\n");
}
function addUpdate(n) {
  const card = makeCard(n, "tool", `<div class="tile ice">${ic("download")}</div>`, `
    <div class="kicker">${t("pop.update")}</div>
    <div class="name">Chilly ${esc(n.name)}</div>
    ${n.title ? `<div class="meta">${esc(pickNotes(n.title, window.LANG || document.documentElement.lang).split("\n")[0])}</div>` : ""}
    <div class="acts">
      <button class="act primary" data-act="install">${ic("download", "sm")}<span>${t("pop.updateNow")}</span></button>
      <button class="act" data-act="later"><span>${t("pop.later")}</span></button>
    </div>`);
  const go = card.querySelector('[data-act="install"]');
  go.onclick = async (e) => {
    e.stopPropagation();
    go.disabled = true;
    go.innerHTML = `<span>${t("pop.updating")}</span>`;
    try { await invoke("install_update"); }
    catch (err) { go.disabled = false; go.title = String(err || ""); go.innerHTML = `<span>${t("pop.updateFail")}</span>`; }
  };
  card.querySelector('[data-act="later"]').onclick = (e) => { e.stopPropagation(); remove(card); };
  show(card, n);
}

function add(n) {
  if (n.kind === "update") return addUpdate(n);
  if (n.kind === "clip") return addClip(n);
  if (n.kind === "drop") return addDrop(n);
  addLive(n);
}

async function drain() {
  try { setLang(await invoke("ui_lang")); } catch {}
  const items = await invoke("popup_take");
  items.forEach(add);
}

listen("popup-poke", drain);
drain();
