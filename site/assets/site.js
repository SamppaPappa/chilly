// Chilly-sivu: kielivalinta, ikonit, kuvien suurennus, aloitusanimaatio
(() => {
  const root = document.documentElement;

  // Kieli: ?lang= → tallennettu valinta → selaimen kieli
  function pickLang() {
    const q = new URLSearchParams(location.search).get("lang");
    if (q === "fi" || q === "en") return q;
    try { const s = localStorage.getItem("chillyLang"); if (s === "fi" || s === "en") return s; } catch (e) {}
    return (navigator.language || "fi").toLowerCase().startsWith("fi") ? "fi" : "en";
  }
  function setLang(l) {
    root.dataset.lang = l;
    root.lang = l;
    document.querySelectorAll(".seg [data-lang]").forEach((b) => b.classList.toggle("on", b.dataset.lang === l));
    document.querySelectorAll("img[data-src-fi]").forEach((im) => { im.src = im.dataset["src" + (l === "fi" ? "Fi" : "En")]; });
    document.querySelectorAll("[data-alt-fi]").forEach((el) => { el.setAttribute("alt", el.dataset["alt" + (l === "fi" ? "Fi" : "En")]); });
    document.title = l === "fi" ? "Chilly – Twitch Windowsille (beta)" : "Chilly – Twitch for Windows (beta)";
    try { localStorage.setItem("chillyLang", l); } catch (e) {}
  }
  setLang(pickLang());
  document.querySelectorAll(".seg [data-lang]").forEach((b) => b.addEventListener("click", () => setLang(b.dataset.lang)));

  window.hydrateIcons();

  // Aloitusanimaatio kuten sovelluksessa: kirjaimet ja lumihiutaleet
  const wm = document.getElementById("wordmark");
  if (wm) {
    [..."Chilly"].forEach((ch, i) => {
      const s = document.createElement("span");
      s.textContent = ch;
      s.style.animationDelay = (0.75 + i * 0.07) + "s";
      wm.appendChild(s);
    });
    const lw = document.querySelector(".logo-wrap");
    for (let i = 0; i < 14; i++) {
      const f = document.createElement("i");
      f.className = "flake";
      const a = (i / 14) * Math.PI * 2, r = 90 + Math.random() * 70, sz = 3 + Math.random() * 4;
      f.style.cssText = `width:${sz}px;height:${sz}px;--dx:${Math.cos(a) * r}px;--dy:${Math.sin(a) * r}px;animation-delay:${0.55 + Math.random() * 0.3}s`;
      lw.appendChild(f);
    }
  }

  // Kuvan suurennus
  const lb = document.getElementById("lightbox");
  const lbImg = lb.querySelector("img");
  document.querySelectorAll(".shot").forEach((s) => {
    s.tabIndex = 0;
    const open = () => { const im = s.querySelector("img"); lbImg.src = im.currentSrc || im.src; lbImg.alt = im.alt; lb.classList.add("on"); };
    s.addEventListener("click", (e) => { if (!e.target.closest(".toast")) open(); });
    s.addEventListener("keydown", (e) => { if (e.key === "Enter" || e.key === " ") { e.preventDefault(); open(); } });
  });
  const close = () => lb.classList.remove("on");
  lb.addEventListener("click", close);
  document.addEventListener("keydown", (e) => { if (e.key === "Escape") close(); });

  // Linkki oppaan aiheeseen (esim. #opas-asetukset) avaa aiheen valmiiksi
  function openTarget() {
    const id = decodeURIComponent(location.hash.slice(1));
    const el = id && document.getElementById(id);
    if (el && el.tagName === "DETAILS") { el.open = true; el.classList.add("in"); }
  }
  openTarget();
  window.addEventListener("hashchange", openTarget);

  // Osiot liukuvat esiin vierittäessä
  const io = "IntersectionObserver" in window ? new IntersectionObserver((es) => {
    es.forEach((e) => { if (e.isIntersecting) { e.target.classList.add("in"); io.unobserve(e.target); } });
  }, { threshold: 0.12 }) : null;
  document.querySelectorAll(".reveal").forEach((el) => (io ? io.observe(el) : el.classList.add("in")));
})();
