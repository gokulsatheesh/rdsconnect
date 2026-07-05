// Generate a simple solid-gradient source PNG for the app icon (no deps).
// Then run `npm run tauri icon scripts/app-icon.png` to produce every required size.
import { deflateSync } from "node:zlib";
import { writeFileSync } from "node:fs";

const S = 512;
function px(x, y) {
  // Bifrost rainbow-ish diagonal: blue -> violet.
  const t = (x + y) / (2 * S);
  const r = Math.round(90 + t * 60);
  const g = Math.round(90 + (1 - t) * 40);
  const b = Math.round(200 + t * 40);
  return [r, g, b, 255];
}

const raw = Buffer.alloc((S * 4 + 1) * S);
let o = 0;
for (let y = 0; y < S; y++) {
  raw[o++] = 0; // filter type 0
  for (let x = 0; x < S; x++) {
    const [r, g, b, a] = px(x, y);
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
console.log("wrote scripts/app-icon.png (512x512)");
