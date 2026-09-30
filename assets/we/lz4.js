// LZ4 raw block decoding, the compression Wallpaper Engine applies to texture mipmaps.
(() => {
  'use strict';
  const G = typeof window !== 'undefined' ? window : globalThis;

  // Decode one LZ4 block of `src` into exactly `dstSize` bytes.
  function decodeBlock(src, dstSize) {
    const dst = new Uint8Array(dstSize);
    let ip = 0, op = 0;
    const n = src.length;
    while (ip < n) {
      const token = src[ip++];
      let literal = token >>> 4;
      if (literal === 15) {
        let s;
        do {
          if (ip >= n) throw new Error('lz4: truncated literal length');
          s = src[ip++];
          literal += s;
        } while (s === 255);
      }
      if (ip + literal > n || op + literal > dstSize) throw new Error('lz4: literal run out of bounds');
      dst.set(src.subarray(ip, ip + literal), op);
      ip += literal;
      op += literal;
      if (ip >= n) break;
      if (ip + 2 > n) throw new Error('lz4: truncated match offset');
      const offset = src[ip] | (src[ip + 1] << 8);
      ip += 2;
      if (offset === 0 || offset > op) throw new Error('lz4: invalid match offset ' + offset);
      let match = token & 15;
      if (match === 15) {
        let s;
        do {
          if (ip >= n) throw new Error('lz4: truncated match length');
          s = src[ip++];
          match += s;
        } while (s === 255);
      }
      match += 4;
      if (op + match > dstSize) throw new Error('lz4: match run out of bounds');
      if (offset >= match) {
        dst.copyWithin(op, op - offset, op - offset + match);
        op += match;
      } else {
        for (let i = 0; i < match; i++) { dst[op] = dst[op - offset]; op++; }
      }
    }
    if (op !== dstSize) throw new Error('lz4: decompressed ' + op + ' bytes, expected ' + dstSize);
    return dst;
  }

  G.WELZ4 = { decodeBlock };
  if (typeof module !== 'undefined' && module.exports) module.exports = G.WELZ4;
})();
