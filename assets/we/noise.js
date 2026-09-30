// Perlin and curl noise as linux-wallpaperengine's particle operators use them.
(() => {
  'use strict';
  const G = typeof window !== 'undefined' ? window : globalThis;

  const P = [151, 160, 137, 91, 90, 15, 131, 13, 201, 95, 96, 53, 194, 233, 7, 225, 140, 36, 103, 30, 69, 142, 8, 99, 37,
    240, 21, 10, 23, 190, 6, 148, 247, 120, 234, 75, 0, 26, 197, 62, 94, 252, 219, 203, 117, 35, 11, 32, 57, 177,
    33, 88, 237, 149, 56, 87, 174, 20, 125, 136, 171, 168, 68, 175, 74, 165, 71, 134, 139, 48, 27, 166, 77, 146,
    158, 231, 83, 111, 229, 122, 60, 211, 133, 230, 220, 105, 92, 41, 55, 46, 245, 40, 244, 102, 143, 54, 65, 25,
    63, 161, 1, 216, 80, 73, 209, 76, 132, 187, 208, 89, 18, 169, 200, 196, 135, 130, 116, 188, 159, 86, 164, 100,
    109, 198, 173, 186, 3, 64, 52, 217, 226, 250, 124, 123, 5, 202, 38, 147, 118, 126, 255, 82, 85, 212, 207, 206,
    59, 227, 47, 16, 58, 17, 182, 189, 28, 42, 223, 183, 170, 213, 119, 248, 152, 2, 44, 154, 163, 70, 221, 153,
    101, 155, 167, 43, 172, 9, 129, 22, 39, 253, 19, 98, 108, 110, 79, 113, 224, 232, 178, 185, 112, 104, 218, 246,
    97, 228, 251, 34, 242, 193, 238, 210, 144, 12, 191, 179, 162, 241, 81, 51, 145, 235, 249, 14, 239, 107, 49, 192,
    214, 31, 181, 199, 106, 157, 184, 84, 204, 176, 115, 121, 50, 45, 127, 4, 150, 254, 138, 236, 205, 93, 222, 114,
    67, 29, 24, 72, 243, 141, 128, 195, 78, 66, 215, 61, 156, 180];
  const PERM = new Uint8Array(512);
  for (let i = 0; i < 512; i++) PERM[i] = P[i & 255];

  function grad(hash, x, y, z) {
    switch (hash & 0xf) {
      case 0x0: return x + y; case 0x1: return -x + y; case 0x2: return x - y; case 0x3: return -x - y;
      case 0x4: return x + z; case 0x5: return -x + z; case 0x6: return x - z; case 0x7: return -x - z;
      case 0x8: return y + z; case 0x9: return -y + z; case 0xa: return y - z; case 0xb: return -y - z;
      case 0xc: return y + x; case 0xd: return -y + z; case 0xe: return y - x; default: return -y - z;
    }
  }
  const ease = (t) => t * t * t * (t * (t * 6 - 15) + 10);
  const lerp = (t, a, b) => a + t * (b - a);

  function perlin(x, y, z) {
    const X = Math.floor(x) & 255, Y = Math.floor(y) & 255, Z = Math.floor(z) & 255;
    x -= Math.floor(x); y -= Math.floor(y); z -= Math.floor(z);
    const u = ease(x), v = ease(y), w = ease(z);
    const A = PERM[X] + Y, AA = PERM[A] + Z, AB = PERM[A + 1] + Z;
    const B = PERM[X + 1] + Y, BA = PERM[B] + Z, BB = PERM[B + 1] + Z;
    return lerp(w,
      lerp(v, lerp(u, grad(PERM[AA], x, y, z), grad(PERM[BA], x - 1, y, z)), lerp(u, grad(PERM[AB], x, y - 1, z), grad(PERM[BB], x - 1, y - 1, z))),
      lerp(v, lerp(u, grad(PERM[AA + 1], x, y, z - 1), grad(PERM[BA + 1], x - 1, y, z - 1)), lerp(u, grad(PERM[AB + 1], x, y - 1, z - 1), grad(PERM[BB + 1], x - 1, y - 1, z - 1))));
  }

  function perlin3(p) {
    return [perlin(p[0], p[1], p[2]), perlin(p[0] + 89.2, p[1] + 33.1, p[2] + 57.3), perlin(p[0] + 100.3, p[1] + 120.1, p[2] + 142.2)];
  }

  function curl(p) {
    const e = 1e-4;
    const x0 = perlin3([p[0] - e, p[1], p[2]]), x1 = perlin3([p[0] + e, p[1], p[2]]);
    const y0 = perlin3([p[0], p[1] - e, p[2]]), y1 = perlin3([p[0], p[1] + e, p[2]]);
    const z0 = perlin3([p[0], p[1], p[2] - e]), z1 = perlin3([p[0], p[1], p[2] + e]);
    const x = (y1[2] - y0[2]) - (z1[1] - z0[1]);
    const y = (z1[0] - z0[0]) - (x1[2] - x0[2]);
    const z = (x1[1] - x0[1]) - (y1[0] - y0[0]);
    return [x / (2 * e), y / (2 * e), z / (2 * e)];
  }

  const api = { perlin, perlin3, curl };
  G.WENoise = api;
  if (typeof module !== 'undefined' && module.exports) module.exports = api;
})();
