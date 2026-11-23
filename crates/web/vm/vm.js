// The browser VM's JavaScript side (docs/container.md): v86 (an x86 PC
// emulator compiled to WebAssembly) running Alpine Linux. This file only
// moves bytes; what they mean (exec markers, HTTP to a guest port) is
// dry_core::container, called from crates/web/src/vm.rs.
//
//   dryVm.boot(opts, onBytes)   load v86, boot the kernel + initramfs; onBytes(Uint8Array) for serial output
//   dryVm.send(text)            type into the serial console
//   dryVm.tcp(port, bytes, ms)  open a TCP connection to the guest, send, read until it closes
//   dryVm.save() / restore()    the whole machine (memory, disk in RAM) to / from IndexedDB
//   dryVm.forget()              drop the saved machine
//   dryVm.meta()                the saved machine's size and time ({stored_bytes, saved}), or null
//   dryVm.stats()               sizes and the JS heap, for docs/container.md
//   dryVm.stop()

// Pinned: v86 0.5.473 (2026-10-30, BSD-2-Clause) from npm; its BIOS from
// the same commit (315b2b0) on GitHub. Both through jsDelivr, which sends
// CORS and Cross-Origin-Resource-Policy: cross-origin.
const V86 = "https://cdn.jsdelivr.net/npm/v86@0.5.473/build/";
const BIOS = "https://cdn.jsdelivr.net/gh/copy/v86@315b2b0/bios/";
const DB = "dry-vm";

let emulator = null;
let onBytes = null;
let booted = null;
const loaded = {};

function idb() {
  return new Promise((ok, no) => {
    const r = indexedDB.open(DB, 1);
    r.onupgradeneeded = () => r.result.createObjectStore("machine");
    r.onsuccess = () => ok(r.result);
    r.onerror = () => no(r.error);
  });
}

async function idbDo(mode, f) {
  const db = await idb();
  return new Promise((ok, no) => {
    const tx = db.transaction("machine", mode);
    const req = f(tx.objectStore("machine"));
    tx.oncomplete = () => { db.close(); ok(req && req.result); };
    tx.onerror = () => { db.close(); no(tx.error); };
  });
}

async function sized(url) {
  const t = performance.now();
  const r = await fetch(url);
  if (!r.ok) throw new Error(`${url}: ${r.status}`);
  const b = await r.arrayBuffer();
  loaded[url.split("/").pop()] = { bytes: b.byteLength, ms: Math.round(performance.now() - t) };
  return b;
}

// opts: { base: "vm/" (kernel and initramfs beside the page), memoryMb, restore: bool }
async function boot(opts, bytes) {
  if (emulator) return { already: true };
  onBytes = bytes;
  const t0 = performance.now();
  const lib = await import(V86 + "libv86.mjs");
  const base = opts.base || "vm/";
  const [bios, vga, kernel, initrd] = await Promise.all([
    sized(BIOS + "seabios.bin"), sized(BIOS + "vgabios.bin"),
    sized(base + "vmlinuz-virt"), sized(base + "alpine-dry.cpio.gz"),
  ]);
  const t1 = performance.now();
  const gz = opts.restore ? await idbDo("readonly", (s) => s.get("state")) : null;
  const saved = gz ? await pipe(gz, new DecompressionStream("gzip")) : null;
  emulator = new lib.V86({
    wasm_path: V86 + "v86.wasm",
    memory_size: (opts.memoryMb || 256) * 1024 * 1024,
    vga_memory_size: 2 * 1024 * 1024,
    bios: { buffer: bios },
    vga_bios: { buffer: vga },
    bzimage: { buffer: kernel },
    initrd: { buffer: initrd },
    cmdline: "console=ttyS0,115200 rdinit=/init quiet loglevel=3 tsc=reliable mitigations=off random.trust_cpu=on",
    filesystem: {},
    net_device: { type: "virtio", relay_url: "fetch" },
    initial_state: saved ? { buffer: saved } : undefined,
    // Else a restored guest keeps its old MAC and the network's new one
    // does not match: ports stop answering.
    preserve_mac_from_state_image: true,
    autostart: true,
    disable_keyboard: true,
    disable_mouse: true,
    disable_speaker: true,
  });
  emulator.add_listener("serial0-output-byte", (b) => onBytes && onBytes(new Uint8Array([b])));
  // A restored machine loads after the constructor returns: what is typed
  // before it runs is lost, so wait for it to start.
  if (!emulator.is_running()) await new Promise((ok) => emulator.add_listener("emulator-started", ok));
  booted = { download_ms: Math.round(t1 - t0), restore_ms: Math.round(performance.now() - t1), restored: !!saved, saved_bytes: gz ? gz.byteLength : 0 };
  return booted;
}

function send(text) {
  if (emulator) emulator.serial0_send(text);
}

// One TCP connection to the guest through v86's fake network (the fetch
// backend's own stack): connect, write, read until the guest closes or
// `ms` passes.
function tcp(port, bytes, ms) {
  return new Promise((ok, no) => {
    if (!emulator) return no(new Error("no VM"));
    const conn = emulator.network_adapter.connect(port);
    const parts = [];
    let done = false;
    const end = (why) => {
      if (done) return;
      done = true;
      const n = parts.reduce((a, p) => a + p.length, 0);
      const all = new Uint8Array(n);
      let o = 0;
      for (const p of parts) { all.set(p, o); o += p.length; }
      if (why && n === 0) no(new Error(why)); else ok(all);
    };
    const timer = setTimeout(() => { end(`port ${port}: no answer in ${ms} ms`); try { conn.close(); } catch (e) {} }, ms || 10000);
    conn.on("connect", () => conn.write(bytes));
    conn.on("data", (d) => parts.push(new Uint8Array(d)));
    conn.on("close", () => { clearTimeout(timer); end(); });
    conn.on("shutdown", () => { clearTimeout(timer); end(); try { conn.close(); } catch (e) {} });
  });
}

// The machine state is mostly guest RAM: gzip it (CompressionStream)
// before it goes to IndexedDB.
const pipe = (buf, t) => new Response(new Blob([buf]).stream().pipeThrough(t)).arrayBuffer();

async function save() {
  if (!emulator) throw new Error("no VM");
  const t = performance.now();
  const state = await emulator.save_state();
  const gz = await pipe(state, new CompressionStream("gzip"));
  const meta = { bytes: state.byteLength, stored_bytes: gz.byteLength, saved: Math.round(Date.now() / 1000) };
  await idbDo("readwrite", (s) => { s.put(gz, "state"); return s.put(meta, "meta"); });
  return { ...meta, ms: Math.round(performance.now() - t) };
}

async function forget() {
  await idbDo("readwrite", (s) => { s.delete("state"); return s.delete("meta"); });
}

async function meta() {
  const m = await idbDo("readonly", (s) => s.get("meta"));
  return m === undefined ? null : m;
}

async function hasSaved() {
  const k = await idbDo("readonly", (s) => s.getKey("state"));
  return k !== undefined;
}

function stats() {
  const m = performance.memory;
  return {
    loaded,
    booted,
    js_heap_mb: m ? Math.round(m.usedJSHeapSize / 1048576) : null,
    cross_origin_isolated: self.crossOriginIsolated === true,
    shared_array_buffer: typeof SharedArrayBuffer !== "undefined",
  };
}

async function stop() {
  if (emulator) { await emulator.destroy(); emulator = null; }
}

window.dryVm = { boot, send, tcp, save, forget, hasSaved, meta, stats, stop };
