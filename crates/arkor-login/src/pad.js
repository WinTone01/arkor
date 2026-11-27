// Console mode in the login and mod windows: the gamepad moves a pointer over the page (the
// window moves it and clicks with it; here it is only drawn), and a text field that is
// clicked opens an on-screen keyboard, whose keys the gamepad's buttons then press (they
// come from the window as `nav` calls while it is open).
(function () {
  if (window.__arkor) return;
  const L = __LABELS__;
  const A = (window.__arkor = {});
  const LETTERS = ["1234567890", "qwertyuiop", "asdfghjkl@", "zxcvbnm.-_"];
  const SYMBOLS = ["1234567890", "!#$%&*+=?/", "()[]{}<>:;", "'\",~^|\\`€"];
  let current = null, kbd = null, kx = 0, ky = 1, shift = false, sym = false, ptr = false;

  const css = `
    .__arkor-focus { outline: 4px solid #a78bfa !important; outline-offset: 3px !important;
      border-radius: 6px; box-shadow: 0 0 0 8px rgba(139,92,246,.25) !important; }
    #__arkor-kbd { position: fixed; left: 50%; bottom: 72px; transform: translateX(-50%);
      z-index: 2147483647; background: #0c0b12; border: 1px solid #ffffff22; border-radius: 22px;
      padding: 18px; display: flex; flex-direction: column; gap: 8px; font: 500 20px system-ui, sans-serif;
      box-shadow: 0 30px 80px #000a; }
    #__arkor-kbd .row { display: flex; gap: 8px; justify-content: center; }
    #__arkor-kbd .key { min-width: 54px; height: 54px; border-radius: 12px; display: flex; align-items: center;
      justify-content: center; color: #f4f4f5; background: #ffffff14; padding: 0 12px; }
    #__arkor-kbd .key.on { background: #f4f4f5; color: #0c0b12; }
    #__arkor-hints { position: fixed; left: 0; right: 0; bottom: 0; height: 56px; z-index: 2147483646;
      display: flex; gap: 28px; align-items: center; justify-content: flex-end; padding: 0 40px;
      background: linear-gradient(transparent, #0c0b12ee 45%); color: #d4d4d8; font: 500 16px system-ui, sans-serif; }
    #__arkor-hints b { border: 1px solid #ffffff55; border-radius: 8px; padding: 2px 8px; margin-right: 8px; }
    #__arkor-cursor { position: fixed; left: 0; top: 0; width: 22px; height: 30px; z-index: 2147483647;
      pointer-events: none; display: none; filter: drop-shadow(0 2px 3px #0008); transform-origin: 0 0; }`;
  const ARROW = `<svg xmlns="http://www.w3.org/2000/svg" width="22" height="30" viewBox="0 0 22 30">
    <path d="M1 1 L1 26 L7 20.5 L11 29.5 L15 27.7 L11.2 19 L19 18.5 Z" fill="#fff" stroke="#6d28d9"
      stroke-width="1.8" stroke-linejoin="round"/></svg>`;

  function ready(f) {
    if (document.readyState === "loading") document.addEventListener("DOMContentLoaded", f);
    else f();
  }
  ready(() => {
    const s = document.createElement("style");
    s.textContent = css;
    document.documentElement.appendChild(s);
    const h = document.createElement("div");
    h.id = "__arkor-hints";
    document.body.appendChild(h);
    hints();
    // A text field clicked with the pointer: the keyboard comes up for it.
    document.addEventListener("mousedown", (e) => {
      const t = e.target && e.target.closest ? e.target.closest("input,textarea") : null;
      if (isText(t)) setTimeout(() => { current = t; openKbd(); }, 0);
    }, true);
  });

  function hints() {
    const h = document.getElementById("__arkor-hints");
    if (!h) return;
    const list = kbd
      ? [["A", L.choose], ["B", L.done], ["X", "⌫"], ["Y", L.space]]
      : ptr
        ? [["A", L.click], ["B", L.back], ["☰", L.close]]
        : [["A", L.choose], ["B", L.back], ["X", L.pointer], ["☰", L.close]];
    h.innerHTML = list.map(([k, t]) => `<span><b>${k}</b>${t}</span>`).join("");
  }
  // The window moves the pointer; here only the hints change.
  A.pointer = function (on) {
    ptr = on;
    if (on) mark(null);
    hints();
  };
  // Draws the pointer with its tip at x, y (smaller while A is held), or hides it.
  A.cursor = function (x, y, down, on) {
    let c = document.getElementById("__arkor-cursor");
    if (!c) {
      if (!on || !document.body) return;
      c = document.createElement("div");
      c.id = "__arkor-cursor";
      c.innerHTML = ARROW;
      document.body.appendChild(c);
    }
    c.style.display = on ? "block" : "none";
    c.style.transform = `translate(${x}px, ${y}px)` + (down ? " scale(.9)" : "");
  };
  // Tells the window whether the keyboard has the gamepad.
  function tell(open) {
    try { window.ipc.postMessage(open ? "arkor:kbd:1" : "arkor:kbd:0"); } catch (_) {}
  }
  // Where the pointer starts: beside a frame (the user takes it to the box), else in the
  // middle of what has the focus.
  function pointerAt() {
    if (!current || !document.contains(current)) return { x: innerWidth / 2, y: innerHeight / 2 };
    const r = current.getBoundingClientRect();
    if (current.tagName !== "IFRAME") return { x: r.left + r.width / 2, y: r.top + r.height / 2 };
    return { x: Math.max(20, r.left + Math.min(r.width / 2, 120)), y: Math.min(innerHeight - 90, r.bottom + 60) };
  }

  function visible(e) {
    const r = e.getBoundingClientRect();
    if (r.width < 3 || r.height < 3) return false;
    const st = getComputedStyle(e);
    return st.visibility !== "hidden" && st.display !== "none" && !e.disabled;
  }
  function focusables() {
    return [...document.querySelectorAll(
      'a[href],button,input:not([type=hidden]),select,textarea,[role=button],[role=checkbox],[role=link],[tabindex]:not([tabindex="-1"]),iframe'
    )].filter((e) => visible(e) && !e.closest("#__arkor-kbd,#__arkor-hints"));
  }
  function mark(e) {
    if (current) current.classList.remove("__arkor-focus");
    current = e || null;
    if (!current) return;
    current.classList.add("__arkor-focus");
    current.scrollIntoView({ block: "nearest", inline: "nearest", behavior: "smooth" });
    if (current.tagName !== "IFRAME") current.focus({ preventScroll: true });
  }
  function centre(e) {
    const r = e.getBoundingClientRect();
    return { x: r.left + r.width / 2, y: r.top + r.height / 2 };
  }
  // To the nearest field or button in that direction.
  function move(dx, dy) {
    const list = focusables();
    if (!current || !document.contains(current) || !visible(current)) {
      return mark(list.find((e) => isText(e)) || list[0]);
    }
    const c = centre(current);
    let best = null, bd = Infinity;
    for (const e of list) {
      if (e === current) continue;
      const p = centre(e);
      const vx = p.x - c.x, vy = p.y - c.y;
      const along = dx ? vx * dx : vy * dy;
      if (along <= 4) continue;
      const d = along + (dx ? Math.abs(vy) : Math.abs(vx)) * 2.5;
      if (d < bd) { bd = d; best = e; }
    }
    if (best) mark(best);
  }
  function isText(e) {
    if (!e) return false;
    if (e.tagName === "TEXTAREA") return true;
    if (e.tagName !== "INPUT") return false;
    return !["checkbox", "radio", "submit", "button", "image", "reset"].includes((e.type || "").toLowerCase());
  }

  // ---- The keyboard ----
  function layout() {
    const rows = (sym ? SYMBOLS : LETTERS).map((r) =>
      [...r].map((c) => ({ k: shift && !sym ? c.toUpperCase() : c, t: "char" }))
    );
    rows.push([
      { k: "⇧", t: "shift", w: 1.5 }, { k: sym ? "abc" : "?123", t: "sym", w: 1.5 },
      { k: L.space, t: "space", w: 4 }, { k: "⌫", t: "bksp", w: 1.5 }, { k: L.done, t: "done", w: 2 },
    ]);
    return rows;
  }
  function draw() {
    if (!kbd) return;
    const rows = layout();
    ky = Math.min(ky, rows.length - 1);
    kx = Math.min(kx, rows[ky].length - 1);
    kbd.innerHTML = "";
    rows.forEach((r, y) => {
      const row = document.createElement("div");
      row.className = "row";
      r.forEach((key, x) => {
        const d = document.createElement("div");
        d.className = "key" + (x === kx && y === ky ? " on" : "") + (key.t === "shift" && shift ? " on" : "");
        d.textContent = key.k;
        if (key.w) d.style.minWidth = 54 * key.w + 8 * (key.w - 1) + "px";
        row.appendChild(d);
      });
      kbd.appendChild(row);
    });
  }
  function openKbd() {
    if (kbd) return;
    kbd = document.createElement("div");
    kbd.id = "__arkor-kbd";
    // Never over the field being typed into.
    if (current && current.getBoundingClientRect().bottom > innerHeight - 440) {
      kbd.style.bottom = "auto";
      kbd.style.top = "32px";
    }
    document.body.appendChild(kbd);
    kx = 4; ky = 1;
    draw();
    hints();
    tell(true);
  }
  function closeKbd() {
    if (!kbd) return;
    kbd.remove();
    kbd = null;
    hints();
    tell(false);
  }
  function type(text) {
    if (current) current.focus({ preventScroll: true });
    document.execCommand("insertText", false, text);
  }
  function press() {
    const key = layout()[ky][kx];
    switch (key.t) {
      case "char": type(key.k); if (shift) { shift = false; } break;
      case "shift": shift = !shift; break;
      case "sym": sym = !sym; break;
      case "space": type(" "); break;
      case "bksp": document.execCommand("delete"); break;
      case "done": closeKbd(); if (!ptr) move(0, 1); return;
    }
    draw();
  }
  function kbdNav(n) {
    const rows = layout();
    switch (n) {
      case "left": kx = Math.max(0, kx - 1); break;
      case "right": kx = Math.min(rows[ky].length - 1, kx + 1); break;
      case "up":
      case "down": {
        const from = rows[ky].length;
        ky = Math.max(0, Math.min(rows.length - 1, ky + (n === "up" ? -1 : 1)));
        kx = Math.min(rows[ky].length - 1, Math.round((kx * rows[ky].length) / from));
        break;
      }
      case "accept": press(); return;
      case "extra": document.execCommand("delete"); return;
      case "search": type(" "); return;
      case "back": closeKbd(); return;
    }
    draw();
  }

  A.nav = function (n) {
    if (kbd) return kbdNav(n);
    switch (n) {
      case "up": return move(0, -1);
      case "down": return move(0, 1);
      case "left": return move(-1, 0);
      case "right": return move(1, 0);
      case "accept":
        if (!current) return move(0, 1);
        if (isText(current)) return openKbd();
        // What is in a frame of another site (a "not a robot" box) is out of the page's
        // reach: the user clicks in it with the pointer.
        if (current.tagName === "IFRAME") return pointerAt();
        current.click();
        return;
      case "extra": return pointerAt();
      case "back": history.back(); return;
    }
  };
})();
