// Dynamic scene values: literals, user-property bindings with conditions, timeline
// animations and SceneScript sources, as Wallpaper Engine's scene.json declares them.
(() => {
  'use strict';
  const G = typeof window !== 'undefined' ? window : globalThis;

  const VEC_RE = /^\s*-?(\d+\.?\d*|\.\d+)([eE][-+]?\d+)?(\s+-?(\d+\.?\d*|\.\d+)([eE][-+]?\d+)?)*\s*$/;

  // "1 2 3" → [1, 2, 3]; anything else → null.
  function parseVecString(s) {
    if (typeof s !== 'string' || !VEC_RE.test(s)) return null;
    return s.trim().split(/\s+/).map(Number);
  }

  function isVec(v) { return Array.isArray(v) && v.length >= 2 && v.every((n) => typeof n === 'number'); }

  // Coerce any JSON or property value to the kind a setting expects.
  function coerce(value, kind) {
    switch (kind) {
      case 'bool':
        if (typeof value === 'boolean') return value;
        if (typeof value === 'number') return value !== 0;
        if (typeof value === 'string') { const t = value.trim().toLowerCase(); return t === 'true' || t === '1' || (t !== '' && t !== 'false' && t !== '0' && Number(t) !== 0 && !Number.isNaN(Number(t))); }
        return !!value;
      case 'int': {
        const n = toNumber(value);
        return Math.trunc(n);
      }
      case 'float': return toNumber(value);
      case 'string': return typeof value === 'string' ? value : (value === null || value === undefined ? '' : String(value));
      case 'vec2': return toVec(value, 2);
      case 'vec3': return toVec(value, 3);
      case 'color': return toVec(value, 3, true);
      case 'vec4': return toVec(value, 4);
      default: return value;
    }
  }

  function toNumber(value) {
    if (typeof value === 'number') return value;
    if (typeof value === 'boolean') return value ? 1 : 0;
    if (typeof value === 'string') { const v = parseVecString(value); if (v) return v[0]; const n = Number(value); return Number.isFinite(n) ? n : 0; }
    if (isVec(value)) return value[0];
    return 0;
  }

  // Wallpaper Engine colour strings, as linux-wallpaperengine's ColorBuilder::parse reads them:
  // "#rgb", "#rgba", "#rrggbb", "#rrggbbaa", or three/four components separated by spaces or
  // commas; components above 1 are 0..255 integers, otherwise they are 0..1 floats.
  function parseColor(value) {
    let copy = value.trim().split(',').join(' ');
    if (copy[0] === '#') {
      let n = copy.slice(1);
      if (n.length === 3) n = n[0] + n[0] + n[1] + n[1] + n[2] + n[2] + 'ff';
      else if (n.length === 4) n = n[0] + n[0] + n[1] + n[1] + n[2] + n[2] + n[3] + n[3];
      else if (n.length === 6) n += 'ff';
      else if (n.length !== 8) throw new Error('invalid CSS colour notation ' + value);
      if (!/^[0-9a-fA-F]{8}$/.test(n)) throw new Error('invalid CSS colour notation ' + value);
      const c = parseInt(n, 16);
      return [((c >>> 24) & 0xff) / 255, ((c >>> 16) & 0xff) / 255, ((c >>> 8) & 0xff) / 255, (c & 0xff) / 255];
    }
    const parts = parseVecString(copy);
    if (!parts || (parts.length !== 3 && parts.length !== 4)) throw new Error('invalid colour value ' + JSON.stringify(value));
    if (copy.indexOf('.') < 0 && parts.some((n) => n > 1)) return parts.map((n) => n / 255);
    return parts;
  }

  function toVec(value, size, color) {
    let parts;
    if (isVec(value)) parts = value;
    else if (color && typeof value === 'string' && value.trim() !== '' && (value.trim()[0] === '#' || value.indexOf(',') >= 0 || (parseVecString(value) || []).length >= 3)) parts = parseColor(value);
    else if (typeof value === 'number') parts = [value];
    else if (typeof value === 'boolean') parts = [value ? 1 : 0];
    else if (typeof value === 'string') parts = parseVecString(value) || [Number(value) || 0];
    else if (value && typeof value === 'object' && typeof value.x === 'number') parts = [value.x, value.y, value.z, value.w].filter((n) => typeof n === 'number');
    else parts = [0];
    const out = new Array(size);
    for (let i = 0; i < size; i++) out[i] = parts.length === 1 ? parts[0] : (parts[i] === undefined ? (i === 3 ? 1 : 0) : parts[i]);
    return out;
  }

  // The kind a literal JSON value implies when the caller has no expectation.
  function inferKind(value, expectColor) {
    if (typeof value === 'boolean') return 'bool';
    if (typeof value === 'number') return Number.isInteger(value) ? 'int' : 'float';
    if (typeof value === 'string') {
      const v = parseVecString(value);
      if (!v) return 'string';
      if (v.length === 1) return 'float';
      if (expectColor) return 'color';
      return 'vec' + Math.min(4, v.length);
    }
    if (isVec(value)) return 'vec' + Math.min(4, value.length);
    return 'string';
  }

  /** A value that other systems watch; user properties, scripts and animations feed it. */
  class Dynamic {
    constructor(value, kind) {
      this.kind = kind;
      this.value = coerce(value, kind);
      this.base = this.value;
      this.listeners = [];
      this.userName = null;
      this.condition = null;
      this.script = null;
      this.scriptProperties = null;
      this.animation = null;
    }
    get() { return this.value; }
    getNumber() { return toNumber(this.value); }
    getBool() { return coerce(this.value, 'bool'); }
    getVec(size) { return toVec(this.value, size); }
    getString() { return coerce(this.value, 'string'); }
    set(value, source) {
      this.value = coerce(value, this.kind);
      if (source !== 'animation') this.base = this.value;
      for (const fn of this.listeners) fn(this.value, source || 'set');
    }
    listen(fn) {
      this.listeners.push(fn);
      return () => { this.listeners = this.listeners.filter((f) => f !== fn); };
    }
    // Apply a user property value (in Wallpaper Engine's form) through the binding.
    applyUser(propertyValue) {
      if (this.condition !== null) this.set(evaluateCondition(propertyValue, this.condition), 'user');
      else this.set(convertProperty(propertyValue, this.kind), 'user');
    }
  }

  // Wallpaper Engine property values ("r g b" colors, combo values, numbers, booleans).
  function convertProperty(value, kind) {
    if (kind === 'color' || kind === 'vec3' || kind === 'vec4' || kind === 'vec2') return coerce(value, kind);
    if (kind === 'bool') return coerce(value, 'bool');
    if (kind === 'string') return coerce(value, 'string');
    return coerce(value, kind);
  }

  // `condition` from a `{"user": {"name": ..., "condition": ...}}` binding: a value to equal,
  // or a comparison such as ">= 2".
  function evaluateCondition(propertyValue, condition) {
    const c = typeof condition === 'string' ? condition.trim() : condition;
    if (typeof c === 'string') {
      const m = /^(==|!=|>=|<=|>|<)\s*(.+)$/.exec(c);
      if (m) {
        const a = toNumber(propertyValue), b = toNumber(m[2]);
        switch (m[1]) {
          case '==': return looseEqual(propertyValue, m[2]);
          case '!=': return !looseEqual(propertyValue, m[2]);
          case '>=': return a >= b;
          case '<=': return a <= b;
          case '>': return a > b;
          default: return a < b;
        }
      }
    }
    return looseEqual(propertyValue, c);
  }

  function looseEqual(a, b) {
    if (typeof a === 'boolean' || typeof b === 'boolean') return coerce(a, 'bool') === coerce(b, 'bool');
    const na = typeof a === 'number' ? a : (typeof a === 'string' && a.trim() !== '' && Number.isFinite(Number(a)) ? Number(a) : null);
    const nb = typeof b === 'number' ? b : (typeof b === 'string' && b.trim() !== '' && Number.isFinite(Number(b)) ? Number(b) : null);
    if (na !== null && nb !== null) return na === nb;
    return String(a).trim() === String(b).trim();
  }

  // ---- timeline animations -----------------------------------------------------------------
  function parseKeyframes(spec, where) {
    let list = spec;
    if (spec && typeof spec === 'object' && !Array.isArray(spec)) {
      if (Array.isArray(spec.keyframes)) list = spec.keyframes;
      else if (Array.isArray(spec.frames)) list = spec.frames;
      else throw new Error(where + ': animation channel has no keyframe list (' + Object.keys(spec).join(', ') + ')');
    }
    if (!Array.isArray(list)) throw new Error(where + ': animation channel is not a keyframe list');
    const frames = list.map((k, i) => {
      if (typeof k === 'number') return { frame: i, value: k, tin: 0, tout: 0, hasTangents: false };
      if (!k || typeof k !== 'object') throw new Error(where + ': keyframe ' + i + ' is not an object');
      const frame = typeof k.frame === 'number' ? k.frame : (typeof k.time === 'number' ? k.time : (typeof k.t === 'number' ? k.t : null));
      const value = typeof k.value === 'number' ? k.value : (typeof k.v === 'number' ? k.v : null);
      if (frame === null || value === null) throw new Error(where + ': keyframe ' + i + ' lacks frame/value (' + Object.keys(k).join(', ') + ')');
      const tin = typeof k.tangentin === 'number' ? k.tangentin : (typeof k.in === 'number' ? k.in : null);
      const tout = typeof k.tangentout === 'number' ? k.tangentout : (typeof k.out === 'number' ? k.out : null);
      return { frame, value, tin: tin === null ? 0 : tin, tout: tout === null ? 0 : tout, hasTangents: tin !== null || tout !== null, interpolation: typeof k.interpolation === 'string' ? k.interpolation : null };
    });
    frames.sort((a, b) => a.frame - b.frame);
    return frames;
  }

  function sampleChannel(frames, frame) {
    if (!frames.length) return 0;
    if (frame <= frames[0].frame) return frames[0].value;
    const last = frames[frames.length - 1];
    if (frame >= last.frame) return last.value;
    let i = 1;
    while (i < frames.length && frames[i].frame < frame) i++;
    const a = frames[i - 1], b = frames[i];
    const span = b.frame - a.frame;
    if (span <= 0) return b.value;
    const t = (frame - a.frame) / span;
    if (a.interpolation === 'step' || a.interpolation === 'constant') return a.value;
    if (a.hasTangents || b.hasTangents) {
      const t2 = t * t, t3 = t2 * t;
      const h00 = 2 * t3 - 3 * t2 + 1, h10 = t3 - 2 * t2 + t, h01 = -2 * t3 + 3 * t2, h11 = t3 - t2;
      return h00 * a.value + h10 * a.tout * span + h01 * b.value + h11 * b.tin * span;
    }
    return a.value + (b.value - a.value) * t;
  }

  /** `{"animation": {...}}` on a scene value. */
  class Animation {
    constructor(json, where) {
      if (!json || typeof json !== 'object') throw new Error(where + ': animation is not an object');
      const options = json.options && typeof json.options === 'object' ? json.options : json;
      this.name = typeof options.name === 'string' ? options.name : (typeof json.name === 'string' ? json.name : '');
      this.fps = typeof options.fps === 'number' && options.fps > 0 ? options.fps : 30;
      this.mode = typeof options.mode === 'string' ? options.mode : (typeof options.playbackmode === 'string' ? options.playbackmode : 'loop');
      this.relative = !!json.relative;
      this.channels = [];
      for (let c = 0; c < 4; c++) {
        const spec = json['c' + c];
        if (spec === undefined) continue;
        this.channels[c] = parseKeyframes(spec, where + ' channel c' + c);
      }
      if (!this.channels.some((c) => c)) throw new Error(where + ': animation declares no channels (' + Object.keys(json).join(', ') + ')');
      let length = typeof options.length === 'number' ? options.length : 0;
      if (length <= 0) for (const ch of this.channels) if (ch) length = Math.max(length, ch[ch.length - 1].frame);
      this.length = length;
      this.rate = 1;
      this.playing = true;
      this.offset = 0;
    }
    frameAt(time) {
      const raw = (time * this.rate + this.offset) * this.fps;
      const len = this.length;
      if (len <= 0) return 0;
      switch (this.mode) {
        case 'mirror': case 'pingpong': { const p = raw % (2 * len); return p <= len ? p : 2 * len - p; }
        case 'single': case 'once': case 'oneshot': return Math.min(raw, len);
        default: return raw % len;
      }
    }
    // Apply to `dyn` at `time`; vectors get per-channel values, scalars channel 0.
    apply(dyn, time) {
      if (!this.playing) return;
      const frame = this.frameAt(time);
      const base = dyn.base;
      if (isVec(base)) {
        const out = base.slice();
        for (let c = 0; c < out.length; c++) {
          const ch = this.channels[c];
          if (!ch) continue;
          const v = sampleChannel(ch, frame);
          out[c] = this.relative ? base[c] + v : v;
        }
        dyn.set(out, 'animation');
      } else if (typeof base === 'boolean') {
        const ch = this.channels[0];
        if (ch) dyn.set(sampleChannel(ch, frame) >= 0.5, 'animation');
      } else {
        const ch = this.channels[0];
        if (ch) {
          const v = sampleChannel(ch, frame);
          dyn.set(this.relative ? toNumber(base) + v : v, 'animation');
        }
      }
    }
  }

  /**
   * Turn a scene.json value into a Dynamic. `kind` names the expected type; `expectColor`
   * reads "r g b" strings as colors; `properties` is the PropertyStore bindings attach to.
   */
  function setting(json, opts) {
    const o = opts || {};
    const where = o.where || 'value';
    // A Dynamic given where a value is expected binds directly (shared live values).
    if (json instanceof Dynamic) return json;
    let raw = json;
    let user = null, script = null, scriptProps = null, animation = null;
    if (json && typeof json === 'object' && !Array.isArray(json)) {
      if (!('value' in json)) {
        if ('user' in json || 'script' in json || 'animation' in json) throw new Error(where + ': bound value has no "value" member');
        raw = json;
      } else {
        raw = json.value;
        if (json.user !== undefined && json.user !== null) user = json.user;
        if (typeof json.script === 'string') { script = json.script; scriptProps = json.scriptproperties && typeof json.scriptproperties === 'object' ? json.scriptproperties : {}; }
        if (json.animation !== undefined && json.animation !== null) animation = json.animation;
      }
    }
    if (raw === undefined || raw === null) raw = o.default !== undefined ? o.default : (o.kind === 'bool' ? false : o.kind === 'string' ? '' : 0);
    const kind = o.kind || inferKind(raw, o.expectColor);
    const dyn = new Dynamic(raw, kind);
    dyn.where = where;
    if (user !== null) {
      let name, condition = null;
      if (typeof user === 'string') name = user;
      else if (user && typeof user === 'object' && typeof user.name === 'string') { name = user.name; condition = user.condition === undefined ? null : user.condition; }
      else throw new Error(where + ': unreadable user binding ' + JSON.stringify(user));
      dyn.userName = name;
      dyn.condition = condition;
      if (o.properties) o.properties.bind(dyn);
    }
    if (script !== null) {
      dyn.script = script;
      dyn.scriptProperties = {};
      for (const [k, v] of Object.entries(scriptProps)) dyn.scriptProperties[k] = setting(v, { where: where + '.scriptproperties.' + k, properties: o.properties });
    }
    if (animation !== null) dyn.animation = new Animation(animation, where);
    return dyn;
  }

  /** The user properties the host pushes, and the values bound to them. */
  class PropertyStore {
    constructor() {
      this.values = {};
      this.bindings = {};
      this.listeners = [];
    }
    bind(dyn) {
      (this.bindings[dyn.userName] = this.bindings[dyn.userName] || []).push(dyn);
      if (dyn.userName in this.values) dyn.applyUser(this.values[dyn.userName]);
    }
    // `changed` maps property names to `{value}` objects as applyUserProperties delivers.
    apply(changed) {
      const names = [];
      for (const [name, entry] of Object.entries(changed || {})) {
        const value = entry && typeof entry === 'object' && 'value' in entry ? entry.value : entry;
        this.values[name] = value;
        names.push(name);
        for (const dyn of this.bindings[name] || []) dyn.applyUser(value);
      }
      for (const fn of this.listeners) fn(names);
    }
    get(name) { return this.values[name]; }
    has(name) { return name in this.values; }
    listen(fn) { this.listeners.push(fn); }
    // Texture-variant and similar `{"condition": ...}` JSON: true when it holds.
    conditionHolds(json) {
      let data;
      try { data = JSON.parse(json); } catch (e) { throw new Error('condition is not JSON: ' + json); }
      const cond = data && data.condition;
      if (typeof cond === 'string') return cond in this.values ? coerce(this.values[cond], 'bool') : false;
      if (cond && typeof cond === 'object' && typeof cond.name === 'string') {
        if (!(cond.name in this.values)) return false;
        return evaluateCondition(this.values[cond.name], cond.condition);
      }
      throw new Error('unreadable condition ' + json);
    }
  }

  const api = { Dynamic, Animation, PropertyStore, setting, coerce, toNumber, toVec, parseColor, parseVecString, inferKind, evaluateCondition, looseEqual, isVec };
  G.WEProps = api;
  if (typeof module !== 'undefined' && module.exports) module.exports = api;
})();
