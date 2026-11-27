// Striimisivu näyttää Twitchille aina olevansa näkyvissä. Twitchin soitin ei käynnisty
// piilossa olevassa välilehdessä (taustakatselu, taustalle avatut välilehdet), joten
// katseluaika, putki ja kanavapisteet eivät kerry. Ääni hoidetaan WebView2-mykistyksellä.
(() => {
  if (window.top !== window || window.__chillyVisibleOn) return;
  window.__chillyVisibleOn = true;
  try {
    const def = (name, value) => Object.defineProperty(Document.prototype, name, { configurable: true, get: () => value });
    def("hidden", false);
    def("webkitHidden", false);
    def("visibilityState", "visible");
    def("webkitVisibilityState", "visible");
    Document.prototype.hasFocus = function () { return true; };
    const swallow = (e) => e.stopImmediatePropagation();
    for (const ev of ["visibilitychange", "webkitvisibilitychange"]) {
      window.addEventListener(ev, swallow, true);
      document.addEventListener(ev, swallow, true);
    }
  } catch (e) {}
})();
