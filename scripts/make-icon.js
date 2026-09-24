// Draws build/icon.png and src/icon.png (512x512, macOS needs at least 512): two quota rings on a dark rounded square.
// Pure Node (zlib only) so the icon can be regenerated without image tools.
const fs = require('node:fs');
const path = require('node:path');
const zlib = require('node:zlib');

const S = 512;
const K = S / 256; // shapes below are laid out on a 256 grid
const px = Buffer.alloc(S * S * 4);

function blend(x, y, [r, g, b], a) {
  if (a <= 0) return;
  const i = (y * S + x) * 4;
  const da = px[i + 3] / 255;
  const oa = a + da * (1 - a);
  px[i] = Math.round((r * a + px[i] * da * (1 - a)) / oa);
  px[i + 1] = Math.round((g * a + px[i + 1] * da * (1 - a)) / oa);
  px[i + 2] = Math.round((b * a + px[i + 2] * da * (1 - a)) / oa);
  px[i + 3] = Math.round(oa * 255);
}

// Coverage with 1px anti-aliasing from a signed distance.
const cover = (d) => Math.max(0, Math.min(1, 0.5 - d));

function roundedSquare(color, inset, radius) {
  for (let y = 0; y < S; y++) for (let x = 0; x < S; x++) {
    const qx = Math.abs(x + 0.5 - S / 2) - (S / 2 - inset - radius);
    const qy = Math.abs(y + 0.5 - S / 2) - (S / 2 - inset - radius);
    const d = Math.hypot(Math.max(qx, 0), Math.max(qy, 0)) + Math.min(Math.max(qx, qy), 0) - radius;
    blend(x, y, color, cover(d));
  }
}

// Arc of a ring starting at 12 o'clock, clockwise, `frac` of a full turn.
function ring(cx, cy, r, width, color, frac) {
  for (let y = 0; y < S; y++) for (let x = 0; x < S; x++) {
    const dx = x + 0.5 - cx;
    const dy = y + 0.5 - cy;
    const d = Math.abs(Math.hypot(dx, dy) - r) - width / 2;
    let ang = Math.atan2(dx, -dy) / (2 * Math.PI);
    if (ang < 0) ang += 1;
    if (ang <= frac) blend(x, y, color, cover(d));
  }
  // Round caps.
  for (const t of [0, frac]) {
    const a = t * 2 * Math.PI;
    const ex = cx + r * Math.sin(a);
    const ey = cy - r * Math.cos(a);
    for (let y = 0; y < S; y++) for (let x = 0; x < S; x++) {
      blend(x, y, color, cover(Math.hypot(x + 0.5 - ex, y + 0.5 - ey) - width / 2) * 1);
    }
  }
}

roundedSquare([22, 24, 30], 8 * K, 52 * K);
ring(128 * K, 128 * K, 84 * K, 26 * K, [255, 255, 255].map((v) => v * 0.13 + 24), 1);
ring(128 * K, 128 * K, 84 * K, 26 * K, [217, 119, 87], 0.68);
ring(128 * K, 128 * K, 46 * K, 22 * K, [255, 255, 255].map((v) => v * 0.13 + 24), 1);
ring(128 * K, 128 * K, 46 * K, 22 * K, [16, 163, 127], 0.4);

function png(width, height, rgba) {
  const raw = Buffer.alloc((width * 4 + 1) * height);
  for (let y = 0; y < height; y++) rgba.copy(raw, y * (width * 4 + 1) + 1, y * width * 4, (y + 1) * width * 4);
  const crcTable = Array.from({ length: 256 }, (_, n) => {
    let c = n;
    for (let k = 0; k < 8; k++) c = c & 1 ? 0xedb88320 ^ (c >>> 1) : c >>> 1;
    return c >>> 0;
  });
  const crc = (buf) => {
    let c = 0xffffffff;
    for (const b of buf) c = crcTable[(c ^ b) & 0xff] ^ (c >>> 8);
    return (c ^ 0xffffffff) >>> 0;
  };
  const chunk = (type, data) => {
    const len = Buffer.alloc(4); len.writeUInt32BE(data.length);
    const td = Buffer.concat([Buffer.from(type), data]);
    const c = Buffer.alloc(4); c.writeUInt32BE(crc(td));
    return Buffer.concat([len, td, c]);
  };
  const ihdr = Buffer.alloc(13);
  ihdr.writeUInt32BE(width, 0); ihdr.writeUInt32BE(height, 4);
  ihdr[8] = 8; ihdr[9] = 6;
  return Buffer.concat([
    Buffer.from([0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a]),
    chunk('IHDR', ihdr), chunk('IDAT', zlib.deflateSync(raw)), chunk('IEND', Buffer.alloc(0)),
  ]);
}

// build/ feeds the installer; src/ copy is the tray icon inside the app.
for (const out of [path.join(__dirname, '..', 'build', 'icon.png'), path.join(__dirname, '..', 'src', 'icon.png')]) {
  fs.mkdirSync(path.dirname(out), { recursive: true });
  fs.writeFileSync(out, png(S, S, px));
  console.log('wrote', out);
}
