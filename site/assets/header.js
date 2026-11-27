// Headerin partikkelit: lumihiutaleet leijuvat alas, kuumat kipinät nousevat logon alta ja
// jääkiteet välkähtävät logon ympärillä. Pysähtyy, kun header ei näy tai välilehti on taustalla.
(() => {
  const cv = document.getElementById("hdrfx");
  if (!cv || matchMedia("(prefers-reduced-motion: reduce)").matches) return;
  const ctx = cv.getContext("2d");
  let W = 0, H = 0, dpr = 1, running = true, visible = true, last = performance.now();
  const rnd = (a, b) => a + Math.random() * (b - a);

  function resize() {
    dpr = Math.min(devicePixelRatio || 1, 2);
    W = cv.clientWidth; H = cv.clientHeight;
    cv.width = W * dpr; cv.height = H * dpr;
    ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
  }
  resize();
  addEventListener("resize", resize);

  const snow = [], embers = [], sparks = [];
  const nSnow = () => Math.round(Math.min(70, W / 16));
  const mkSnow = (y) => ({ x: rnd(0, W), y: y ?? rnd(-H, H), r: rnd(1.2, 3.4), vy: rnd(10, 26), sway: rnd(.4, 1.2), ph: rnd(0, 6.3), rot: rnd(0, 6.3), vr: rnd(-.8, .8), a: rnd(.35, .85) });
  const mkEmber = () => ({ x: W / 2 + rnd(-90, 90), y: H * 0.72 + rnd(-10, 20), vx: rnd(-12, 12), vy: rnd(-55, -25), life: 0, max: rnd(1.6, 3.2), r: rnd(1, 2.4), hue: rnd(320, 345) });
  const mkSpark = () => { const a = rnd(0, Math.PI * 2), d = rnd(110, 210); return { x: W / 2 + Math.cos(a) * d, y: H * 0.47 + Math.sin(a) * d * 0.62, life: 0, max: rnd(.6, 1.2), s: rnd(5, 10) }; };
  for (let i = 0; i < nSnow(); i++) snow.push(mkSnow());

  function flake(p) {
    ctx.save(); ctx.translate(p.x, p.y); ctx.rotate(p.rot);
    ctx.strokeStyle = `rgba(214,246,255,${p.a})`; ctx.lineWidth = p.r > 2.6 ? 1.2 : 0.9;
    if (p.r > 2.6) {
      ctx.beginPath();
      for (let k = 0; k < 6; k++) { const a = (k * Math.PI) / 3; ctx.moveTo(0, 0); ctx.lineTo(Math.cos(a) * p.r * 1.6, Math.sin(a) * p.r * 1.6); }
      ctx.stroke();
    } else {
      ctx.fillStyle = `rgba(214,246,255,${p.a})`; ctx.fillRect(-p.r / 2, -p.r / 2, p.r, p.r);
    }
    ctx.restore();
  }
  function star(x, y, s, a) {
    ctx.save(); ctx.translate(x, y); ctx.globalAlpha = a;
    const g = ctx.createRadialGradient(0, 0, 0, 0, 0, s);
    g.addColorStop(0, "rgba(255,255,255,1)"); g.addColorStop(0.3, "rgba(127,231,251,.8)"); g.addColorStop(1, "rgba(34,211,238,0)");
    ctx.fillStyle = g; ctx.beginPath(); ctx.arc(0, 0, s, 0, 7); ctx.fill();
    ctx.strokeStyle = "rgba(255,255,255,.9)"; ctx.lineWidth = 1;
    ctx.beginPath(); ctx.moveTo(-s, 0); ctx.lineTo(s, 0); ctx.moveTo(0, -s); ctx.lineTo(0, s); ctx.stroke();
    ctx.restore();
  }

  let emberAcc = 0, sparkAcc = 0;
  function frame(now) {
    if (!running) return;
    const dt = Math.min(0.05, (now - last) / 1000); last = now;
    ctx.clearRect(0, 0, W, H);
    // lumi
    while (snow.length < nSnow()) snow.push(mkSnow(-10));
    for (const p of snow) {
      p.ph += dt * p.sway; p.y += p.vy * dt; p.x += Math.sin(p.ph) * 12 * dt; p.rot += p.vr * dt;
      if (p.y > H + 10) Object.assign(p, mkSnow(-10));
      flake(p);
    }
    // kipinät
    emberAcc += dt * 9;
    while (emberAcc > 1) { embers.push(mkEmber()); emberAcc--; }
    ctx.globalCompositeOperation = "lighter";
    for (let i = embers.length - 1; i >= 0; i--) {
      const e = embers[i]; e.life += dt;
      if (e.life > e.max) { embers.splice(i, 1); continue; }
      e.x += (e.vx + Math.sin(e.life * 6) * 8) * dt; e.y += e.vy * dt;
      const t = e.life / e.max, a = (t < .2 ? t / .2 : 1 - (t - .2) / .8) * 0.9;
      ctx.fillStyle = `hsla(${e.hue},100%,${65 + 15 * (1 - t)}%,${a})`;
      ctx.beginPath(); ctx.arc(e.x, e.y, e.r * (1 - t * .5), 0, 7); ctx.fill();
    }
    // jääkiteiden välähdykset
    sparkAcc += dt * 1.6;
    while (sparkAcc > 1) { sparks.push(mkSpark()); sparkAcc--; }
    for (let i = sparks.length - 1; i >= 0; i--) {
      const s = sparks[i]; s.life += dt;
      if (s.life > s.max) { sparks.splice(i, 1); continue; }
      const t = s.life / s.max; star(s.x, s.y, s.s * Math.sin(t * Math.PI), Math.sin(t * Math.PI));
    }
    ctx.globalCompositeOperation = "source-over";
    requestAnimationFrame(frame);
  }
  const go = () => { const want = visible && !document.hidden; if (want && !running) { running = true; last = performance.now(); requestAnimationFrame(frame); } else if (!want) running = false; };
  new IntersectionObserver((es) => { visible = es[0].isIntersecting; go(); }).observe(cv);
  document.addEventListener("visibilitychange", go);
  requestAnimationFrame(frame);
})();
