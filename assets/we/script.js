// SceneScript: Wallpaper Engine's property-bound scripting, run in the page. Vec/Mat classes,
// the WEMath/WEColor/WEVector modules, the engine/input/thisScene/thisLayer globals and the
// event hooks scripts export.
(() => {
  'use strict';
  const G = typeof window !== 'undefined' ? window : globalThis;
  const M = G.WEM;

  const DEG = Math.PI / 180, RAD = 180 / Math.PI;
  const EPS = 1e-5;

  function components(v, n) {
    if (typeof v === 'number') return new Array(n).fill(v);
    if (typeof v === 'string') { const p = v.trim().split(/\s+/).map(Number); const out = []; for (let i = 0; i < n; i++) out.push(Number.isFinite(p[i]) ? p[i] : (p.length === 1 ? p[0] : 0)); return out; }
    if (v && typeof v === 'object') { const keys = ['x', 'y', 'z', 'w']; const out = []; for (let i = 0; i < n; i++) out.push(typeof v[keys[i]] === 'number' ? v[keys[i]] : 0); return out; }
    if (Array.isArray(v)) { const out = []; for (let i = 0; i < n; i++) out.push(Number(v[i]) || 0); return out; }
    return new Array(n).fill(0);
  }

  function defineVec(n) {
    const keys = ['x', 'y', 'z', 'w'].slice(0, n);
    class Vec {
      constructor(...args) {
        let vals;
        if (args.length === 0) vals = new Array(n).fill(0);
        else if (args.length === 1) vals = components(args[0], n);
        else { vals = args.slice(0, n).map((a) => Number(a) || 0); while (vals.length < n) vals.push(0); }
        for (let i = 0; i < n; i++) this[keys[i]] = vals[i];
      }
      static from(arr) { return new Vec(...arr); }
      toArray() { return keys.map((k) => this[k]); }
      map(fn, other) {
        const o = other === undefined ? null : components(other, n);
        return new Vec(...keys.map((k, i) => fn(this[k], o ? o[i] : undefined, i)));
      }
      length() { return Math.sqrt(this.lengthSqr()); }
      lengthSqr() { return keys.reduce((s, k) => s + this[k] * this[k], 0); }
      distance(o) { return this.subtract(o).length(); }
      distanceSqr(o) { return this.subtract(o).lengthSqr(); }
      normalize() { const l = this.length(); return l > 0 ? this.map((a) => a / l) : this.copy(); }
      copy() { return new Vec(...this.toArray()); }
      equals(o) { const c = components(o, n); return keys.every((k, i) => Math.abs(this[k] - c[i]) < EPS); }
      isFinite() { return keys.every((k) => Number.isFinite(this[k])); }
      negate() { return this.map((a) => -a); }
      add(o) { return this.map((a, b) => a + b, o); }
      subtract(o) { return this.map((a, b) => a - b, o); }
      multiply(o) { return this.map((a, b) => a * b, o); }
      divide(o) { return this.map((a, b) => a / b, o); }
      dot(o) { const c = components(o, n); return keys.reduce((s, k, i) => s + this[k] * c[i], 0); }
      reflect(normal) { const nn = new Vec(normal).normalize(); return this.subtract(nn.multiply(2 * this.dot(nn))); }
      project(o) { const v = new Vec(o); const d = v.lengthSqr(); return d > 0 ? v.multiply(this.dot(v) / d) : new Vec(); }
      mix(o, t) { return this.map((a, b, i) => a + (b - a) * (typeof t === 'number' ? t : components(t, n)[i]), o); }
      min(o) { return this.map((a, b) => Math.min(a, b), o); }
      max(o) { return this.map((a, b) => Math.max(a, b), o); }
      clamp(lo, hi) { const l = components(lo, n), h = components(hi, n); return this.map((a, b, i) => Math.min(h[i], Math.max(l[i], a))); }
      abs() { return this.map((a) => Math.abs(a)); }
      sign() { return this.map((a) => Math.sign(a)); }
      round() { return this.map((a) => Math.round(a)); }
      floor() { return this.map((a) => Math.floor(a)); }
      ceil() { return this.map((a) => Math.ceil(a)); }
      fract() { return this.map((a) => a - Math.floor(a)); }
      mod(o) { return this.map((a, b) => a - b * Math.floor(a / b), o); }
      step(edge) { return this.map((a, b) => (a < b ? 0 : 1), edge); }
      smoothStep(lo, hi) { const l = components(lo, n), h = components(hi, n); return this.map((a, b, i) => { const t = Math.min(1, Math.max(0, (a - l[i]) / ((h[i] - l[i]) || 1))); return t * t * (3 - 2 * t); }); }
      toString() { return keys.map((k) => String(this[k])).join(' '); }
    }
    if (n === 2) {
      Vec.prototype.perpendicular = function () { return new Vec(-this.y, this.x); };
      Vec.prototype.angle = function () { return Math.atan2(this.y, this.x) * RAD; };
      Vec.prototype.angleBetween = function (o) { const v = new Vec(o); return (Math.atan2(v.y, v.x) - Math.atan2(this.y, this.x)) * RAD; };
      Vec.prototype.rotate = function (deg) { const a = deg * DEG, c = Math.cos(a), s = Math.sin(a); return new Vec(this.x * c - this.y * s, this.x * s + this.y * c); };
    }
    if (n === 3) {
      Vec.prototype.cross = function (o) { const c = components(o, 3); return new Vec(this.y * c[2] - this.z * c[1], this.z * c[0] - this.x * c[2], this.x * c[1] - this.y * c[0]); };
      Vec.prototype.refract = function (normal, eta) {
        const nn = new Vec(normal).normalize(), i = this.normalize();
        const d = nn.dot(i), k = 1 - eta * eta * (1 - d * d);
        if (k < 0) return new Vec();
        return i.multiply(eta).subtract(nn.multiply(eta * d + Math.sqrt(k)));
      };
      Vec.prototype.angleBetween = function (o) { const v = new Vec(o); const d = this.dot(v) / ((this.length() * v.length()) || 1); return Math.acos(Math.min(1, Math.max(-1, d))) * RAD; };
      Vec.prototype.toSpherical = function () { const r = this.length(); if (r === 0) return new Vec(0, 0, 0); return new Vec(r, Math.acos(this.y / r) * RAD, Math.atan2(this.z, this.x) * RAD); };
      Vec.fromSpherical = (r, theta, phi) => { const t = theta * DEG, p = phi * DEG; return new Vec(r * Math.sin(t) * Math.cos(p), r * Math.cos(t), r * Math.sin(t) * Math.sin(p)); };
    }
    return Vec;
  }

  const Vec2 = defineVec(2), Vec3 = defineVec(3), Vec4 = defineVec(4);

  class Mat4 {
    constructor(m) { this.m = m ? Array.from(m) : Array.from(M.identity()); }
    static identity() { return new Mat4(); }
    static fromTranslation(v) { const c = components(v, 3); return new Mat4(M.translation(c[0], c[1], c[2])); }
    static fromScale(v) { const c = typeof v === 'number' ? [v, v, v] : components(v, 3); return new Mat4(M.scaling(c[0], c[1], c[2])); }
    static fromRotation(angle, axis) { const a = components(axis, 3); return new Mat4(M.rotation(angle * DEG, a[0], a[1], a[2])); }
    static fromEuler(x, y, z) { const e = typeof x === 'number' && y !== undefined ? [x, y, z] : components(x, 3); let m = M.rotation(e[2] * DEG, 0, 0, 1); m = M.rotate(m, e[1] * DEG, 0, 1, 0); m = M.rotate(m, e[0] * DEG, 1, 0, 0); return new Mat4(m); }
    static fromBasis(right, up, forward) { const r = components(right, 3), u = components(up, 3), f = components(forward, 3); const m = M.identity(); m[0] = r[0]; m[1] = r[1]; m[2] = r[2]; m[4] = u[0]; m[5] = u[1]; m[6] = u[2]; m[8] = f[0]; m[9] = f[1]; m[10] = f[2]; return new Mat4(m); }
    static lookAt(eye, center, up) { return new Mat4(M.lookAt(components(eye, 3), components(center, 3), components(up, 3))); }
    static compose(t, r, s) { return Mat4.fromTranslation(t).multiply(Mat4.fromEuler(r)).multiply(Mat4.fromScale(s)); }
    translation(p) { if (p !== undefined) { const c = components(p, 3); this.m[12] = c[0]; this.m[13] = c[1]; this.m[14] = c[2]; } return new Vec3(this.m[12], this.m[13], this.m[14]); }
    right() { return new Vec3(this.m[0], this.m[1], this.m[2]); }
    up() { return new Vec3(this.m[4], this.m[5], this.m[6]); }
    forward() { return new Vec3(this.m[8], this.m[9], this.m[10]); }
    add(o) { return new Mat4(this.m.map((a, i) => a + o.m[i])); }
    subtract(o) { return new Mat4(this.m.map((a, i) => a - o.m[i])); }
    multiply(v) {
      if (typeof v === 'number') return new Mat4(this.m.map((a) => a * v));
      if (v instanceof Mat4) return new Mat4(M.multiply(this.m, v.m));
      const c = components(v, 4);
      const m = this.m;
      return new Vec4(m[0] * c[0] + m[4] * c[1] + m[8] * c[2] + m[12] * c[3], m[1] * c[0] + m[5] * c[1] + m[9] * c[2] + m[13] * c[3], m[2] * c[0] + m[6] * c[1] + m[10] * c[2] + m[14] * c[3], m[3] * c[0] + m[7] * c[1] + m[11] * c[2] + m[15] * c[3]);
    }
    translate(v) { return this.multiply(Mat4.fromTranslation(v)); }
    rotate(angle, axis) { return this.multiply(Mat4.fromRotation(angle, axis)); }
    scale(v) { return this.multiply(Mat4.fromScale(v)); }
    transformPoint(v) { return new Vec3(...M.transformPoint(this.m, components(v, 3))); }
    transformDirection(v) { return new Vec3(...M.transformDirection(this.m, components(v, 3))); }
    transpose() { return new Mat4(M.transpose(this.m)); }
    inverse() { return new Mat4(M.inverse(this.m)); }
    determinant() {
      const a = this.m;
      const b00 = a[0] * a[5] - a[1] * a[4], b01 = a[0] * a[6] - a[2] * a[4], b02 = a[0] * a[7] - a[3] * a[4], b03 = a[1] * a[6] - a[2] * a[5], b04 = a[1] * a[7] - a[3] * a[5], b05 = a[2] * a[7] - a[3] * a[6];
      const b06 = a[8] * a[13] - a[9] * a[12], b07 = a[8] * a[14] - a[10] * a[12], b08 = a[8] * a[15] - a[11] * a[12], b09 = a[9] * a[14] - a[10] * a[13], b10 = a[9] * a[15] - a[11] * a[13], b11 = a[10] * a[15] - a[11] * a[14];
      return b00 * b11 - b01 * b10 + b02 * b09 + b03 * b08 - b04 * b07 + b05 * b06;
    }
    extractEuler() { return this.decompose().rotation; }
    normalMatrix() { return Mat3.fromMat4(this.inverse().transpose()); }
    decompose() {
      const m = this.m;
      const sx = Math.hypot(m[0], m[1], m[2]), sy = Math.hypot(m[4], m[5], m[6]), sz = Math.hypot(m[8], m[9], m[10]);
      const r = [m[0] / (sx || 1), m[1] / (sx || 1), m[2] / (sx || 1), m[4] / (sy || 1), m[5] / (sy || 1), m[6] / (sy || 1), m[8] / (sz || 1), m[9] / (sz || 1), m[10] / (sz || 1)];
      const y = Math.asin(Math.max(-1, Math.min(1, -r[2])));
      let x, z;
      if (Math.abs(r[2]) < 0.9999) { x = Math.atan2(r[5], r[8]); z = Math.atan2(r[1], r[0]); } else { x = Math.atan2(-r[7], r[4]); z = 0; }
      return { translation: new Vec3(m[12], m[13], m[14]), rotation: new Vec3(x * RAD, y * RAD, z * RAD), scale: new Vec3(sx, sy, sz) };
    }
    copy() { return new Mat4(this.m); }
    equals(o) { return this.m.every((a, i) => Math.abs(a - o.m[i]) < EPS); }
    toString() { return this.m.join(' '); }
  }

  class Mat3 {
    constructor(m) { this.m = m ? Array.from(m) : [1, 0, 0, 0, 1, 0, 0, 0, 1]; }
    static identity() { return new Mat3(); }
    static fromTranslation(v) { const c = components(v, 2); return new Mat3([1, 0, 0, 0, 1, 0, c[0], c[1], 1]); }
    static fromScale(v) { const c = typeof v === 'number' ? [v, v] : components(v, 2); return new Mat3([c[0], 0, 0, 0, c[1], 0, 0, 0, 1]); }
    static fromRotation(deg) { const a = deg * DEG, c = Math.cos(a), s = Math.sin(a); return new Mat3([c, s, 0, -s, c, 0, 0, 0, 1]); }
    static fromBasis(right, up) { const r = components(right, 2), u = components(up, 2); return new Mat3([r[0], r[1], 0, u[0], u[1], 0, 0, 0, 1]); }
    static fromMat4(m4) { const m = m4.m; return new Mat3([m[0], m[1], m[2], m[4], m[5], m[6], m[8], m[9], m[10]]); }
    static compose(t, r, s) { return Mat3.fromTranslation(t).multiply(Mat3.fromRotation(r)).multiply(Mat3.fromScale(s)); }
    translation(p) { if (p !== undefined) { const c = components(p, 2); this.m[6] = c[0]; this.m[7] = c[1]; } return new Vec2(this.m[6], this.m[7]); }
    angle() { return Math.atan2(this.m[1], this.m[0]) * RAD; }
    add(o) { return new Mat3(this.m.map((a, i) => a + o.m[i])); }
    subtract(o) { return new Mat3(this.m.map((a, i) => a - o.m[i])); }
    multiply(v) {
      if (typeof v === 'number') return new Mat3(this.m.map((a) => a * v));
      const a = this.m;
      if (v instanceof Mat3) {
        const b = v.m, o = new Array(9);
        for (let c = 0; c < 3; c++) for (let r = 0; r < 3; r++) o[c * 3 + r] = a[r] * b[c * 3] + a[3 + r] * b[c * 3 + 1] + a[6 + r] * b[c * 3 + 2];
        return new Mat3(o);
      }
      const c = components(v, 3);
      return new Vec3(a[0] * c[0] + a[3] * c[1] + a[6] * c[2], a[1] * c[0] + a[4] * c[1] + a[7] * c[2], a[2] * c[0] + a[5] * c[1] + a[8] * c[2]);
    }
    translate(v) { return this.multiply(Mat3.fromTranslation(v)); }
    rotate(deg) { return this.multiply(Mat3.fromRotation(deg)); }
    scale(v) { return this.multiply(Mat3.fromScale(v)); }
    transformPoint(v) { const c = components(v, 2), a = this.m; return new Vec2(a[0] * c[0] + a[3] * c[1] + a[6], a[1] * c[0] + a[4] * c[1] + a[7]); }
    transformDirection(v) { const c = components(v, 2), a = this.m; return new Vec2(a[0] * c[0] + a[3] * c[1], a[1] * c[0] + a[4] * c[1]); }
    transpose() { const a = this.m; return new Mat3([a[0], a[3], a[6], a[1], a[4], a[7], a[2], a[5], a[8]]); }
    determinant() { const a = this.m; return a[0] * (a[4] * a[8] - a[7] * a[5]) - a[3] * (a[1] * a[8] - a[7] * a[2]) + a[6] * (a[1] * a[5] - a[4] * a[2]); }
    inverse() {
      const a = this.m, d = this.determinant();
      if (!d) return new Mat3();
      const o = [a[4] * a[8] - a[5] * a[7], a[2] * a[7] - a[1] * a[8], a[1] * a[5] - a[2] * a[4], a[5] * a[6] - a[3] * a[8], a[0] * a[8] - a[2] * a[6], a[2] * a[3] - a[0] * a[5], a[3] * a[7] - a[4] * a[6], a[1] * a[6] - a[0] * a[7], a[0] * a[4] - a[1] * a[3]];
      return new Mat3(o.map((x) => x / d));
    }
    decompose() { const a = this.m; return { translation: new Vec2(a[6], a[7]), rotation: Math.atan2(a[1], a[0]) * RAD, scale: new Vec2(Math.hypot(a[0], a[1]), Math.hypot(a[3], a[4])) }; }
    copy() { return new Mat3(this.m); }
    equals(o) { return this.m.every((a, i) => Math.abs(a - o.m[i]) < EPS); }
    toString() { return this.m.join(' '); }
  }

  const WEMath = {
    smoothStep: (min, max, value) => { const t = Math.min(1, Math.max(0, (value - min) / ((max - min) || 1))); return t * t * (3 - 2 * t); },
    mix: (a, b, value) => a + (b - a) * value,
    deg2rad: DEG,
    rad2deg: RAD,
  };
  const WEVector = {
    angleVector2: (angle) => new Vec2(Math.cos(angle * DEG), Math.sin(angle * DEG)),
    vectorAngle2: (dir) => { const v = new Vec2(dir); return Math.atan2(v.y, v.x) * RAD; },
  };
  const WEColor = {
    rgb2hsv: (rgb) => {
      const v = new Vec3(rgb); const max = Math.max(v.x, v.y, v.z), min = Math.min(v.x, v.y, v.z);
      let h = 0, s = 0; const val = max;
      if (max > 0 && max - min > 0) {
        s = (max - min) / max;
        if (max === v.x) h = 60 * ((v.y - v.z) / (max - min));
        else if (max === v.y) h = 60 * ((v.z - v.x) / (max - min)) + 120;
        else h = 60 * ((v.x - v.y) / (max - min)) + 240;
      }
      if (h < 0) h += 360;
      return new Vec3(h / 360, s, val);
    },
    hsv2rgb: (hsv) => {
      const v = new Vec3(hsv); const h = ((v.x % 1) + 1) % 1 * 6;
      const i = Math.floor(h) % 6, f = h - Math.floor(h);
      const p = v.z * (1 - v.y), q = v.z * (1 - v.y * f), t = v.z * (1 - v.y * (1 - f));
      switch (i) { case 0: return new Vec3(v.z, t, p); case 1: return new Vec3(q, v.z, p); case 2: return new Vec3(p, v.z, t); case 3: return new Vec3(p, q, v.z); case 4: return new Vec3(t, p, v.z); default: return new Vec3(v.z, p, q); }
    },
    normalizeColor: (rgb) => new Vec3(rgb).divide(255),
    expandColor: (rgb) => new Vec3(rgb).multiply(255),
  };
  const MODULES = { WEMath, WEVector, WEColor };

  const HOOKS = ['init', 'update', 'destroy', 'resizeScreen', 'applyUserProperties', 'applyGeneralSettings', 'cursorEnter', 'cursorLeave', 'cursorMove', 'cursorDown', 'cursorUp', 'cursorClick', 'mediaStatusChanged', 'mediaPlaybackChanged', 'mediaPropertiesChanged', 'mediaThumbnailChanged', 'mediaTimelineChanged', 'animationEvent'];

  // ES module syntax → a function body: imports become parameters, exports plain declarations.
  function transformSource(source) {
    const imports = [];
    let body = source.replace(/^[ \t]*import\s+([^;'"]+?)\s+from\s+['"]([^'"]+)['"]\s*;?[ \t]*$/gm, (all, spec, mod) => {
      imports.push({ spec: spec.trim(), mod });
      return '';
    });
    body = body.replace(/^[ \t]*import\s+['"]([^'"]+)['"]\s*;?[ \t]*$/gm, '');
    body = body.replace(/^([ \t]*)export\s+default\s+/gm, '$1const __default = ');
    body = body.replace(/^([ \t]*)export\s+(?=(function|const|let|var|class|async)\b)/gm, '$1');
    body = body.replace(/^[ \t]*export\s*\{[^}]*\}\s*;?[ \t]*$/gm, '');
    let prelude = '';
    for (const { spec, mod } of imports) {
      const m = /^\*\s+as\s+([A-Za-z_$][\w$]*)$/.exec(spec);
      if (m) { prelude += 'const ' + m[1] + ' = __modules[' + JSON.stringify(mod) + '];\n'; continue; }
      const named = /^\{([^}]*)\}$/.exec(spec);
      if (named) {
        for (const part of named[1].split(',')) {
          const p = part.trim();
          if (!p) continue;
          const as = /^([A-Za-z_$][\w$]*)\s+as\s+([A-Za-z_$][\w$]*)$/.exec(p);
          if (as) prelude += 'const ' + as[2] + ' = __modules[' + JSON.stringify(mod) + '].' + as[1] + ';\n';
          else prelude += 'const ' + p + ' = __modules[' + JSON.stringify(mod) + '].' + p + ';\n';
        }
        continue;
      }
      const both = /^([A-Za-z_$][\w$]*)\s*,\s*\{([^}]*)\}$/.exec(spec);
      if (both) {
        prelude += 'const ' + both[1] + ' = __modules[' + JSON.stringify(mod) + '];\n';
        for (const part of both[2].split(',')) { const p = part.trim(); if (p) prelude += 'const ' + p + ' = __modules[' + JSON.stringify(mod) + '].' + p + ';\n'; }
        continue;
      }
      if (/^[A-Za-z_$][\w$]*$/.test(spec)) { prelude += 'const ' + spec + ' = __modules[' + JSON.stringify(mod) + '];\n'; continue; }
      throw new Error('SceneScript import that cannot be resolved: ' + spec + ' from ' + mod);
    }
    const epilogue = '\n;return {' + HOOKS.map((h) => h + ': (typeof ' + h + ' === "function") ? ' + h + ' : undefined').join(', ') + ', scriptProperties: (typeof scriptProperties !== "undefined") ? scriptProperties : undefined};';
    return prelude + body + epilogue;
  }

  function toScript(value, kind) {
    if (Array.isArray(value)) {
      if (value.length === 2) return new Vec2(value[0], value[1]);
      if (value.length === 3) return new Vec3(value[0], value[1], value[2]);
      return new Vec4(value[0], value[1], value[2], value[3]);
    }
    if (kind === 'bool') return !!value;
    return value;
  }

  function fromScript(value, kind) {
    if (value === undefined) return undefined;
    if (value instanceof Vec2) return [value.x, value.y];
    if (value instanceof Vec3) return [value.x, value.y, value.z];
    if (value instanceof Vec4) return [value.x, value.y, value.z, value.w];
    if (value && typeof value === 'object' && typeof value.x === 'number') return [value.x, value.y, typeof value.z === 'number' ? value.z : 0, typeof value.w === 'number' ? value.w : 1].slice(0, kind === 'vec2' ? 2 : kind === 'vec4' ? 4 : 3);
    return value;
  }

  // ---- event classes scripts receive -----------------------------------------------------------
  class MediaPlaybackEvent { constructor(state) { this.state = state; } }
  MediaPlaybackEvent.PLAYBACK_STOPPED = 0;
  MediaPlaybackEvent.PLAYBACK_PLAYING = 1;
  MediaPlaybackEvent.PLAYBACK_PAUSED = 2;
  class MediaStatusEvent { constructor(enabled) { this.enabled = !!enabled; } }
  class MediaPropertiesEvent {
    constructor(p) {
      const str = (k) => (p && typeof p[k] === 'string' ? p[k] : '');
      this.title = str('title'); this.artist = str('artist'); this.subTitle = str('subTitle'); this.albumTitle = str('albumTitle');
      this.albumArtist = str('albumArtist'); this.genres = str('genres'); this.contentType = str('contentType');
    }
  }
  class MediaThumbnailEvent {
    constructor(p) {
      this.hasThumbnail = !!(p && typeof p.thumbnail === 'string' && p.thumbnail.length);
      for (const k of ['primaryColor', 'secondaryColor', 'tertiaryColor', 'textColor', 'highContrastColor']) this[k] = cssColorToVec3(p ? p[k] : null, k);
    }
  }
  class MediaTimelineEvent { constructor(position, duration) { this.position = Number(position) || 0; this.duration = Number(duration) || 0; } }
  class CursorEvent { constructor(worldPosition, localPosition) { this.worldPosition = worldPosition; this.localPosition = localPosition; } }
  class AnimationEvent { constructor(name, frame) { this.name = name; this.frame = frame; } }
  class CameraTransforms { constructor(eye, center, up, zoom) { this.eye = eye; this.center = center; this.up = up; this.zoom = zoom; } }

  // The host hands thumbnail colours as CSS rgb(r, g, b) strings; scripts read 0..1 vectors.
  function cssColorToVec3(value, what) {
    if (value === null || value === undefined || value === '') return new Vec3(0, 0, 0);
    if (typeof value !== 'string') throw new Error('media thumbnail ' + what + ' is not a colour string: ' + JSON.stringify(value));
    const m = /^\s*rgba?\(\s*([\d.]+)\s*,\s*([\d.]+)\s*,\s*([\d.]+)/i.exec(value);
    if (m) return new Vec3(Number(m[1]) / 255, Number(m[2]) / 255, Number(m[3]) / 255);
    const parts = value.trim().split(/[\s,]+/).map(Number);
    if (parts.length >= 3 && parts.every((n) => Number.isFinite(n))) return parts.some((n) => n > 1) ? new Vec3(parts[0] / 255, parts[1] / 255, parts[2] / 255) : new Vec3(parts[0], parts[1], parts[2]);
    throw new Error('media thumbnail ' + what + ' is not a colour: ' + JSON.stringify(value));
  }

  const EVENT_CLASSES = { MediaPlaybackEvent, MediaStatusEvent, MediaPropertiesEvent, MediaThumbnailEvent, MediaTimelineEvent, CursorEvent, AnimationEvent, CameraTransforms };
  const FACTORY_PARAMS = ['__modules', 'thisLayer', 'thisObject', 'thisScene', 'engine', 'input', 'console', 'shared', 'localStorage', 'Vec2', 'Vec3', 'Vec4', 'Mat3', 'Mat4', 'createScriptProperties', 'renderContext'].concat(Object.keys(EVENT_CLASSES));

  function scaleVec(value, factor) {
    if (value instanceof Vec2 || value instanceof Vec3 || value instanceof Vec4) return value.map((a) => a * factor);
    if (typeof value === 'number') return value * factor;
    return value;
  }

  /**
   * One script attached to one bound value. `context`: `layer` (the ILayer handle or null for
   * scene values), `object` (what thisObject is: the layer, an effect or the scene), `key` (where
   * the value lives, for messages), `degrees` (the value is an angle the script sees in degrees).
   */
  class Module {
    constructor(engine, dyn, context) {
      this.engine = engine;
      this.dyn = dyn;
      this.context = context;
      this.layer = context.layer;
      this.key = context.key;
      this.initialized = false;
      const factory = new Function(...FACTORY_PARAMS, transformSource(dyn.script));
      const props = {};
      const store = dyn.scriptProperties || {};
      const creator = () => {
        const builder = {};
        for (const add of ['addSlider', 'addCheckbox', 'addCombo', 'addColor', 'addText', 'addTextInput', 'addFile', 'addDirectory']) {
          builder[add] = (o) => {
            if (o && typeof o.name === 'string') {
              Object.defineProperty(props, o.name, {
                enumerable: true, configurable: true,
                get: () => (store[o.name] ? toScript(store[o.name].get(), store[o.name].kind) : toScript(o.value)),
              });
            }
            return builder;
          };
        }
        builder.finish = () => props;
        return builder;
      };
      const args = [MODULES, context.layer, context.object, engine.scene, engine.engineObject, engine.inputObject, engine.consoleObject, engine.shared, engine.storage, Vec2, Vec3, Vec4, Mat3, Mat4, creator, engine.renderContext];
      for (const cls of Object.values(EVENT_CLASSES)) args.push(cls);
      this.exports = factory(...args);
    }
    has(hook) { return typeof this.exports[hook] === 'function'; }
    call(hook, ...args) {
      const fn = this.exports[hook];
      if (typeof fn !== 'function') return undefined;
      try { return fn(...args); } catch (e) { this.engine.host.error('SceneScript ' + this.key + '.' + hook + ': ' + (e && e.stack ? e.stack : e)); return undefined; }
    }
    // The bound value as the script sees it (angles in degrees).
    scriptValue() {
      const value = toScript(this.dyn.get(), this.dyn.kind);
      return this.context.degrees ? scaleVec(value, RAD) : value;
    }
    // Hand the current value in and apply what comes back.
    run(hook) {
      const fn = this.exports[hook];
      if (typeof fn !== 'function') return;
      let result;
      try { result = fn(this.scriptValue()); } catch (e) { this.engine.host.error('SceneScript ' + this.key + '.' + hook + ': ' + (e && e.stack ? e.stack : e)); return; }
      let back = fromScript(this.context.degrees ? scaleVec(result, DEG) : result, this.dyn.kind);
      if (back !== undefined && back !== null) this.dyn.set(back, 'script');
    }
  }

  /**
   * The scripting runtime of one scene. `host` supplies: log(text), error(text), storageKey,
   * screenSize() -> [w, h], canvasSize() -> [w, h], userProperties() -> Object,
   * cursorWorld() -> [x, y, z], cursorScreen() -> [x, y], cursorDown() -> Boolean, wantsAudio(),
   * registerAsset(handle), openUserShortcut(name) -> Boolean, sceneObject (the IScene handle).
   */
  class Engine {
    constructor(host) {
      this.host = host;
      this.modules = [];
      this.shared = {};
      this.timers = [];
      this.nextTimer = 1;
      this.audioBuffers = [];
      this.time = 0;
      this.dt = 0;
      this.renderContext = {};
      this.onFirstRun = null;
      this.consoleObject = { log: (...a) => host.log(a.map(String).join(' ')), error: (...a) => host.error(a.map(String).join(' ')) };
      const prefix = 'we-script:' + (host.storageKey || 'scene') + ':';
      const key = (k, loc) => prefix + (loc === 'global' ? 'global' : 'screen') + ':' + k;
      const store = () => {
        let s = null;
        try { s = G.localStorage; } catch (e) { s = null; }
        if (!s) throw new Error('SceneScript localStorage is not available in this page');
        return s;
      };
      this.storage = {
        LOCATION_GLOBAL: 'global', LOCATION_SCREEN: 'screen',
        set: (k, v, loc) => { store().setItem(key(k, loc), JSON.stringify(v === undefined ? null : v)); },
        get: (k, loc) => { const v = store().getItem(key(k, loc)); if (v === null) return null; try { return JSON.parse(v); } catch (e) { return v; } },
        delete: (k, loc) => { const s = store(); const had = s.getItem(key(k, loc)) !== null; s.removeItem(key(k, loc)); return had; },
        clear: (loc) => { const s = store(); const p = prefix + (loc === 'global' ? 'global' : 'screen') + ':'; for (let i = s.length - 1; i >= 0; i--) { const k = s.key(i); if (k && k.startsWith(p)) s.removeItem(k); } },
      };
      const self = this;
      this.engineObject = {
        isRunningInEditor: () => false,
        isPortrait: () => host.screenSize()[1] > host.screenSize()[0],
        isLandscape: () => host.screenSize()[0] >= host.screenSize()[1],
        isDesktopDevice: () => true,
        isMobileDevice: () => false,
        isWallpaper: () => true,
        isScreensaver: () => false,
        AUDIO_RESOLUTION_16: 16, AUDIO_RESOLUTION_32: 32, AUDIO_RESOLUTION_64: 64,
        registerAudioBuffers: (resolution) => {
          if (resolution !== 16 && resolution !== 32 && resolution !== 64) throw new Error('registerAudioBuffers takes engine.AUDIO_RESOLUTION_16, _32 or _64, not ' + resolution);
          const buffers = { left: new Float32Array(resolution), right: new Float32Array(resolution), average: new Float32Array(resolution), resolution };
          self.audioBuffers.push(buffers);
          host.wantsAudio();
          return buffers;
        },
        registerAsset: (file, precache) => { const handle = { path: String(file), precache: !!precache }; host.registerAsset(handle); return handle; },
        setTimeout: (cb, delay) => self.addTimer(cb, delay, false),
        setInterval: (cb, delay) => self.addTimer(cb, delay, true),
        openUserShortcut: (name) => host.openUserShortcut(String(name)),
        get screenResolution() { const s = host.screenSize(); return new Vec2(s[0], s[1]); },
        get canvasSize() { const s = host.canvasSize(); return new Vec2(s[0], s[1]); },
        get userProperties() { return host.userProperties(); },
        get timeOfDay() { const d = new Date(); return (d.getHours() * 3600 + d.getMinutes() * 60 + d.getSeconds()) / 86400; },
        get frametime() { return self.dt; },
        get runtime() { return self.time; },
      };
      this.inputObject = {
        get cursorWorldPosition() { const p = host.cursorWorld(); return new Vec3(p[0], p[1], p[2] || 0); },
        get cursorScreenPosition() { const p = host.cursorScreen(); return new Vec2(p[0], p[1]); },
        get cursorLeftDown() { return host.cursorDown(); },
      };
      this.scene = host.sceneObject;
    }

    addTimer(cb, delay, repeat) {
      if (typeof cb !== 'function') throw new Error('engine timers need a function');
      const id = this.nextTimer++;
      const ms = Math.max(0, Number(delay) || 0);
      this.timers.push({ id, cb, interval: ms / 1000, next: this.time + ms / 1000, repeat });
      return () => { const had = this.timers.some((t) => t.id === id); this.timers = this.timers.filter((t) => t.id !== id); return had; };
    }

    // Compile `dyn.script` in `context`; init runs on the next tick, update every tick after.
    attach(dyn, context) {
      const mod = new Module(this, dyn, context);
      this.modules.push(mod);
      return mod;
    }

    // Destroy and drop the modules `predicate(module)` selects.
    detach(predicate) {
      for (const m of this.modules) if (predicate(m)) m.call('destroy');
      this.modules = this.modules.filter((m) => !predicate(m));
    }

    tick(time, dt) {
      this.time = time;
      this.dt = dt;
      const due = this.timers.filter((t) => t.next <= time);
      for (const t of due) {
        if (t.repeat) t.next = time + t.interval; else this.timers = this.timers.filter((x) => x !== t);
        try { t.cb(); } catch (e) { this.host.error('SceneScript timer: ' + (e && e.stack ? e.stack : e)); }
      }
      for (const m of this.modules.slice()) {
        if (!m.initialized) {
          m.initialized = true;
          m.run('init');
          if (this.onFirstRun) this.onFirstRun(m);
        }
        m.run('update');
      }
    }

    // Fan an event out to every module (`predicate` narrows the audience).
    notify(hook, arg, predicate) {
      for (const m of this.modules.slice()) if (!predicate || predicate(m)) m.call(hook, arg);
    }

    anyDefines(hook) { return this.modules.some((m) => m.has(hook)); }

    // 64-band stereo spectrum -> every registered buffer's resolution.
    feedAudio(left64, right64) {
      for (const b of this.audioBuffers) {
        const step = 64 / b.resolution;
        for (let i = 0; i < b.resolution; i++) {
          let l = 0, r = 0;
          for (let k = 0; k < step; k++) { l += left64[i * step + k]; r += right64[i * step + k]; }
          b.left[i] = l / step; b.right[i] = r / step; b.average[i] = (b.left[i] + b.right[i]) / 2;
        }
      }
    }

    destroy() {
      for (const m of this.modules) m.call('destroy');
      this.modules = [];
      this.timers = [];
    }
  }

  const api = { Vec2, Vec3, Vec4, Mat3, Mat4, WEMath, WEColor, WEVector, Engine, Module, transformSource, toScript, fromScript, HOOKS, EVENT_CLASSES, FACTORY_PARAMS, cssColorToVec3, scaleVec, DEG, RAD };
  Object.assign(api, EVENT_CLASSES);
  G.WEScript = api;
  if (typeof module !== 'undefined' && module.exports) module.exports = api;
})();
