// The web client's script: the navigation button, soft navigation, and
// live updates. No framework: the server draws every page; this only
// fetches a fresh copy and swaps the parts that changed, so nothing flashes.
//
// Live: `/events` (server-sent events) says `change` when the store or the
// files the app is built from change. The page refetches itself, swaps
// `#tokens` (the theme) and morphs `#nav` and `#pane` into the fresh copy
// node by node (`morph`): only what changed is touched, so open folds,
// scroll positions, a selection and focus stay, a streaming answer and a
// terminal's output grow in place, and boxes marked `data-stick` keep to
// their newest line. A dropped connection reconnects; on `hello` a page
// drawn at an older generation (`data-gen`) refreshes too. The host's
// turn events (`activity`, `message`, `turn`) refresh it the same way, so
// tints and the chat follow a running turn. What is typed (the composer,
// an open ask's picks) survives every swap.
//
// Sending (the host, docs/web.md): the controls with `data-act` post an
// intent with `fetch` to `/api/…`, with `base` (the generation this page
// was drawn at). The host refuses a send to an engine in a turn, or made
// against a view that is stale (409): the composer says why, as the window
// does, and the text stays.
(function () {
  "use strict";
  var root = document.documentElement;
  var COMPACT = 420;

  // ---- the navigation button (layout rule 1) ----
  function setNav(open) {
    root.classList.toggle("nav-open", open);
    try { localStorage.setItem("dry.nav", open ? "open" : "closed"); } catch (e) {}
  }
  document.addEventListener("click", function (ev) {
    var toggle = ev.target.closest("[data-nav-toggle]");
    if (toggle) { setNav(!root.classList.contains("nav-open")); return; }
    var a = ev.target.closest("a[href]");
    if (!a || ev.defaultPrevented || ev.button !== 0 || ev.metaKey || ev.ctrlKey || ev.shiftKey || a.target) return;
    var url = new URL(a.href, location.href);
    if (url.origin !== location.origin || url.pathname.indexOf("/assets/") === 0) return;
    ev.preventDefault();
    // An overlay closes when an entry is picked.
    if (a.closest("#nav") && innerWidth < COMPACT && !root.classList.contains("nav-pinned")) root.classList.remove("nav-open");
    go(url.href, true);
  });
  window.addEventListener("popstate", function () { go(location.href, false); });

  // ---- swapping in a fresh copy of a page ----
  var busy = null, again = false;

  // ---- morphing: the fresh page laid over the live one, node by node ----
  // Only what changed is touched, so a streaming answer grows in place,
  // a terminal gains its new lines, and everything else (scroll positions,
  // a selection, focus, an open fold, a running spinner) stays as it is.
  function keyOf(n) {
    if (n.nodeType !== 1) return null;
    return n.id ? "#" + n.id : n.getAttribute("data-key") || n.getAttribute("data-session") || null;
  }
  function sameKind(a, b) {
    return a.nodeType === b.nodeType && (a.nodeType !== 1 || a.tagName === b.tagName);
  }
  // An ask still open keeps the box the person is filling in.
  function keepAsIs(from, to) {
    return from.matches && from.matches(".ask[data-msg]:not(.answered)") && to.matches(".ask[data-msg]:not(.answered)") &&
      from.getAttribute("data-msg") === to.getAttribute("data-msg") && from.getAttribute("data-engine") === to.getAttribute("data-engine");
  }
  function syncAttrs(from, to) {
    var keepOpen = from.tagName === "DETAILS" && from.hasAttribute("data-key");
    for (var i = from.attributes.length - 1; i >= 0; i--) {
      var name = from.attributes[i].name;
      if (!to.hasAttribute(name) && !(keepOpen && name === "open")) from.removeAttribute(name);
    }
    for (var j = 0; j < to.attributes.length; j++) {
      var a = to.attributes[j];
      if (keepOpen && a.name === "open") continue;
      if (a.name === "value" && from === document.activeElement) continue;
      if (from.getAttribute(a.name) !== a.value) from.setAttribute(a.name, a.value);
    }
  }
  function morph(from, to) {
    if (from.nodeType === 3 || from.nodeType === 8) {
      if (from.nodeValue !== to.nodeValue) from.nodeValue = to.nodeValue;
      return;
    }
    if (from.nodeType !== 1 || keepAsIs(from, to)) return;
    // A terminal the reader scrolled up in holds still (its tail slides
    // otherwise); it catches up once they are back at the newest line.
    if (from.__away) { from.classList.add("paused"); return; }
    if (from.classList.contains("paused")) from.classList.remove("paused");
    syncAttrs(from, to);
    // What is typed lives in the field, not in its markup.
    if (from.tagName === "TEXTAREA") return;
    morphChildren(from, to);
  }
  function morphChildren(from, to) {
    var keyed = {};
    for (var c = from.firstChild; c; c = c.nextSibling) { var k = keyOf(c); if (k) keyed[k] = c; }
    var cur = from.firstChild, fresh = to.firstChild;
    while (fresh) {
      var next = fresh.nextSibling, k2 = keyOf(fresh), match = null;
      if (k2 && keyed[k2] && sameKind(keyed[k2], fresh)) { match = keyed[k2]; delete keyed[k2]; }
      else if (cur && sameKind(cur, fresh) && !(keyOf(cur) && keyed[keyOf(cur)] === cur && keyOf(cur) !== k2)) match = cur;
      if (match) {
        if (match === cur) cur = cur.nextSibling;
        else from.insertBefore(match, cur);
        morph(match, fresh);
      } else {
        from.insertBefore(document.importNode(fresh, true), cur);
      }
      fresh = next;
    }
    while (cur) { var gone = cur; cur = cur.nextSibling; from.removeChild(gone); }
  }

  // A box that keeps to its newest line (`data-stick`: a terminal, the
  // live chat): reversed columns sit at the bottom by themselves; one the
  // reader scrolled up keeps the same lines in view as lines come in.
  function sticks() {
    return Array.prototype.map.call(document.querySelectorAll("#pane [data-stick]"), function (el) {
      var reversed = getComputedStyle(el).flexDirection === "column-reverse";
      var fromEnd = reversed ? -el.scrollTop : el.scrollHeight - el.clientHeight - el.scrollTop;
      var bottom = fromEnd < 8;
      el.__away = !bottom && el.classList.contains("term-scroll");
      return { el: el, reversed: reversed, top: el.scrollTop, height: el.scrollHeight, bottom: bottom };
    });
  }
  function restick(list) {
    list.forEach(function (s) {
      s.el.__away = false;
      if (!s.el.isConnected) return;
      if (s.bottom) { s.el.scrollTop = s.reversed ? 0 : s.el.scrollHeight; return; }
      // Reversed: the distance from the end is kept, so move by what was added.
      if (s.reversed) s.el.scrollTop = s.top - (s.el.scrollHeight - s.height);
    });
  }
  // The answer being written stays in view while it grows, if it was.
  function streamingEdge(main) {
    var m = main && main.querySelector("[data-streaming]");
    if (!m) return null;
    var box = main.getBoundingClientRect(), r = m.getBoundingClientRect();
    return r.bottom <= box.bottom + 4 && r.bottom >= box.top ? m : null;
  }

  function swap(doc, keep) {
    var tokens = doc.getElementById("tokens"), mine = document.getElementById("tokens");
    if (tokens && mine && tokens.textContent !== mine.textContent) mine.textContent = tokens.textContent;
    root.setAttribute("data-mode", doc.documentElement.getAttribute("data-mode") || "dark");
    root.setAttribute("data-gen", doc.documentElement.getAttribute("data-gen") || "0");
    // The scene (scene.js): the engines' mood and the scene setting's name (its colours are in #tokens).
    root.setAttribute("data-mood", doc.documentElement.getAttribute("data-mood") || "idle");
    var sc = doc.getElementById("scene"), mysc = document.getElementById("scene");
    if (sc && mysc) mysc.setAttribute("data-scene", sc.getAttribute("data-scene") || "");
    ["nav-pinned", "nav-float"].forEach(function (c) { root.classList.toggle(c, doc.documentElement.classList.contains(c)); });
    if (document.title !== doc.title) document.title = doc.title;
    var main = document.getElementById("main");
    var stuck = keep ? sticks() : [];
    var follow = keep ? streamingEdge(main) : null;
    var kept = keepTyped();
    ["nav", "pane"].forEach(function (id) {
      var fresh = doc.getElementById(id), old = document.getElementById(id);
      if (!fresh || !old || fresh.outerHTML === old.outerHTML) return;
      // A live update morphs; a new page replaces (nothing of the old one carries over).
      if (keep) morph(old, fresh);
      else old.replaceWith(document.importNode(fresh, true));
    });
    putTyped(kept);
    main = document.getElementById("main");
    if (!keep && main) main.scrollTop = 0;
    restick(stuck);
    if (follow && follow.isConnected && main) {
      var over = follow.getBoundingClientRect().bottom - main.getBoundingClientRect().bottom;
      if (over > 0) main.scrollTop += over + 8;
    }
    viewport();
    liveState();
    if (window.dryScene) window.dryScene.sync();
  }

  function go(href, push, then) {
    fetch(href, { headers: { "X-Dry-Live": "1" }, cache: "no-store" })
      .then(function (r) { return r.text(); })
      .then(function (html) {
        var doc = new DOMParser().parseFromString(html, "text/html");
        if (push) history.pushState(null, "", href);
        swap(doc, false);
        if (then) then();
      })
      .catch(function () { location.href = href; });
  }

  function refresh() {
    if (busy) { again = true; return; }
    var at = location.href;
    busy = fetch(at, { headers: { "X-Dry-Live": "1" }, cache: "no-store" })
      .then(function (r) { return r.text(); })
      .then(function (html) {
        // The page moved on while this was on its way (a navigation): it
        // is a copy of the old page, so fetch the new one instead.
        if (location.href !== at) { again = true; return; }
        swap(new DOMParser().parseFromString(html, "text/html"), true);
      })
      .catch(function () {})
      .then(function () { busy = null; if (again) { again = false; refresh(); } });
  }

  // ---- live updates ----
  var state = "connecting";
  function liveState() {
    var el = document.getElementById("live");
    if (!el) return;
    el.className = "live " + (state === "live" ? "on" : state === "offline" ? "off" : "");
    var word = el.querySelector(".word");
    if (word) word.textContent = state;
  }
  if (window.EventSource) {
    var es = new EventSource("/events");
    es.addEventListener("hello", function (ev) {
      state = "live"; liveState();
      try {
        var gen = JSON.parse(ev.data).gen;
        if (String(gen) !== root.getAttribute("data-gen")) refresh();
      } catch (e) {}
    });
    // `reply`: an answer streaming in (its view is in the page, drawn by the host).
    ["change", "activity", "message", "turn", "reply", "tool", "approval"].forEach(function (name) {
      es.addEventListener(name, function () { refresh(); });
    });
    es.addEventListener("open", function () { state = "live"; liveState(); });
    es.onerror = function () { state = "offline"; liveState(); };
  }

  // ---- what is typed survives a swap ----
  function askKey(bx) { return bx.getAttribute("data-engine") + "/" + bx.getAttribute("data-msg"); }
  function keepTyped() {
    var k = { values: {}, focus: null, sel: null, asks: {} };
    document.querySelectorAll("#pane [data-keep]").forEach(function (el) {
      var key = el.getAttribute("data-keep");
      k.values[key] = el.value;
      if (el === document.activeElement) { k.focus = key; k.sel = [el.selectionStart, el.selectionEnd]; }
    });
    // An open ask keeps its picks: the box itself stays while it is still open.
    document.querySelectorAll("#pane .ask[data-msg]:not(.answered)").forEach(function (bx) { k.asks[askKey(bx)] = bx; });
    return k;
  }
  function putTyped(k) {
    document.querySelectorAll("#pane [data-keep]").forEach(function (el) {
      var key = el.getAttribute("data-keep");
      if (key in k.values && el.value !== k.values[key]) { el.value = k.values[key]; if (el.tagName === "TEXTAREA") grow(el); }
      if (k.focus === key) { el.focus(); try { el.setSelectionRange(k.sel[0], k.sel[1]); } catch (e) {} }
    });
    // (An open ask was kept by `morph`; after a new page it is the fresh one.)
    document.querySelectorAll("#pane .ask[data-msg]:not(.answered)").forEach(function (bx) {
      var old = k.asks[askKey(bx)];
      if (old && old !== bx && !old.isConnected) bx.replaceWith(old);
    });
    Object.keys(notes).forEach(function (target) { showNote(target, notes[target]); });
    // A `/` list open before the swap opens again.
    document.querySelectorAll("#pane form[data-composer] textarea").forEach(function (t) { if (t.value.charAt(0) === "/") slashMenu(t); });
  }

  // ---- sending: intents to the host ----
  var notes = {}; // composer target -> the host's last refusal, until the next send
  var client = "web-" + Math.random().toString(36).slice(2, 10);

  function base() { var g = parseInt(root.getAttribute("data-gen") || "", 10); return isNaN(g) ? null : g; }

  function post(path, body) {
    body = body || {};
    if (!("base" in body)) body.base = base();
    return fetch(path, { method: "POST", headers: { "Content-Type": "application/json" }, body: JSON.stringify(body), cache: "no-store" })
      .then(function (r) {
        return r.text().then(function (t) {
          var v = {};
          try { v = JSON.parse(t); } catch (e) { v = { error: t || String(r.status) }; }
          v.status = r.status;
          v.ok = r.status >= 200 && r.status < 300;
          return v;
        });
      })
      .catch(function (e) { return { ok: false, status: 0, error: "the host did not answer: " + e }; });
  }

  var toastTimer = null;
  function toast(text) {
    var t = document.getElementById("toast");
    if (!t) {
      t = document.createElement("div");
      t.id = "toast";
      t.setAttribute("role", "status");
      t.style.cssText = "position:fixed;left:50%;bottom:56px;transform:translateX(-50%);z-index:50;max-width:min(560px,90vw);padding:8px 14px;border-radius:12px;background:var(--surface_raised,#222);color:var(--text_primary,#eee);box-shadow:0 6px 24px rgba(0,0,0,.3);font-size:var(--text_sm,13px);transition:opacity .2s";
      document.body.appendChild(t);
    }
    t.textContent = text;
    t.style.opacity = "1";
    clearTimeout(toastTimer);
    toastTimer = setTimeout(function () { t.style.opacity = "0"; }, 4000);
  }

  function showNote(target, text) {
    var f = document.querySelector('form[data-composer][data-engine="' + target + '"]');
    var n = f && f.querySelector("[data-composer-note]");
    if (!n) return;
    if (!n.hasAttribute("data-was")) n.setAttribute("data-was", n.textContent);
    n.textContent = text;
    n.classList.add("bad");
  }
  function clearNote(target) {
    delete notes[target];
    var f = document.querySelector('form[data-composer][data-engine="' + target + '"]');
    var n = f && f.querySelector("[data-composer-note]");
    if (n && n.hasAttribute("data-was")) { n.textContent = n.getAttribute("data-was"); n.removeAttribute("data-was"); n.classList.remove("bad"); }
  }

  function send(form) {
    var t = form.querySelector("textarea");
    if (!t || !t.value.trim()) return;
    var target = form.getAttribute("data-engine") || "new";
    var text = t.value;
    // The Desk's composer: a guided post on its thread, to the picked engine.
    var thread = form.getAttribute("data-thread");
    var sel = form.querySelector('select[name="to"]');
    var req = thread !== null
      ? post("/api/threads/" + encodeURIComponent(thread || "top") + "/posts", { text: text, to: sel ? sel.value : null, client: client })
      : post("/api/engines/" + encodeURIComponent(target) + "/messages", { text: text, client: client });
    req.then(function (v) {
      if (v.ok) {
        clearNote(target);
        var now = document.querySelector('form[data-composer][data-engine="' + target + '"] textarea');
        if (now && now.value === text) { now.value = ""; grow(now); }
        if (thread !== null && v.busy) toast(v.to + " is running: it reads your post when its turn ends.");
        if (target === "new" && typeof v.index === "number") go("/desk/" + v.index, true);
        else refresh();
      } else {
        notes[target] = v.error || "not sent";
        showNote(target, notes[target]);
        if (v.status === 409 && /^stale/.test(v.error || "")) refresh();
      }
    });
  }

  function number(n) { var r = Math.round(n * 1e6) / 1e6; return String(r); }
  function today() { var d = new Date(); return Math.floor((d.getTime() - d.getTimezoneOffset() * 60000) / 86400000); }
  function dayOf(el, name) { var v = el && el.getAttribute(name); return v === null || v === undefined || v === "" ? null : parseInt(v, 10); }
  function typedDay(field) {
    var i = field.querySelector('input[name="day"]');
    if (!i || !i.value) return null;
    var p = i.value.split("-").map(Number);
    return Math.floor(Date.UTC(p[0], p[1] - 1, p[2]) / 86400000);
  }
  function inputOf(field) {
    var picked = [];
    if (field.getAttribute("data-chosen")) picked.push(field.getAttribute("data-chosen"));
    field.querySelectorAll('[data-act="ask-toggle"].on').forEach(function (e) { picked.push(e.getAttribute("data-value")); });
    var typed = field.querySelector('input[name="number"]') || field.querySelector('input[name="text"]');
    var day = typedDay(field);
    if (day === null) day = dayOf(field, "data-picked");
    return { picked: picked, day: day, minute: dayOf(field, "data-minute"), typed: typed ? typed.value : "" };
  }

  // An ask's control: what it picked, for the host to turn into the
  // answer the core writes (`Ask::answer`), the person's next message.
  function answer(b) {
    var bx = b.closest(".ask");
    if (!bx) return;
    var kind = bx.getAttribute("data-ask"), value = b.getAttribute("data-value") || "";
    var pick = {};
    var input = function (name) { var i = bx.querySelector('input[name="' + name + '"]'); return i ? i.value : ""; };
    switch (kind) {
      case "choice": case "confirm": pick.input = { picked: [value] }; break;
      case "multi":
        pick.input = { picked: Array.prototype.map.call(bx.querySelectorAll('[data-act="ask-toggle"].on'), function (e) { return e.getAttribute("data-value"); }) };
        break;
      case "date": pick.input = { day: dayOf(b, "data-day") !== null ? dayOf(b, "data-day") : today() }; break;
      case "date-time": case "datetime": {
        var d = dayOf(bx, "data-picked");
        if (d === null) d = dayOf(b, "data-day");
        pick.input = { day: d === null ? today() : d, minute: dayOf(b, "data-minute") !== null ? dayOf(b, "data-minute") : 540 };
        break;
      }
      case "number": pick.input = { typed: input("number") }; break;
      case "text":
        if (!input("text").trim()) return toast("Type an answer first.");
        pick.input = { typed: input("text") };
        break;
      case "review":
        if (value === "reject") pick.reject = true;
        else pick.rows = Array.prototype.map.call(bx.querySelectorAll('input[name="row"]'), function (i) { return [i.getAttribute("data-key"), i.value]; });
        break;
      case "form": pick.inputs = Array.prototype.map.call(bx.querySelectorAll(".ask-field"), inputOf); break;
      default: pick.input = { picked: value ? [value] : [] };
    }
    var target = bx.getAttribute("data-engine"), msg = parseInt(bx.getAttribute("data-msg"), 10);
    post("/api/engines/" + encodeURIComponent(target) + "/answers", { message: msg, pick: pick, client: client }).then(function (v) {
      if (v.ok) refresh();
      else { toast(v.error || "not sent"); if (v.status === 409) refresh(); }
    });
  }

  function act(b) {
    var what = b.getAttribute("data-act"), value = b.getAttribute("data-value") || "", engine = b.getAttribute("data-engine");
    var done = function (v) { if (!v.ok) toast(v.error || "refused"); refresh(); };
    switch (what) {
      case "send": { var f = b.closest("form[data-composer]"); if (f) send(f); return; }
      case "clear": { var t = b.closest("form[data-composer]"); t = t && t.querySelector("textarea"); if (t) t.value = ""; return; }
      case "ask": return answer(b);
      case "ask-toggle": {
        var on = b.classList.toggle("on");
        b.textContent = (on ? "☑ " : "☐ ") + value;
        var bx = b.closest(".ask");
        if (bx && !b.closest(".ask-field")) {
          var s = bx.querySelector('[data-act="ask"]');
          if (s) s.textContent = "Send (" + bx.querySelectorAll('[data-act="ask-toggle"].on').length + ")";
        }
        return;
      }
      case "ask-pick": {
        var box = b.closest(".ask-field") || b.closest(".ask");
        if (!box) return;
        var kinds = [["data-day", "data-picked"], ["data-minute", "data-minute"], ["data-value", "data-chosen"]];
        var k = kinds.filter(function (p) { return b.hasAttribute(p[0]); })[0] || kinds[0];
        box.querySelectorAll(".sel[" + k[0] + "]").forEach(function (e) { e.classList.remove("sel"); });
        b.classList.add("sel");
        box.setAttribute(k[1], b.getAttribute(k[0]) || "");
        return;
      }
      case "ask-step": {
        var holder = b.closest(".ask-field") || b.closest(".ask");
        var i = holder && holder.querySelector('input[name="number"]');
        if (i) i.value = number((parseFloat(i.value) || 0) + (parseFloat(b.getAttribute("data-step")) || 0));
        return;
      }
      case "ask-edit": { var a = b.closest(".ask"); if (a) a.querySelectorAll('input[name="row"]').forEach(function (r) { r.readOnly = false; }); return; }
      // A new thread (a root from the default's translator) or session (a
      // root run by a program): open its thread with the composer on it.
      // `new-engine` is the old name of `new-thread`.
      case "new-thread": case "new-engine": case "new-session":
        return post("/api/engines", what === "new-session" ? { kind: value } : { kind: "thread" }).then(function (v) {
          if (!v.ok) return toast(v.error || "refused");
          var id = encodeURIComponent(v.id);
          go("/desk?thread=" + id + "&to=" + id, true, function () {
            var ta = document.querySelector("form[data-thread] textarea");
            if (ta) ta.focus();
          });
        });
      case "approve": {
        var id = b.getAttribute("data-id");
        var reason = value === "deny" ? (prompt("Why not? (the session reads this)", "") || "") : "";
        return post("/api/approvals/" + encodeURIComponent(id), { decision: value, reason: reason }).then(done);
      }
      case "slash": {
        var f = b.closest("form[data-composer]"), ta = f && f.querySelector("textarea");
        if (ta) { ta.value = "/" + value + " "; ta.focus(); slashMenu(ta); }
        return;
      }
      case "delete-engine": {
        var name = (document.querySelector(".here") || {}).textContent || "this engine";
        if (!confirm("Delete " + name + " and its child engines? This cannot be undone.")) return;
        return post("/api/engines/" + encodeURIComponent(engine) + "/delete", {}).then(function (v) { if (v.ok) go("/desk", true); else toast(v.error || "refused"); });
      }
      case "set-format": case "format":
        return post("/api/engines/" + encodeURIComponent(engine) + "/format", { value: what === "format" ? "" : value }).then(done);
      case "set-mode": return post("/api/mode", { value: value }).then(done);
      case "set-scene": return post("/api/scene", { value: value }).then(done);
      case "detect-model": case "test-model": {
        var row = b.closest("[data-model]") || b.parentNode;
        var line = row.parentNode.querySelector('[data-model-line="' + value + '"]');
        if (!line) { line = document.createElement("div"); line.className = "note"; line.setAttribute("data-model-line", value); row.parentNode.insertBefore(line, row.nextSibling); }
        line.classList.remove("bad");
        line.textContent = what === "detect-model" ? "asking the server for its models…" : "sending a word (the first call can take a while)…";
        return post("/api/models/" + encodeURIComponent(value) + "/" + (what === "detect-model" ? "detect" : "test"), {}).then(function (v) {
          line.textContent = v.line || v.error || "no answer";
          if (!v.ok) line.classList.add("bad");
        });
      }
      case "join-peer": {
        var f = b.closest("form[data-peer]"), get = function (n) { var i = f && f.querySelector('input[name="' + n + '"]'); return i ? i.value.trim() : ""; };
        b.disabled = true;
        return post("/api/capabilities/peers", { url: get("peer-url"), code: get("peer-code"), name: get("peer-name") }).then(function (v) {
          b.disabled = false;
          if (!v.ok) toast(v.error || "could not join"); else toast("joined " + v.peer);
          refresh();
        });
      }
      case "leave-peer": return post("/api/capabilities/peers/" + encodeURIComponent(value) + "/leave", {}).then(done);
      case "refresh": return refresh();
      // The app's own files, written by the host (files.rs).
      case "set-theme": return post("/api/theme", { value: value }).then(done);
      // A theme or scene is a folder (dry_core::look): copy one, remove one, or import a shared one.
      case "look-new": {
        var from = value.split(":"), name = prompt("A name for the copy of the " + from[0] + " " + from[1] + " (letters, digits, - and _)", from[1] + "-copy");
        if (!name) return;
        return post("/api/looks/" + from[0] + "/new", { name: name.trim(), from: from[1] }).then(function (v) { if (v.ok) toast("made " + from[0] + " " + v.name + " (folder " + from[0] + "s/" + v.name + "/)"); done(v); });
      }
      case "look-rm": {
        var it = value.split(":"), spare = it[1].split(",");
        var gone = prompt("Which " + it[0] + " to delete? (not the one in use): " + spare.join(", "), "");
        if (!gone) return;
        gone = gone.trim();
        if (spare.indexOf(gone) < 0) return toast("no " + it[0] + " " + gone + " to delete; one of " + spare.join(", "));
        if (!confirm("Delete the " + it[0] + " " + gone + "? Its folder " + it[0] + "s/" + gone + "/ is removed. Export it first to keep a copy.")) return;
        return post("/api/looks/" + it[0] + "/rm", { name: gone }).then(done);
      }
      case "look-import": {
        var pick = document.createElement("input");
        pick.type = "file"; pick.accept = ".md,text/markdown,text/plain";
        pick.onchange = function () {
          var file = pick.files && pick.files[0];
          if (!file) return;
          file.text().then(function (text) {
            var send = function (replace) {
              return post("/api/looks/import", { text: text, replace: replace }).then(function (v) {
                if (!v.ok && !replace && /already exists/.test(v.error || "") && confirm((v.error || "") + "\n\nReplace it?")) return send(true);
                if (v.ok) toast("imported " + v.kind + " " + v.name);
                done(v);
              });
            };
            if (!/^\s*# dry (theme|scene):/.test(text)) return toast("not a shared " + value + ": its first line is # dry " + value + ": <name>");
            return send(false);
          });
        };
        pick.click();
        return;
      }
      case "reset-theme":
        if (!confirm("Put every token of this theme back to its default? This rewrites the theme file.")) return;
        return post("/api/theme/reset", {}).then(done);
      case "make-default": return post("/api/models/" + encodeURIComponent(value) + "/default", {}).then(done);
      case "route-hide": return post("/api/routes/hide", { value: value }).then(done);
      case "route-off": return post("/api/routes/off", { value: value }).then(done);
      case "route-delete":
        if (!confirm("Delete the page " + value + "? Its page.json is removed.")) return;
        return post("/api/routes/delete", { value: value }).then(done);
      case "route-create": {
        var nf = b.closest("form[data-new-route]"), field = function (n) { var i = nf && nf.querySelector('input[name="' + n + '"]'); return i ? i.value.trim() : ""; };
        return post("/api/routes/create", { path: field("path"), title: field("title"), icon: field("icon") }).then(function (v) {
          if (!v.ok) return toast(v.error || "refused");
          if (nf) nf.querySelectorAll("input").forEach(function (i) { i.value = ""; });
          go(v.pattern, true);
        });
      }
      // The second press of Reset history (its fold is the confirm step).
      case "reset-history":
        b.disabled = true;
        return post("/api/reset", { confirm: true }).then(function (v) {
          b.disabled = false;
          if (!v.ok) return toast(v.error || "refused");
          var fold = b.closest("details");
          if (fold) fold.open = false;
          toast(v.backup ? "History reset. The old store is in " + v.backup : "Nothing to reset: the store is the default already.");
          go("/desk", true);
        });
      case "reload-responses": return post("/api/engines/" + encodeURIComponent(engine) + "/reload", { what: "responses" }).then(done);
      case "reload-spec": return post("/api/engines/" + encodeURIComponent(engine) + "/reload", { what: "spec" }).then(done);
      case "run-terminal": { var tf = b.closest("form[data-terminal]"); if (tf) runLine(tf); return; }
    }
  }

  // The Code page's terminal on the host: the line runs there; every page
  // sees its output stream in (`change` events), so nothing else to do.
  function runLine(form) {
    var i = form.querySelector('input[name="line"]');
    if (!i || !i.value.trim()) return;
    var line = i.value;
    post("/api/terminal", { line: line }).then(function (v) {
      if (!v.ok) return toast(v.error || "not run");
      var now = document.querySelector('form[data-terminal] input[name="line"]');
      if (now && now.value === line) now.value = "";
      refresh();
    });
  }

  document.addEventListener("click", function (ev) {
    var b = ev.target.closest("[data-act]");
    if (!b || b.disabled || !b.closest("#pane, #nav")) return;
    ev.preventDefault();
    act(b);
  });
  // ---- the composer's / list: a session's commands and skills ----
  function slashMenu(ta) {
    var f = ta.closest("form[data-composer]");
    if (!f) return;
    var menu = f.querySelector(".slash-menu");
    var all = (f.getAttribute("data-slash") || "").split(" ").filter(Boolean);
    var m = /^\/(\S*)$/.exec(ta.value);
    if (!all.length || !m) { if (menu) menu.remove(); return; }
    var typed = m[1].toLowerCase();
    var hits = all.filter(function (c) { return c.toLowerCase().indexOf(typed) === 0; });
    if (!hits.length) hits = all.filter(function (c) { return c.toLowerCase().indexOf(typed) >= 0; });
    if (!menu) { menu = document.createElement("div"); menu.className = "slash-menu"; f.insertBefore(menu, f.firstChild); }
    menu.innerHTML = "";
    hits.slice(0, 40).forEach(function (c, i) {
      var b = document.createElement("button");
      b.type = "button"; b.className = "btn" + (i === 0 ? " sel" : ""); b.setAttribute("data-act", "slash"); b.setAttribute("data-value", c);
      b.textContent = "/" + c;
      menu.appendChild(b);
    });
    if (!hits.length) menu.innerHTML = '<span class="note">no command starts with /' + typed.replace(/[<&]/g, "") + "</span>";
  }
  // The pinned composer's line grows with what is typed (up to its max-height).
  function grow(t) { t.style.height = "auto"; t.style.height = Math.min(t.scrollHeight, 200) + "px"; }
  document.addEventListener("input", function (ev) {
    var t = ev.target;
    if (t.tagName === "TEXTAREA" && t.closest && t.closest("form[data-composer]")) { slashMenu(t); grow(t); }
  });

  document.addEventListener("keydown", function (ev) {
    var tt = ev.target, fm = tt.closest && tt.closest("form[data-composer]"), sm = fm && fm.querySelector(".slash-menu .btn.sel");
    if (sm && (ev.key === "Tab" || (ev.key === "Enter" && !ev.shiftKey))) { ev.preventDefault(); return act(sm); }
    if (ev.key !== "Enter" || ev.shiftKey || ev.isComposing) return;
    var t = ev.target;
    var f = t.closest && t.closest("form[data-composer]");
    if (f && t.tagName === "TEXTAREA" && !t.disabled) { ev.preventDefault(); return send(f); }
    var tf = t.closest && t.closest("form[data-terminal]");
    if (tf && t.tagName === "INPUT") { ev.preventDefault(); return runLine(tf); }
    var bx = t.closest && t.closest(".ask");
    if (bx && t.tagName === "INPUT" && !t.disabled) {
      var b = bx.querySelector('[data-act="ask"]');
      if (b) { ev.preventDefault(); answer(b); }
    }
  });
  document.addEventListener("submit", function (ev) {
    var f = ev.target.closest && ev.target.closest("form[data-composer]");
    if (f) { ev.preventDefault(); send(f); }
  });

  // ---- the Diagnostics line: the window as the browser measures it ----
  function viewport() {
    var el = document.querySelector("[data-viewport]");
    if (!el) return;
    var w = innerWidth, cls = w < COMPACT ? "compact" : w < 800 ? "regular" : "wide";
    el.textContent = "viewport: " + w + "×" + innerHeight + " CSS px · screen class " + cls + " · device pixel ratio " + (window.devicePixelRatio || 1);
  }
  window.addEventListener("resize", viewport);
  document.addEventListener("DOMContentLoaded", function () {
    // No choice made yet: open where the screen has room (the head's
    // script did this before the first paint, if the width was known then).
    var saved = null;
    try { saved = localStorage.getItem("dry.nav"); } catch (e) {}
    if (saved === null && innerWidth >= COMPACT) root.classList.add("nav-open");
    viewport();
    liveState();
  });
})();
