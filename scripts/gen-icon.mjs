// Generate the app icon source PNG (a database "cylinder" on a Bifrost blue->violet rounded
// square), with no dependencies. Renders at 2x and box-downsamples for anti-aliasing.
// Then run `npm run tauri icon scripts/app-icon.png` to produce every required size.
import { deflateSync } from "node:zlib";
import { writeFileSync } from "node:fs";

const S = 512; // output size
const SS = 2; // supersample factor
const H = S * SS; // render size

// ---- geometry (in output S space; scaled by SS at render time) ----
const cx = S / 2;
const rx = 132; // cylinder half-width
const ry = 40; // cap ellipse half-height
const topY = 168; // top cap center
const botY = 356; // bottom cap center
const bgRadius = 0.2 * S; // rounded-square corner radius

const lerp = (a, b, t) => a + (b - a) * t;
function bg(x, y) {
  // Diagonal blue -> violet.
  const t = (x + y) / (2 * S);
  return [
    Math.round(lerp(59, 124, t)), // 3b5bdb -> 7c3aed
    Math.round(lerp(91, 58, t)),
    Math.round(lerp(219, 237, t)),
  ];
}

function inRoundedRect(x, y) {
  const r = bgRadius;
  const dx = Math.max(r - x, x - (S - r), 0);
  const dy = Math.max(r - y, y - (S - r), 0);
  return dx * dx + dy * dy <= r * r;
}

const ell = (x, y, cyc, ex, ey) => ((x - cx) / ex) ** 2 + ((y - cyc) / ey) ** 2;

function inCylinder(x, y) {
  if (Math.abs(x - cx) <= rx && y >= topY && y <= botY) return true; // body
  if (ell(x, y, topY, rx, ry) <= 1) return true; // top cap
  if (y >= botY && ell(x, y, botY, rx, ry) <= 1) return true; // bottom front cap
  return false;
}

// A "ring" line: near the ellipse edge. `front` = only the lower (visible) arc.
function onRing(x, y, cyc, front) {
  if (Math.abs(x - cx) > rx + 1) return false;
  if (front && y < cyc) return false;
  const v = ell(x, y, cyc, rx, ry);
  return Math.abs(v - 1) < 0.09;
}

// Compose one output pixel by averaging SS*SS render samples.
function sample(px, py) {
  let R = 0,
    G = 0,
    B = 0,
    A = 0;
  for (let sy = 0; sy < SS; sy++) {
    for (let sx = 0; sx < SS; sx++) {
      const x = px + (sx + 0.5) / SS;
      const y = py + (sy + 0.5) / SS;
      let r = 0,
        g = 0,
        b = 0,
        a = 0;
      if (inRoundedRect(x, y)) {
        [r, g, b] = bg(x, y);
        a = 255;
      }
      if (inCylinder(x, y)) {
        // near-white fill; the top cap a touch brighter
        const top = ell(x, y, topY, rx, ry) <= 1 && y <= topY + ry;
        [r, g, b] = top ? [255, 255, 255] : [238, 242, 248];
        a = 255;
      }
      // ring separators (top rim + two disks) in translucent deep blue
      if (
        onRing(x, y, topY, false) ||
        onRing(x, y, lerp(topY, botY, 0.5), true) ||
        onRing(x, y, botY, true)
      ) {
        const k = 0.72;
        r = Math.round(lerp(r, 40, k));
        g = Math.round(lerp(g, 60, k));
        b = Math.round(lerp(b, 130, k));
      }
      R += r;
      G += g;
      B += b;
      A += a;
    }
  }
  const n = SS * SS;
  return [Math.round(R / n), Math.round(G / n), Math.round(B / n), Math.round(A / n)];
}

const raw = Buffer.alloc((S * 4 + 1) * S);
let o = 0;
for (let y = 0; y < S; y++) {
  raw[o++] = 0; // filter type
  for (let x = 0; x < S; x++) {
    const [r, g, b, a] = sample(x, y);
    raw[o++] = r;
    raw[o++] = g;
    raw[o++] = b;
    raw[o++] = a;
  }
}

function chunk(type, data) {
  const len = Buffer.alloc(4);
  len.writeUInt32BE(data.length);
  const td = Buffer.concat([Buffer.from(type), data]);
  const crc = Buffer.alloc(4);
  crc.writeUInt32BE(crc32(td) >>> 0);
  return Buffer.concat([len, td, crc]);
}
function crc32(buf) {
  let c = ~0;
  for (const b of buf) {
    c ^= b;
    for (let i = 0; i < 8; i++) c = (c >>> 1) ^ (0xedb88320 & -(c & 1));
  }
  return ~c;
}

const ihdr = Buffer.alloc(13);
ihdr.writeUInt32BE(S, 0);
ihdr.writeUInt32BE(S, 4);
ihdr[8] = 8; // bit depth
ihdr[9] = 6; // RGBA
const png = Buffer.concat([
  Buffer.from([137, 80, 78, 71, 13, 10, 26, 10]),
  chunk("IHDR", ihdr),
  chunk("IDAT", deflateSync(raw)),
  chunk("IEND", Buffer.alloc(0)),
]);

writeFileSync(new URL("./app-icon.png", import.meta.url), png);
console.log(`wrote scripts/app-icon.png (${S}x${S})`);
