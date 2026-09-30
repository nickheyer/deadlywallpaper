// Column-major 4x4 matrices and small vector helpers with glm's conventions, so the scene
// renderer's math reads like linux-wallpaperengine's.
(() => {
  'use strict';
  const G = typeof window !== 'undefined' ? window : globalThis;

  const M = {};

  M.identity = () => {
    const m = new Float32Array(16);
    m[0] = m[5] = m[10] = m[15] = 1;
    return m;
  };

  M.clone = (a) => new Float32Array(a);

  // a * b
  M.multiply = (a, b) => {
    const o = new Float32Array(16);
    for (let c = 0; c < 4; c++) {
      for (let r = 0; r < 4; r++) {
        o[c * 4 + r] = a[r] * b[c * 4] + a[4 + r] * b[c * 4 + 1] + a[8 + r] * b[c * 4 + 2] + a[12 + r] * b[c * 4 + 3];
      }
    }
    return o;
  };

  M.translation = (x, y, z) => {
    const m = M.identity();
    m[12] = x; m[13] = y; m[14] = z;
    return m;
  };

  M.scaling = (x, y, z) => {
    const m = M.identity();
    m[0] = x; m[5] = y; m[10] = z;
    return m;
  };

  // Rotation by `angle` radians around `axis` (unit or not).
  M.rotation = (angle, ax, ay, az) => {
    const len = Math.hypot(ax, ay, az) || 1;
    const x = ax / len, y = ay / len, z = az / len;
    const c = Math.cos(angle), s = Math.sin(angle), t = 1 - c;
    const m = M.identity();
    m[0] = c + x * x * t; m[1] = y * x * t + z * s; m[2] = z * x * t - y * s;
    m[4] = x * y * t - z * s; m[5] = c + y * y * t; m[6] = z * y * t + x * s;
    m[8] = x * z * t + y * s; m[9] = y * z * t - x * s; m[10] = c + z * z * t;
    return m;
  };

  // glm::translate(m, v) == m * T(v)
  M.translate = (m, x, y, z) => M.multiply(m, M.translation(x, y, z));
  // glm::scale(m, v) == m * S(v)
  M.scale = (m, x, y, z) => M.multiply(m, M.scaling(x, y, z));
  // glm::rotate(m, angle, axis) == m * R
  M.rotate = (m, angle, ax, ay, az) => M.multiply(m, M.rotation(angle, ax, ay, az));

  M.ortho = (left, right, bottom, top, near, far) => {
    const m = M.identity();
    m[0] = 2 / (right - left);
    m[5] = 2 / (top - bottom);
    m[10] = -2 / (far - near);
    m[12] = -(right + left) / (right - left);
    m[13] = -(top + bottom) / (top - bottom);
    m[14] = -(far + near) / (far - near);
    return m;
  };

  M.perspective = (fovy, aspect, near, far) => {
    const f = 1 / Math.tan(fovy / 2);
    const m = new Float32Array(16);
    m[0] = f / aspect;
    m[5] = f;
    m[10] = (far + near) / (near - far);
    m[11] = -1;
    m[14] = (2 * far * near) / (near - far);
    return m;
  };

  M.lookAt = (eye, center, up) => {
    const fx = center[0] - eye[0], fy = center[1] - eye[1], fz = center[2] - eye[2];
    const fl = Math.hypot(fx, fy, fz) || 1;
    const f = [fx / fl, fy / fl, fz / fl];
    let sx = f[1] * up[2] - f[2] * up[1], sy = f[2] * up[0] - f[0] * up[2], sz = f[0] * up[1] - f[1] * up[0];
    const sl = Math.hypot(sx, sy, sz) || 1;
    sx /= sl; sy /= sl; sz /= sl;
    const ux = sy * f[2] - sz * f[1], uy = sz * f[0] - sx * f[2], uz = sx * f[1] - sy * f[0];
    const m = M.identity();
    m[0] = sx; m[4] = sy; m[8] = sz;
    m[1] = ux; m[5] = uy; m[9] = uz;
    m[2] = -f[0]; m[6] = -f[1]; m[10] = -f[2];
    m[12] = -(sx * eye[0] + sy * eye[1] + sz * eye[2]);
    m[13] = -(ux * eye[0] + uy * eye[1] + uz * eye[2]);
    m[14] = f[0] * eye[0] + f[1] * eye[1] + f[2] * eye[2];
    return m;
  };

  M.inverse = (a) => {
    const o = new Float32Array(16);
    const a00 = a[0], a01 = a[1], a02 = a[2], a03 = a[3];
    const a10 = a[4], a11 = a[5], a12 = a[6], a13 = a[7];
    const a20 = a[8], a21 = a[9], a22 = a[10], a23 = a[11];
    const a30 = a[12], a31 = a[13], a32 = a[14], a33 = a[15];
    const b00 = a00 * a11 - a01 * a10, b01 = a00 * a12 - a02 * a10, b02 = a00 * a13 - a03 * a10;
    const b03 = a01 * a12 - a02 * a11, b04 = a01 * a13 - a03 * a11, b05 = a02 * a13 - a03 * a12;
    const b06 = a20 * a31 - a21 * a30, b07 = a20 * a32 - a22 * a30, b08 = a20 * a33 - a23 * a30;
    const b09 = a21 * a32 - a22 * a31, b10 = a21 * a33 - a23 * a31, b11 = a22 * a33 - a23 * a32;
    let det = b00 * b11 - b01 * b10 + b02 * b09 + b03 * b08 - b04 * b07 + b05 * b06;
    if (!det) return M.identity();
    det = 1 / det;
    o[0] = (a11 * b11 - a12 * b10 + a13 * b09) * det;
    o[1] = (a02 * b10 - a01 * b11 - a03 * b09) * det;
    o[2] = (a31 * b05 - a32 * b04 + a33 * b03) * det;
    o[3] = (a22 * b04 - a21 * b05 - a23 * b03) * det;
    o[4] = (a12 * b08 - a10 * b11 - a13 * b07) * det;
    o[5] = (a00 * b11 - a02 * b08 + a03 * b07) * det;
    o[6] = (a32 * b02 - a30 * b05 - a33 * b01) * det;
    o[7] = (a20 * b05 - a22 * b02 + a23 * b01) * det;
    o[8] = (a10 * b10 - a11 * b08 + a13 * b06) * det;
    o[9] = (a01 * b08 - a00 * b10 - a03 * b06) * det;
    o[10] = (a30 * b04 - a31 * b02 + a33 * b00) * det;
    o[11] = (a21 * b02 - a20 * b04 - a23 * b00) * det;
    o[12] = (a11 * b07 - a10 * b09 - a12 * b06) * det;
    o[13] = (a00 * b09 - a01 * b07 + a02 * b06) * det;
    o[14] = (a31 * b01 - a30 * b03 - a32 * b00) * det;
    o[15] = (a20 * b03 - a21 * b01 + a22 * b00) * det;
    return o;
  };

  M.transpose = (a) => {
    const o = new Float32Array(16);
    for (let c = 0; c < 4; c++) for (let r = 0; r < 4; r++) o[c * 4 + r] = a[r * 4 + c];
    return o;
  };

  M.mat3 = (a) => new Float32Array([a[0], a[1], a[2], a[4], a[5], a[6], a[8], a[9], a[10]]);

  M.transformPoint = (m, v) => {
    const x = v[0], y = v[1], z = v[2] || 0;
    const w = m[3] * x + m[7] * y + m[11] * z + m[15] || 1;
    return [
      (m[0] * x + m[4] * y + m[8] * z + m[12]) / w,
      (m[1] * x + m[5] * y + m[9] * z + m[13]) / w,
      (m[2] * x + m[6] * y + m[10] * z + m[14]) / w,
    ];
  };

  M.transformDirection = (m, v) => {
    const x = v[0], y = v[1], z = v[2] || 0;
    return [m[0] * x + m[4] * y + m[8] * z, m[1] * x + m[5] * y + m[9] * z, m[2] * x + m[6] * y + m[10] * z];
  };

  // Row-major "row vector" matrix as Wallpaper Engine's puppet files store them, to glm layout.
  M.fromRowMajor = (rows) => {
    const m = new Float32Array(16);
    for (let r = 0; r < 4; r++) for (let c = 0; c < 4; c++) m[c * 4 + r] = rows[r * 4 + c];
    return m;
  };

  const V = {};
  V.add = (a, b) => a.map((x, i) => x + (b[i] || 0));
  V.sub = (a, b) => a.map((x, i) => x - (b[i] || 0));
  V.mul = (a, b) => a.map((x, i) => x * b[i]);
  V.scale = (a, s) => a.map((x) => x * s);
  V.dot = (a, b) => a.reduce((sum, x, i) => sum + x * b[i], 0);
  V.length = (a) => Math.sqrt(V.dot(a, a));
  V.normalize = (a) => { const l = V.length(a); return l > 0 ? V.scale(a, 1 / l) : a.map(() => 0); };
  V.cross = (a, b) => [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]];
  V.mix = (a, b, t) => a.map((x, i) => x + (b[i] - x) * t);
  V.clamp = (x, lo, hi) => Math.min(hi, Math.max(lo, x));
  V.smoothstep = (e0, e1, x) => { const t = V.clamp((x - e0) / (e1 - e0 || 1), 0, 1); return t * t * (3 - 2 * t); };

  G.WEM = M;
  G.WEV = V;
  if (typeof module !== 'undefined' && module.exports) module.exports = { WEM: M, WEV: V };
})();
