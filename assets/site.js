// CreaBIM website: theme switch, phone menu, guide contents, and the woven
// logo ident (hero) and film chapter card, both drawn from the logo's paths.
(function () {
  const root = document.documentElement;
  const NS = "http://www.w3.org/2000/svg";
  const el = (t, a = {}, p) => { const e = document.createElementNS(NS, t); for (const k in a) e.setAttribute(k, a[k]); if (p) p.appendChild(e); return e; };
  const P = {
    branch: "M26 52 H174",
    nest: "M130 52 C162 64 172 100 166 128 C160 156 146 166 142 190 L118 190 C114 166 98 156 94 128 C88 100 98 64 130 52 Z",
    bird: "M44.5 8.0 C51.2 2.1 63.8 3.0 67.2 12.2 C69.7 18.9 66.4 24.8 63.0 29.0 C61.3 39.9 52.9 50.0 38.6 50.8 C32.8 51.3 27.7 50.0 23.5 48.3 L8.4 56.7 L16.8 43.3 C14.3 35.7 16.0 27.3 21.8 22.3 C27.7 17.2 37.8 14.7 44.5 8.0 Z",
    beak: "M65.5 10.5 L80.6 16.4 L65.5 21.4 Z",
  };
  const EASE = "cubic-bezier(.22,.8,.24,1)";
  const reduce = matchMedia("(prefers-reduced-motion: reduce)").matches;
  const css = (n) => getComputedStyle(root).getPropertyValue(n).trim();
  const canAnimate = typeof Element !== "undefined" && "animate" in Element.prototype;
  function draw(n, delay, dur) { n.setAttribute("pathLength", 1); n.style.strokeDasharray = 1; if (!canAnimate) return null; return n.animate([{strokeDashoffset: 1}, {strokeDashoffset: 0}], {duration: dur, delay, easing: EASE, fill: "both"}); }
  function fadeIn(n, delay, dur) { return canAnimate ? n.animate([{opacity: 0}, {opacity: 1}], {duration: dur, delay, easing: EASE, fill: "both"}) : null; }

  let identDone = false;
  function ident() {
    const box = document.getElementById("ident");
    if (!box) return;
    box.innerHTML = "";
    const svg = el("svg", {viewBox: "0 0 520 200", role: "img", "aria-hidden": "true"}, box);
    const ink = css("--sage"), bg = css("--paper"), soft = css("--line-2"), deep = css("--sage-deep");
    const g = el("g", {transform: "translate(9 4)"}, svg);
    const clip = el("clipPath", {id: "nestclip"}, svg); el("path", {d: P.nest}, clip);
    const strands = el("g", {"clip-path": "url(#nestclip)"}, g);
    const A = [];
    for (let i = 0; i < 9; i++) {
      const y = 60 + i * 15, d = i % 2 ? 1 : -1;
      A.push(draw(el("path", {d: `M84 ${y} C110 ${y + 18 * d} 150 ${y - 18 * d} 176 ${y + 10}`, fill: "none", stroke: soft, "stroke-width": 7, "stroke-linecap": "round"}, strands), 400 + i * 110, 650));
    }
    A.push(draw(el("path", {d: P.branch, fill: "none", stroke: ink, "stroke-width": 9, "stroke-linecap": "round"}, g), 100, 700));
    A.push(fadeIn(el("path", {d: P.nest, fill: ink}, g), 1550, 500));
    A.push(draw(el("path", {d: "M98 98 L164 124", fill: "none", stroke: bg, "stroke-width": 9, "stroke-linecap": "round"}, g), 1900, 350));
    A.push(draw(el("path", {d: "M96 132 L160 156", fill: "none", stroke: bg, "stroke-width": 9, "stroke-linecap": "round"}, g), 2050, 350));
    const bird = el("g", {}, g);
    el("path", {d: P.bird, fill: ink}, bird); el("path", {d: P.beak, fill: ink}, bird); el("circle", {cx: 61.5, cy: 11.4, r: 2.8, fill: bg}, bird);
    if (canAnimate) A.push(bird.animate([{transform: "translate(-200px,-90px) rotate(-18deg)", opacity: 0}, {transform: "translate(-20px,-14px) rotate(6deg)", opacity: 1, offset: .8}, {transform: "none", opacity: 1}], {duration: 1200, delay: 1700, easing: EASE, fill: "both"}));
    const word = el("text", {x: 214, y: 132, "font-family": "Nunito, sans-serif", "font-weight": 800, "font-size": 104, fill: deep, "letter-spacing": -1}, svg);
    word.textContent = "creabim";
    let ww = 0; try { ww = word.getComputedTextLength(); } catch (e) {}
    if (!ww) ww = 370;
    const left = 11, right = 214 + ww + 6;
    svg.setAttribute("viewBox", `${left} 0 ${right - left} 200`);
    box.style.aspectRatio = `${right - left} / 200`;
    if (canAnimate) A.push(word.animate([{opacity: 0, transform: "translateY(14px)"}, {opacity: 1, transform: "none"}], {duration: 700, delay: 2600, easing: EASE, fill: "both"}));
    // Play once per visit; a theme switch redraws it already finished.
    if (reduce || identDone) A.forEach((a) => a && a.finish());
    identDone = true;
  }

  function poster() {
    const box = document.getElementById("poster");
    if (!box) return;
    box.innerHTML = "";
    const svg = el("svg", {viewBox: "0 0 1600 900", role: "img", "aria-hidden": "true"}, box);
    el("rect", {width: 1600, height: 900, fill: "#f4f2ea"}, svg);
    const n = el("text", {x: 140, y: 360, "font-family": "JetBrains Mono, monospace", "font-size": 40, fill: "#6f9a6a", "letter-spacing": 6}, svg); n.textContent = "01";
    const t = el("text", {x: 140, y: 450, "font-family": "Nunito, sans-serif", "font-weight": 800, "font-size": 64, fill: "#3E5A3B"}, svg); t.textContent = "One model, every drawing";
    const rule = el("path", {d: "M140 510 H1080", stroke: "#4F6B4C", "stroke-width": 6, "stroke-linecap": "round", fill: "none"}, svg);
    const g = el("g", {transform: "translate(984 454) translate(9 4)"}, svg);
    const bird = el("g", {}, g);
    el("path", {d: P.bird, fill: "#4F6B4C"}, bird); el("path", {d: P.beak, fill: "#4F6B4C"}, bird); el("circle", {cx: 61.5, cy: 11.4, r: 2.8, fill: "#f4f2ea"}, bird);
    const sk = el("g", {transform: "translate(1150 300) scale(1.1)", fill: "none", stroke: "#4F6B4C", "stroke-width": 3.5, "stroke-linecap": "round"}, svg);
    ["M0 130 H90", "M100 130 L150 60 H210 V200 H150 Z", "M100 130 H210", "M100 130 L150 200", "M230 60 H330 V200 H230 Z M250 180 H310 M250 168 H290"].forEach((d) => el("path", {d}, sk));
    const s = document.createElement("div"); s.className = "soon"; s.textContent = box.dataset.soon || "Film coming soon"; box.appendChild(s);
    if (reduce || !canAnimate) return;
    const loop = () => {
      n.animate([{opacity: 0}, {opacity: 1}], {duration: 500, fill: "both"});
      t.animate([{opacity: 0, transform: "translateY(18px)"}, {opacity: 1, transform: "none"}], {duration: 650, delay: 150, easing: EASE, fill: "both"});
      draw(rule, 300, 700);
      [...sk.children].forEach((p, i) => draw(p, 300 + i * 200, 800));
      bird.animate([{transform: "translate(-160px,-110px) rotate(-16deg)", opacity: 0}, {transform: "translate(-30px,-18px) rotate(6deg)", opacity: 1, offset: .75}, {transform: "none", opacity: 1}], {duration: 900, delay: 900, easing: EASE, fill: "both"});
      setTimeout(loop, 6000);
    };
    loop();
  }

  // light / dark (the page follows the system until the button is used)
  const themeBtn = document.getElementById("theme");
  if (themeBtn) themeBtn.addEventListener("click", () => {
    const dark = root.dataset.theme ? root.dataset.theme === "dark" : matchMedia("(prefers-color-scheme: dark)").matches;
    root.dataset.theme = dark ? "light" : "dark";
    try { localStorage.setItem("creabim-theme", root.dataset.theme); } catch (e) {}
    ident();
  });

  // phone menu
  const nav = document.querySelector(".nav"), menu = document.getElementById("menu");
  if (menu && nav) {
    menu.addEventListener("click", () => { const o = nav.classList.toggle("open"); menu.setAttribute("aria-expanded", o); });
    nav.querySelectorAll(".links a").forEach((a) => a.addEventListener("click", () => { nav.classList.remove("open"); menu.setAttribute("aria-expanded", "false"); }));
    document.addEventListener("keydown", (e) => { if (e.key === "Escape") { nav.classList.remove("open"); menu.setAttribute("aria-expanded", "false"); } });
  }
  // guide contents on phones
  const st = document.querySelector(".side-toggle"), side = document.querySelector(".side");
  if (st && side) st.addEventListener("click", () => { const o = side.classList.toggle("open"); st.setAttribute("aria-expanded", o); });

  (document.fonts ? document.fonts.ready : Promise.resolve()).then(() => { ident(); poster(); });
})();
// "On this page": light the heading being read
(function () {
  const links = [...document.querySelectorAll(".pagetoc a")];
  if (!links.length || !("IntersectionObserver" in window)) return;
  const heads = links.map((a) => document.getElementById(a.getAttribute("href").slice(1))).filter(Boolean);
  const light = (id) => links.forEach((a) => a.classList.toggle("on", a.getAttribute("href") === "#" + id));
  const seen = new Set();
  const obs = new IntersectionObserver((es) => {
    es.forEach((e) => (e.isIntersecting ? seen.add(e.target.id) : seen.delete(e.target.id)));
    const first = heads.find((h) => seen.has(h.id));
    if (first) return light(first.id);
    const above = heads.filter((h) => h.getBoundingClientRect().top < 100).pop();
    if (above) light(above.id);
  }, {rootMargin: "-80px 0px -60% 0px"});
  heads.forEach((h) => obs.observe(h));
})();
