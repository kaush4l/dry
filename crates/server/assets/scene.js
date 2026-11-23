// The scene behind the glass on the pages the server draws: the
// core's own scene code (`dry_core::scene`: start, step, frame), compiled
// to /assets/scene.wasm (crates/scene), run here so the cat follows the
// pointer with no trip to the server. This file only feeds it inputs and
// paints its rectangles, as the browser build's `crates/web/src/scene.rs`
// does: the same inputs (the engines' mood, the window in cells, the floor
// at the footer's top edge, the pointer heard on the document in the
// capture phase and never stopped), the same fps gate, and it parks when
// the frame says nothing moves. `prefers-reduced-motion` and the motion
// setting 0 (`--motion`) keep it still.
//
// The page says what to draw: `<canvas id="scene" data-scene="cat">` (the
// app's scene setting) and `<html data-mood="idle|working|needs-you">`
// (Home's tiles' rule over every engine's status), both drawn by the
// server and kept fresh by app.js, which calls `dryScene.sync()` after
// every live update. Colours are resolved by the server for each mode
// (the scene's own, or the theme's for a key with a role) into the page's
// tokens as `--scene-<key>`; `scene_palette` names each key's property, so
// this file only reads and paints them.
(function () {
  "use strict";
  var STRIDE = 7, MOODS = { idle: 0, working: 1, "needs-you": 2 };
  var root = document.documentElement;
  var ex = null, loaded = "", loading = null, palette = {}, colours = {}, coloursFor = "";
  var heard = { pointer: false, x: 0, y: 0, pressed: false, clicks: 0, inside: false };
  var last = 0, running = false, started = false;

  function now() { return performance.now() / 1000; }
  function canvas() { return document.getElementById("scene"); }
  function still() {
    var m = parseFloat(getComputedStyle(root).getPropertyValue("--motion"));
    return (window.matchMedia && matchMedia("(prefers-reduced-motion: reduce)").matches) || m === 0;
  }
  // The floor: the footer row's top edge, so the cat stands on the footer
  // (`layout::INSET + FOOTER_HEIGHT` above the bottom when there is no footer).
  function floorPx(h) {
    var f = document.getElementById("footer");
    var top = f ? f.getBoundingClientRect().top : 0;
    return top > 0 && top <= h ? top : h - 12 - 28;
  }
  function hex(c) {
    var m = /^#?([0-9a-f]{6})([0-9a-f]{2})?$/i.exec((c || "").trim());
    if (!m) return null;
    var v = parseInt(m[1], 16);
    return [v >> 16, (v >> 8) & 255, v & 255, m[2] ? parseInt(m[2], 16) / 255 : 1];
  }
  function readColours() {
    var key = root.getAttribute("data-mode") + "|" + ((document.getElementById("tokens") || {}).textContent || "").length;
    if (key === coloursFor) return;
    coloursFor = key;
    var cs = getComputedStyle(root);
    colours = {};
    Object.keys(palette).forEach(function (k) { colours[k] = hex(cs.getPropertyValue("--" + palette[k])); });
  }
  function text(ptr, len) { return new TextDecoder().decode(new Uint8Array(ex.memory.buffer, ptr, len)); }

  // The inputs now: step the scene at this moment (a click is timed when
  // it happened), and draw on the next frame if the picture changed.
  function inputs() {
    if (!ex || !loaded) return;
    var cell = ex.scene_cell(), w = innerWidth, h = innerHeight;
    var mood = MOODS[root.getAttribute("data-mood")] || 0;
    var changed = ex.scene_set(mood, w / cell, h / cell, Math.floor(floorPx(h) / cell), still() ? 1 : 0,
      heard.pointer ? 1 : 0, heard.x / cell, heard.y / cell, heard.pressed ? 1 : 0, heard.clicks, heard.inside ? 1 : 0, now());
    if (changed) { last = 0; wake(); }
  }

  function paint() {
    var c = canvas();
    if (!ex || !loaded || !c) { running = false; return; }
    var t = now(), fps = ex.scene_fps();
    if (t - last < 1 / fps - 0.004) { requestAnimationFrame(paint); return; }
    var dpr = window.devicePixelRatio || 1, w = innerWidth, h = innerHeight;
    var pw = Math.floor(w * dpr), ph = Math.floor(h * dpr);
    if (c.width !== pw || c.height !== ph) { c.width = pw; c.height = ph; }
    inputs();
    readColours();
    var cell = ex.scene_cell(), n = ex.scene_frame(t);
    var d = new Float32Array(ex.memory.buffer, ex.scene_draws(), n * STRIDE);
    var ctx = c.getContext("2d");
    last = t;
    ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
    ctx.clearRect(0, 0, w, h);
    for (var i = 0; i < n; i++) {
      var o = i * STRIDE, rgb = colours[String.fromCodePoint(d[o + 4])];
      if (!rgb) continue;
      var a = rgb[3] * d[o + 5];
      if (a <= 0) continue;
      var fill = "rgba(" + rgb[0] + "," + rgb[1] + "," + rgb[2] + "," + a.toFixed(3) + ")";
      ctx.fillStyle = fill;
      if (d[o + 6] > 0) { ctx.shadowColor = fill; ctx.shadowBlur = d[o + 6] * cell; } else ctx.shadowBlur = 0;
      ctx.fillRect(d[o] * cell, d[o + 1] * cell, d[o + 2] * cell, d[o + 3] * cell);
    }
    // Parks when nothing will move; an input or a new scene wakes it.
    if (ex.scene_next() < 0) { running = false; return; }
    requestAnimationFrame(paint);
  }
  function wake() { if (!running) { running = true; requestAnimationFrame(paint); } }

  function clear() {
    var c = canvas();
    loaded = "";
    if (c) { c.width = 0; }
  }
  function module() {
    if (!loading) {
      loading = fetch("/assets/scene.wasm")
        .then(function (r) { if (!r.ok) throw new Error("no scene.wasm"); return r.arrayBuffer(); })
        .then(function (b) { return WebAssembly.instantiate(b, {}); })
        .then(function (m) { ex = m.instance.exports; return ex; });
    }
    return loading;
  }
  // Load the scene the page names (a new one replaces the old; none clears).
  function load(name) {
    if (!name) { clear(); return; }
    Promise.all([module(), fetch("/assets/scenes/" + encodeURIComponent(name) + ".md").then(function (r) { if (!r.ok) throw new Error(name); return r.text(); })])
      .then(function (got) {
        if (want !== name) return;
        var bytes = new TextEncoder().encode(got[1]);
        var p = ex.scene_alloc(bytes.length);
        new Uint8Array(ex.memory.buffer, p, bytes.length).set(bytes);
        if (!ex.scene_load(p, bytes.length)) { console.warn("scene " + name + ": " + text(ex.scene_text(), ex.scene_text_len())); clear(); return; }
        palette = {};
        var plen = ex.scene_palette(); // first: it rewrites the text `scene_text` points at
        text(ex.scene_text(), plen).split("\n").forEach(function (l) { var kv = l.split("\t"); if (kv.length === 2) palette[kv[0]] = kv[1]; });
        var c = canvas(); if (c) c.setAttribute("data-place", ex.scene_front() ? "front" : "back");
        coloursFor = "";
        loaded = name;
        last = 0;
        inputs();
        wake();
      })
      .catch(function (e) { console.warn("no scene: " + e); clear(); });
  }

  // After a live update or a new page: the scene, the mood, the colours.
  function sync() {
    var c = canvas(), name = c ? c.getAttribute("data-scene") || "" : "";
    if (name !== want) { want = name; load(name); return; }
    coloursFor = "";
    inputs();
    wake();
  }
  var want = null;

  function listen() {
    if (started) return;
    started = true;
    var at = function (e) { heard.pointer = true; heard.x = e.clientX; heard.y = e.clientY; heard.inside = true; };
    document.addEventListener("mousemove", function (e) { at(e); inputs(); }, true);
    document.addEventListener("mousedown", function (e) { at(e); heard.pressed = true; heard.clicks = (heard.clicks + 1) >>> 0; inputs(); }, true);
    document.addEventListener("mouseup", function (e) { heard.pointer = true; heard.x = e.clientX; heard.y = e.clientY; heard.pressed = false; inputs(); }, true);
    root.addEventListener("mouseleave", function () { heard.pointer = true; heard.inside = false; inputs(); }, true);
    window.addEventListener("resize", function () { inputs(); wake(); });
  }

  window.dryScene = { sync: sync };
  function boot() { listen(); sync(); }
  if (document.readyState === "loading") document.addEventListener("DOMContentLoaded", boot); else boot();
})();
