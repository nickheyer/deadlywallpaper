// Wallpaper Engine particle systems: emitters, initializers, operators, control points and
// the sprite / rope vertex streams the generic particle shaders consume.
(() => {
  'use strict';
  const G = typeof window !== 'undefined' ? window : globalThis;
  const M = G.WEM, Noise = G.WENoise, Props = G.WEProps;

  const DEFAULT_MAX = 1000;
  const SPRITE_FLOATS = 17;
  const ROPE_FLOATS = 26;
  const TWO_PI = Math.PI * 2;

  const rand = (a, b) => a + Math.random() * (b - a);
  const randVec = (a, b) => [rand(a[0], b[0]), rand(a[1], b[1]), rand(a[2], b[2])];
  const fade = (x, x0, x1, y0, y1) => {
    if (x1 === x0) return x <= x0 ? y0 : y1;
    const t = Math.max(0, Math.min(1, (x - x0) / (x1 - x0)));
    return y0 + (y1 - y0) * t;
  };
  const num = (d, dflt) => (d && typeof d.getNumber === 'function') ? d.getNumber() : (typeof d === 'number' ? d : dflt);
  const vec = (d, size, dflt) => (d && typeof d.getVec === 'function') ? d.getVec(size) : (Array.isArray(d) ? d : dflt);

  function newParticle() {
    return {
      position: [0, 0, 0], velocity: [0, 0, 0], rotation: [0, 0, 0], angularVelocity: [0, 0, 0],
      color: [1, 1, 1], alpha: 1, size: 20, frame: -1, lifetime: 1, age: 0, alive: false,
      initial: { color: [1, 1, 1], alpha: 1, size: 20, lifetime: 1 },
      oscAlpha: null, oscSize: null, oscPos: null,
    };
  }

  // Audio amplitude over a band range in 0..1, shaped as an emitter asks.
  function audioLevel(audio, start, end, bounds, exponent) {
    if (!audio || !audio.length) return 0;
    const bands = audio.length / 2;
    const a = Math.max(0, Math.min(bands - 1, start | 0)), b = Math.max(a, Math.min(bands - 1, end | 0));
    let sum = 0;
    for (let i = a; i <= b; i++) sum += (audio[i] + audio[bands + i]) / 2;
    const amp = sum / (b - a + 1);
    const lo = bounds[0], hi = bounds[1];
    const t = hi > lo ? Math.max(0, Math.min(1, (amp - lo) / (hi - lo))) : (amp >= hi ? 1 : 0);
    return Math.pow(t, exponent > 0 ? exponent : 1);
  }

  /** Parse the JSON of a particle definition as scene.json and particle files hold it. */
  function parseDefinition(json, object, setting) {
    const properties = { where: 'particle' };
    const s = (v, opts) => setting(v, Object.assign({}, properties, opts || {}));
    const vec3 = (field, obj, dflt) => {
      const v = obj[field];
      if (v === undefined || v === null) return dflt;
      if (typeof v === 'number') return [v, v, v];
      if (typeof v === 'string') { const p = Props.parseVecString(v); return p ? (p.length === 1 ? [p[0], p[0], p[0]] : [p[0], p[1] || 0, p[2] || 0]) : dflt; }
      if (Array.isArray(v) && v.length >= 3) return [Number(v[0]), Number(v[1]), Number(v[2])];
      return dflt;
    };
    const vec2 = (field, obj, dflt) => {
      const v = obj[field];
      if (v === undefined || v === null) return dflt;
      if (typeof v === 'string') { const p = Props.parseVecString(v); return p ? [p[0], p[1] === undefined ? p[0] : p[1]] : dflt; }
      if (Array.isArray(v) && v.length >= 2) return [Number(v[0]), Number(v[1])];
      if (typeof v === 'number') return [v, v];
      return dflt;
    };
    const opt = (obj, field, dflt) => (obj[field] === undefined || obj[field] === null ? dflt : obj[field]);
    const def = {
      emitters: [], initializers: [], operators: [], renderers: [], controlPoints: [], children: [],
      material: typeof json.material === 'string' ? json.material : null,
      animationMode: typeof json.animationmode === 'string' ? json.animationmode : 'sequence',
      sequenceMultiplier: typeof json.sequencemultiplier === 'number' ? json.sequencemultiplier : 1,
      maxCount: typeof json.maxcount === 'number' ? json.maxcount : 100,
      startTime: typeof json.starttime === 'number' ? json.starttime : 0,
      flags: typeof json.flags === 'number' ? json.flags : 0,
    };
    for (const e of json.emitter || []) {
      def.emitters.push({
        id: opt(e, 'id', -1), name: typeof e.name === 'string' ? e.name : '',
        directions: vec3('directions', e, [1, 1, 0]), distanceMin: vec3('distancemin', e, [0, 0, 0]), distanceMax: vec3('distancemax', e, [256, 256, 0]),
        origin: vec3('origin', e, [0, 0, 0]), sign: vec3('sign', e, [0, 0, 0]), instantaneous: opt(e, 'instantaneous', 0),
        speedMin: opt(e, 'speedmin', 0), speedMax: opt(e, 'speedmax', 0), rate: opt(e, 'rate', 10), controlPoint: opt(e, 'controlpoint', -1),
        flags: opt(e, 'flags', 0), cone: opt(e, 'cone', 0), delay: opt(e, 'delay', 0), duration: opt(e, 'duration', 0),
        audioBounds: vec2('audioprocessingbounds', e, [0.8, 1]), audioExponent: opt(e, 'audioprocessingexponent', 2),
        audioStart: opt(e, 'audioprocessingfrequencystart', 0), audioEnd: opt(e, 'audioprocessingfrequencyend', 1), audioMode: opt(e, 'audioprocessingmode', 0),
        minPeriodicDelay: opt(e, 'minperiodicdelay', 1), maxPeriodicDelay: opt(e, 'maxperiodicdelay', 2),
        minPeriodicDuration: opt(e, 'minperiodicduration', 2), maxPeriodicDuration: opt(e, 'maxperiodicduration', 3),
      });
    }
    for (const it of json.initializer || []) {
      const name = typeof it.name === 'string' ? it.name : '';
      const w = { where: 'particle initializer ' + name };
      const u = (field, dflt, expectColor) => s(it[field] === undefined ? dflt : it[field], { where: w.where + '.' + field, expectColor, default: dflt });
      switch (name) {
        case 'colorrandom': def.initializers.push({ name, min: u('min', '0 0 0', true), max: u('max', '1 1 1', true) }); break;
        case 'sizerandom': def.initializers.push({ name, min: u('min', 0), max: u('max', 20), exponent: u('exponent', 1) }); break;
        case 'alpharandom': def.initializers.push({ name, min: u('min', 0.05), max: u('max', 1) }); break;
        case 'lifetimerandom': def.initializers.push({ name, min: u('min', 0), max: u('max', 1) }); break;
        case 'velocityrandom': def.initializers.push({ name, min: u('min', '-32 -32 -32'), max: u('max', '32 32 32') }); break;
        case 'rotationrandom': def.initializers.push({ name, min: u('min', '0 0 0'), max: u('max', '0 0 ' + TWO_PI) }); break;
        case 'angularvelocityrandom': def.initializers.push({ name, min: u('min', '0 0 -5'), max: u('max', '0 0 5'), exponent: u('exponent', 1) }); break;
        case 'turbulentvelocityrandom': def.initializers.push({ name, speedMin: u('speedmin', 100), speedMax: u('speedmax', 250), scale: u('scale', 1), offset: u('offset', 0), forward: u('forward', '0 1 0'), timeScale: u('timescale', 1), phaseMin: u('phasemin', 0), phaseMax: u('phasemax', 0.1), right: u('right', '0 0 1') }); break;
        case 'mapsequencearoundcontrolpoint': def.initializers.push({ name, controlPoint: u('controlpoint', 0), count: u('count', 1), speedMin: u('speedmin', '0 0 0'), speedMax: u('speedmax', '100 100 100') }); break;
        default: throw new Error('particle initializer "' + name + '" is not one Wallpaper Engine defines');
      }
    }
    for (const op of json.operator || []) {
      const name = typeof op.name === 'string' ? op.name : '';
      const w = 'particle operator ' + name;
      const u = (field, dflt, expectColor) => s(op[field] === undefined ? dflt : op[field], { where: w + '.' + field, expectColor, default: dflt });
      switch (name) {
        case 'movement': def.operators.push({ name, drag: u('drag', 0), gravity: u('gravity', '0 0 0') }); break;
        case 'angularmovement': def.operators.push({ name, drag: u('drag', 0), force: u('force', '0 0 0') }); break;
        case 'alphafade': def.operators.push({ name, fadeInTime: u('fadeintime', 0.5), fadeOutTime: u('fadeouttime', 0.5) }); break;
        case 'sizechange': case 'alphachange': def.operators.push({ name, startTime: u('starttime', 0), endTime: u('endtime', 1), startValue: u('startvalue', 1), endValue: u('endvalue', 0) }); break;
        case 'colorchange': def.operators.push({ name, startTime: u('starttime', 0), endTime: u('endtime', 1), startValue: u('startvalue', '1 1 1'), endValue: u('endvalue', '1 1 1') }); break;
        case 'turbulence': def.operators.push({ name, scale: u('scale', 0.005), speedMin: u('speedmin', 500), speedMax: u('speedmax', 1000), timeScale: u('timescale', 0.01), mask: u('mask', '1 1 0'), phaseMin: u('phasemin', 0), phaseMax: u('phasemax', 0), audioMode: u('audioprocessingmode', 0), audioBounds: u('audioprocessingbounds', '0 1'), audioExponent: u('audioprocessingexponent', 1), audioStart: u('audioprocessingfrequencystart', 0), audioEnd: u('audioprocessingfrequencyend', 15) }); break;
        case 'vortex': case 'vortex_v2': def.operators.push({ name: 'vortex', controlPoint: opt(op, 'controlpoint', 0), flags: opt(op, 'flags', 0), axis: u('axis', '0 0 1'), offset: u('offset', '0 0 0'), distanceInner: u('distanceinner', 500), distanceOuter: u('distanceouter', 650), speedInner: u('speedinner', 2500), speedOuter: u('speedouter', 0), centerForce: u('centerforce', 1), ringRadius: u('ringradius', 300), ringWidth: u('ringwidth', 50), ringPullDistance: u('ringpulldistance', 50), ringPullForce: u('ringpullforce', 10), audioMode: u('audioprocessingmode', 0), audioBounds: u('audioprocessingbounds', '0 1') }); break;
        case 'controlpointattract': def.operators.push({ name, controlPoint: opt(op, 'controlpoint', 0), origin: u('origin', '0 0 0'), scale: u('scale', 100), threshold: u('threshold', 1000) }); break;
        case 'oscillatealpha': case 'oscillatesize': def.operators.push({ name, frequencyMin: u('frequencymin', 0), frequencyMax: u('frequencymax', 10), scaleMin: u('scalemin', name === 'oscillatesize' ? 0.8 : 0), scaleMax: u('scalemax', name === 'oscillatesize' ? 1.2 : 1), phaseMin: u('phasemin', 0), phaseMax: u('phasemax', TWO_PI) }); break;
        case 'oscillateposition': def.operators.push({ name, frequencyMin: u('frequencymin', 0), frequencyMax: u('frequencymax', 5), scaleMin: u('scalemin', 0), scaleMax: u('scalemax', 10), phaseMin: u('phasemin', 0), phaseMax: u('phasemax', TWO_PI), mask: u('mask', '1 1 0') }); break;
        default: throw new Error('particle operator "' + name + '" is not one Wallpaper Engine defines');
      }
    }
    for (const rd of json.renderer || []) {
      const name = typeof rd.name === 'string' ? rd.name : 'sprite';
      if (!['sprite', 'spritetrail', 'rope', 'ropetrail'].includes(name)) throw new Error('particle renderer "' + name + '" is not one Wallpaper Engine defines');
      def.renderers.push({
        name, length: opt(rd, 'length', name === 'ropetrail' ? 1 : 0.05), maxLength: opt(rd, 'maxlength', 10), minLength: opt(rd, 'minlength', 0),
        subdivision: opt(rd, 'subdivision', name === 'rope' ? 4 : 1), segments: opt(rd, 'segments', 4), uvScale: opt(rd, 'uvscale', 1),
        uvScrolling: !!opt(rd, 'uvscrolling', false), uvSmoothing: opt(rd, 'uvsmoothing', true) !== false, fadeAlpha: !!opt(rd, 'fadealpha', false), fadeSize: !!opt(rd, 'fadesize', false),
      });
    }
    if (!def.renderers.length) def.renderers.push({ name: 'sprite', length: 0.05, maxLength: 10, minLength: 0, subdivision: 1, segments: 4, uvScale: 1, uvScrolling: false, uvSmoothing: true, fadeAlpha: false, fadeSize: false });
    for (const cp of json.controlpoint || []) {
      let flags = opt(cp, 'flags', 0);
      if (opt(cp, 'followmouse', false)) flags |= 1;
      if (opt(cp, 'worldspace', false)) flags |= 2;
      def.controlPoints.push({ id: opt(cp, 'id', -1), flags, offset: vec3('offset', cp, [0, 0, 0]), lockToPointer: !!opt(cp, 'locktopointer', false) });
    }
    for (const ch of json.children || []) {
      def.children.push({ type: typeof ch.type === 'string' ? ch.type : 'static', name: typeof ch.name === 'string' ? ch.name : '', maxCount: opt(ch, 'maxcount', null), controlPointStartIndex: opt(ch, 'controlpointstartindex', 0), probability: opt(ch, 'probability', 1), angles: vec3('angles', ch, [0, 0, 0]), origin: vec3('origin', ch, [0, 0, 0]), scale: vec3('scale', ch, [1, 1, 1]), particle: typeof ch.particle === 'string' ? ch.particle : '' });
    }
    const io = object && object.instanceoverride && typeof object.instanceoverride === 'object' ? object.instanceoverride : {};
    const iu = (field, dflt, expectColor) => s(io[field] === undefined ? dflt : io[field], { where: 'instanceoverride.' + field, expectColor, default: dflt });
    def.instance = { enabled: iu('enabled', true), alpha: iu('alpha', 1), size: iu('size', 1), lifetime: iu('lifetime', 1), rate: iu('rate', 1), speed: iu('speed', 1), count: iu('count', 1), color: iu('color', '1 1 1', true), colorn: iu('colorn', '1 1 1', true) };
    for (let i = 0; i < 8; i++) {
      const key = 'controlpoint' + i;
      if (io[key] !== undefined) def.instance[key] = s(io[key], { where: 'instanceoverride.' + key, kind: 'vec3' });
    }
    return def;
  }

  /** A running particle system in the scene's centred space. */
  class System {
    constructor(def, opts) {
      this.def = def;
      this.origin = opts.origin;
      this.scale = opts.scale;
      this.angles = opts.angles;
      this.sceneWidth = opts.sceneWidth;
      this.sceneHeight = opts.sceneHeight;
      this.audio = opts.audio;
      this.renderer = def.renderers[0];
      this.rope = this.renderer.name === 'rope' || this.renderer.name === 'ropetrail';
      this.trail = this.renderer.name === 'ropetrail' || this.renderer.name === 'spritetrail';
      const countMul = num(def.instance.count, 1);
      const adjusted = Math.floor(def.maxCount * countMul);
      this.maxParticles = adjusted > 0 ? adjusted : DEFAULT_MAX;
      this.particles = [];
      for (let i = 0; i < this.maxParticles; i++) this.particles.push(newParticle());
      this.count = 0;
      this.time = 0;
      this.started = false;
      this.playing = true;
      this.emitting = true;
      this.controlPoints = [];
      for (let i = 0; i < 8; i++) this.controlPoints.push({ position: [0, 0, 0], offset: [0, 0, 0], linkMouse: false, worldSpace: false, mirrored: false });
      this.transformedOrigin = [0, 0, 0];
      this.spritesheet = opts.spritesheet || null;
      this.uniformLifetimes = false;
      this.emitterStates = def.emitters.map((e) => ({ timer: 0, delay: e.delay, duration: 0, periodicTimer: 0, periodicDuration: 0, periodicDelay: 0, emitting: false, instantaneousDone: false }));
      this.turbulence = def.operators.filter((o) => o.name === 'turbulence').map((o) => ({ phase: rand(num(o.phaseMin, 0), num(o.phaseMax, 0)), speed: rand(num(o.speedMin, 500), num(o.speedMax, 1000)) }));
      this.sequence = 0;
      for (const it of def.initializers) if (it.name === 'lifetimerandom') this.uniformLifetimes = num(it.min, 0) === num(it.max, 1);
      this.refreshOrigin();
      this.forcedEmit = 0;
    }

    refreshOrigin() {
      const o = vec(this.origin, 3, [0, 0, 0]);
      this.transformedOrigin = [o[0] - this.sceneWidth / 2, this.sceneHeight / 2 - o[1], o[2]];
      for (const cp of this.def.controlPoints) {
        if (cp.id < 0 || cp.id >= 8) continue;
        const c = this.controlPoints[cp.id];
        if (c.mirrored) continue;
        c.offset = cp.offset.slice();
        c.linkMouse = (cp.flags & 1) !== 0 || cp.lockToPointer;
        c.worldSpace = (cp.flags & 2) !== 0;
        if (!c.linkMouse) c.position = c.worldSpace ? [cp.offset[0] - this.transformedOrigin[0], cp.offset[1] - this.transformedOrigin[1], cp.offset[2] - this.transformedOrigin[2]] : cp.offset.slice();
      }
    }

    // SceneScript IParticleSystem.
    play() { this.playing = true; this.emitting = true; }
    pause() { this.emitting = false; }
    stop() { this.emitting = false; this.count = 0; for (const p of this.particles) p.alive = false; }
    isPlaying() { return this.emitting || this.count > 0; }
    emitParticles(n) { this.forcedEmit += Math.max(0, n === undefined ? 1 : n | 0); }

    // Control point `index` follows a position supplied from outside (a parent system's control
    // point in this system's local frame); the definition and the pointer no longer move it.
    mirrorControlPoint(index, position) {
      const cp = this.controlPoints[index];
      cp.mirrored = true;
      cp.position = [position[0], position[1], position[2]];
    }

    controlPointPosition(index) {
      if (this.controlPoints[index].mirrored) return this.controlPoints[index].position;
      const override = this.def.instance['controlpoint' + index];
      if (override) { const v = override.getVec(3); return [v[0] - this.transformedOrigin[0] + this.controlPoints[index].offset[0], -v[1] - this.transformedOrigin[1] + this.controlPoints[index].offset[1], v[2]]; }
      return this.controlPoints[index].position;
    }

    spawn(p, spawnOrigin, randomPos, emitter) {
      p.position = [spawnOrigin[0] + randomPos[0], spawnOrigin[1] + randomPos[1], spawnOrigin[2] + randomPos[2]];
      p.velocity = [0, 0, 0];
      p.rotation = [0, 0, 0];
      p.angularVelocity = [0, 0, 0];
      const colorn = vec(this.def.instance.colorn, 3, [1, 1, 1]);
      p.color = [colorn[0], colorn[1], colorn[2]];
      p.alpha = num(this.def.instance.alpha, 1);
      p.size = 20 * num(this.def.instance.size, 1);
      p.lifetime = num(this.def.instance.lifetime, 1);
      p.age = 0;
      p.alive = true;
      p.frame = -1;
      p.initial.color = p.color.slice(); p.initial.alpha = p.alpha; p.initial.size = p.size; p.initial.lifetime = p.lifetime;
      p.oscAlpha = null; p.oscSize = null; p.oscPos = null;
      if (emitter && (emitter.speedMax > 0 || emitter.speedMin !== 0)) {
        const len = Math.hypot(randomPos[0], randomPos[1], randomPos[2]);
        const dir = len > 0 ? [randomPos[0] / len, randomPos[1] / len, randomPos[2] / len] : [0, 1, 0];
        const speed = rand(emitter.speedMin, emitter.speedMax);
        p.velocity = [dir[0] * speed, dir[1] * speed, dir[2] * speed];
      }
      for (const init of this.def.initializers) this.initialize(init, p);
    }

    initialize(init, p) {
      const speedMul = num(this.def.instance.speed, 1);
      switch (init.name) {
        case 'colorrandom': {
          const c = randVec(vec(init.min, 3, [0, 0, 0]), vec(init.max, 3, [1, 1, 1]));
          const n = vec(this.def.instance.colorn, 3, [1, 1, 1]);
          p.color = [c[0] * n[0], c[1] * n[1], c[2] * n[2]];
          p.initial.color = p.color.slice();
          break;
        }
        case 'sizerandom': {
          const t = Math.pow(Math.random(), num(init.exponent, 1));
          const mn = num(init.min, 0), mx = num(init.max, 20);
          p.size = (mn + t * (mx - mn)) * num(this.def.instance.size, 1) / 2;
          p.initial.size = p.size;
          break;
        }
        case 'alpharandom': p.alpha = rand(num(init.min, 0.05), num(init.max, 1)) * num(this.def.instance.alpha, 1); p.initial.alpha = p.alpha; break;
        case 'lifetimerandom': p.lifetime = rand(num(init.min, 0), num(init.max, 1)) * num(this.def.instance.lifetime, 1); p.initial.lifetime = p.lifetime; break;
        case 'velocityrandom': {
          const v = randVec(vec(init.min, 3, [-32, -32, -32]), vec(init.max, 3, [32, 32, 32]));
          p.velocity[0] += v[0] * speedMul; p.velocity[1] += -v[1] * speedMul; p.velocity[2] += v[2] * speedMul;
          break;
        }
        case 'rotationrandom': {
          const v = randVec(vec(init.min, 3, [0, 0, 0]), vec(init.max, 3, [0, 0, TWO_PI]));
          p.rotation = [v[0] * speedMul, v[1] * speedMul, v[2] * speedMul];
          break;
        }
        case 'angularvelocityrandom': {
          const mn = vec(init.min, 3, [0, 0, -5]), mx = vec(init.max, 3, [0, 0, 5]), ex = num(init.exponent, 1);
          const r = [0, 0, 0];
          for (let i = 0; i < 3; i++) { const t = Math.pow(Math.random(), ex); r[i] = (mn[i] + t * (mx[i] - mn[i])) * speedMul; }
          p.angularVelocity = r;
          break;
        }
        case 'turbulentvelocityrandom': {
          let forward = vec(init.forward, 3, [0, 1, 0]), right = vec(init.right, 3, [0, 0, 1]);
          forward = [forward[0], -forward[1], forward[2]];
          right = [right[0], -right[1], right[2]];
          const fl = Math.hypot(...forward), rl = Math.hypot(...right);
          forward = fl > 1e-4 ? forward.map((x) => x / fl) : [0, 1, 0];
          right = rl > 1e-4 ? right.map((x) => x / rl) : [1, 0, 0];
          const speed = rand(num(init.speedMin, 100), num(init.speedMax, 250));
          const scale = num(init.scale, 1), offset = num(init.offset, 0), timeScale = num(init.timeScale, 1);
          const phase = rand(num(init.phaseMin, 0), num(init.phaseMax, 0.1));
          const noisePos = [p.position[0] * 0.1 + this.time * timeScale + phase, p.position[1] * 0.1 + this.time * timeScale + phase * 0.7, p.position[2] * 0.1 + this.time * timeScale + phase * 1.3];
          let result = Noise.curl(noisePos);
          const len = Math.hypot(...result);
          result = len < 1e-4 ? forward.slice() : result.map((x) => x / len);
          if (scale < 2) {
            const cosA = Math.max(-1, Math.min(1, result[0] * forward[0] + result[1] * forward[1] + result[2] * forward[2]));
            const angle = Math.acos(cosA) / Math.PI;
            const maxAngle = scale / 2;
            if (angle > maxAngle && maxAngle > 1e-4) {
              const axis = [result[1] * forward[2] - result[2] * forward[1], result[2] * forward[0] - result[0] * forward[2], result[0] * forward[1] - result[1] * forward[0]];
              const al = Math.hypot(...axis);
              if (al > 1e-4) {
                const rot = M.rotation((angle - maxAngle) * Math.PI, axis[0] / al, axis[1] / al, axis[2] / al);
                result = M.transformDirection(rot, result);
              }
            }
          }
          if (Math.abs(offset) > 1e-4) result = M.transformDirection(M.rotation(-offset, right[0], right[1], right[2]), result);
          if ((this.def.flags & 4) === 0) { result[2] = 0; const l2 = Math.hypot(result[0], result[1]); if (l2 > 1e-4) { result[0] /= l2; result[1] /= l2; } }
          p.velocity[0] += result[0] * speed * speedMul; p.velocity[1] += result[1] * speed * speedMul; p.velocity[2] += result[2] * speed * speedMul;
          break;
        }
        case 'mapsequencearoundcontrolpoint': {
          const cp = num(init.controlPoint, 0) | 0, count = Math.max(1, num(init.count, 1) | 0);
          const angle = (this.sequence / count) * TWO_PI;
          this.sequence = (this.sequence + 1) % count;
          const center = cp >= 0 && cp < 8 ? this.controlPointPosition(cp) : [0, 0, 0];
          p.position = center.slice();
          const sp = randVec(vec(init.speedMin, 3, [0, 0, 0]), vec(init.speedMax, 3, [100, 100, 100]));
          sp[1] = -sp[1];
          const c = Math.cos(angle), s = Math.sin(angle);
          p.velocity = [(c * sp[0] - s * sp[1]) * speedMul, (s * sp[0] + c * sp[1]) * speedMul, sp[2] * speedMul];
          break;
        }
        default: break;
      }
    }

    emit(dt) {
      const def = this.def;
      const particles = this.particles;
      for (let e = 0; e < def.emitters.length; e++) {
        const em = def.emitters[e];
        const st = this.emitterStates[e];
        if (this.count >= particles.length) return;
        if (st.delay > 0) { st.delay -= dt; continue; }
        if (em.duration > 0) { st.duration += dt; if (st.duration >= em.duration) continue; }
        if (em.flags & 4) {
          st.periodicTimer += dt;
          if (!st.emitting) {
            if (st.periodicTimer >= st.periodicDelay) { st.emitting = true; st.periodicTimer = 0; st.periodicDuration = rand(em.minPeriodicDuration, em.maxPeriodicDuration); }
            else continue;
          } else if (st.periodicTimer >= st.periodicDuration) {
            st.emitting = false; st.periodicTimer = 0; st.periodicDelay = rand(em.minPeriodicDelay, em.maxPeriodicDelay);
            continue;
          }
        }
        let rate = em.rate * num(def.instance.rate, 1);
        if (em.audioMode !== 0) rate *= audioLevel(this.audio(), em.audioStart, em.audioEnd, em.audioBounds, em.audioExponent);
        let toEmit = 0;
        if (em.instantaneous > 0 && !st.instantaneousDone) { toEmit = em.instantaneous; st.instantaneousDone = true; }
        if (em.rate > 0 && this.emitting) {
          st.timer += dt * rate;
          let n = Math.floor(st.timer);
          st.timer -= n;
          if ((em.flags & 2) && n > 1) n = 1;
          toEmit += n;
        }
        if (e === 0 && this.forcedEmit > 0) { toEmit += this.forcedEmit; this.forcedEmit = 0; }
        if (!this.emitting && !(em.instantaneous > 0)) toEmit = Math.min(toEmit, 0);
        let cpIndex = em.controlPoint;
        if (cpIndex === -1 && def.controlPoints.length && (def.controlPoints[0].flags & 1)) cpIndex = 0;
        const origin = [em.origin[0], -em.origin[1], em.origin[2]];
        const spawnOrigin = origin.slice();
        if (cpIndex >= 0 && cpIndex < 8) { const cp = this.controlPointPosition(cpIndex); spawnOrigin[0] += cp[0]; spawnOrigin[1] += cp[1]; spawnOrigin[2] += cp[2]; }
        for (let i = 0; i < toEmit && this.count < particles.length; i++) {
          const p = particles[this.count];
          let randomPos;
          if (em.name === 'sphererandom') {
            if ((def.flags & 4) === 0) {
              const angle = rand(0, TWO_PI);
              const minR = em.distanceMin[0], maxR = em.distanceMax[0];
              const r = Math.sqrt(rand(minR * minR, maxR * maxR));
              randomPos = [r * Math.cos(angle) * em.directions[0], r * Math.sin(angle) * em.directions[1], rand(-maxR, maxR) * em.directions[2]];
            } else {
              const theta = rand(0, TWO_PI), cosT = rand(-1, 1), sinT = Math.sqrt(1 - cosT * cosT);
              const minR = em.distanceMin[0], maxR = em.distanceMax[0];
              const r = Math.cbrt(rand(minR ** 3, maxR ** 3));
              randomPos = [sinT * Math.cos(theta) * r * em.directions[0], sinT * Math.sin(theta) * r * em.directions[1], cosT * r * em.directions[2]];
            }
            for (let k = 0; k < 3; k++) {
              if (em.sign[k] === 1) randomPos[k] = Math.abs(randomPos[k]);
              else if (em.sign[k] === -1) randomPos[k] = -Math.abs(randomPos[k]);
            }
            this.spawn(p, spawnOrigin, randomPos, em);
          } else {
            randomPos = [0, 0, 0];
            for (let axis = 0; axis < 3; axis++) {
              let d = rand(em.distanceMin[axis], em.distanceMax[axis]);
              if (Math.random() < 0.5) d = -d;
              randomPos[axis] = d * em.directions[axis] * (axis === 1 ? -1 : 1);
            }
            this.spawn(p, spawnOrigin, randomPos, null);
          }
          this.count++;
        }
      }
    }

    operate(op, dt) {
      const particles = this.particles, count = this.count;
      const speedMul = num(this.def.instance.speed, 1);
      switch (op.name) {
        case 'movement': {
          const drag = num(op.drag, 0), g = vec(op.gravity, 3, [0, 0, 0]);
          const gravity = [g[0], -g[1], g[2]];
          const df = Math.max(0, 1 - drag * dt);
          for (let i = 0; i < count; i++) {
            const p = particles[i]; if (!p.alive) continue;
            p.position[0] += p.velocity[0] * dt; p.position[1] += p.velocity[1] * dt; p.position[2] += p.velocity[2] * dt;
            p.velocity[0] = (p.velocity[0] + gravity[0] * dt * speedMul) * df;
            p.velocity[1] = (p.velocity[1] + gravity[1] * dt * speedMul) * df;
            p.velocity[2] = (p.velocity[2] + gravity[2] * dt * speedMul) * df;
          }
          break;
        }
        case 'angularmovement': {
          const drag = num(op.drag, 0), force = vec(op.force, 3, [0, 0, 0]);
          const df = Math.max(0, 1 - drag * dt);
          for (let i = 0; i < count; i++) {
            const p = particles[i]; if (!p.alive) continue;
            for (let k = 0; k < 3; k++) {
              p.rotation[k] += p.angularVelocity[k] * dt * speedMul;
              p.angularVelocity[k] = (p.angularVelocity[k] + force[k] * dt * speedMul) * df;
              while (p.rotation[k] > Math.PI) p.rotation[k] -= TWO_PI;
              while (p.rotation[k] < -Math.PI) p.rotation[k] += TWO_PI;
            }
          }
          break;
        }
        case 'alphafade': {
          const fin = num(op.fadeInTime, 0.5), fout = num(op.fadeOutTime, 0.5);
          for (let i = 0; i < count; i++) {
            const p = particles[i]; if (!p.alive) continue;
            const life = p.lifetime > 0 ? p.age / p.lifetime : 1;
            if (life <= fin) p.alpha = p.initial.alpha * fade(life, 0, fin, 0, 1);
            else if (life > fout) p.alpha = p.initial.alpha * (1 - fade(life, fout, 1, 0, 1));
            else p.alpha = p.initial.alpha;
            if (p.oscAlpha) p.oscAlpha.base = p.alpha;
          }
          break;
        }
        case 'sizechange': case 'alphachange': {
          const st = num(op.startTime, 0), et = num(op.endTime, 1), sv = num(op.startValue, 1), ev = num(op.endValue, 0);
          for (let i = 0; i < count; i++) {
            const p = particles[i]; if (!p.alive) continue;
            const life = p.lifetime > 0 ? p.age / p.lifetime : 1;
            const mul = fade(life, st, et, sv, ev);
            if (op.name === 'sizechange') { p.size = p.initial.size * mul; if (p.oscSize) p.oscSize.base = p.size; }
            else { p.alpha = p.initial.alpha * mul; if (p.oscAlpha) p.oscAlpha.base = p.alpha; }
          }
          break;
        }
        case 'colorchange': {
          const st = num(op.startTime, 0), et = num(op.endTime, 1), sv = vec(op.startValue, 3, [1, 1, 1]), ev = vec(op.endValue, 3, [1, 1, 1]);
          for (let i = 0; i < count; i++) {
            const p = particles[i]; if (!p.alive) continue;
            const life = p.lifetime > 0 ? p.age / p.lifetime : 1;
            for (let k = 0; k < 3; k++) p.color[k] = p.initial.color[k] * fade(life, st, et, sv[k], ev[k]);
          }
          break;
        }
        case 'turbulence': {
          const state = this.turbulence[this.def.operators.filter((o) => o.name === 'turbulence').indexOf(op)];
          if (!state || state.speed <= 1e-4) break;
          let strength = 1;
          if (num(op.audioMode, 0) !== 0) strength = 1 + audioLevel(this.audio(), num(op.audioStart, 0), num(op.audioEnd, 15), vec(op.audioBounds, 2, [0, 1]), num(op.audioExponent, 1));
          const noiseScale = num(op.scale, 0.005) * 2, timeScale = num(op.timeScale, 0.01), mask = vec(op.mask, 3, [1, 1, 0]);
          for (let i = 0; i < count; i++) {
            const p = particles[i]; if (!p.alive) continue;
            const np = [(p.position[0] + state.phase + timeScale * this.time) * noiseScale, p.position[1] * noiseScale, p.position[2] * noiseScale];
            let c = Noise.curl(np);
            const len = Math.hypot(...c);
            if (len > 1e-4) c = c.map((x) => (x / len) * state.speed * strength);
            p.velocity[0] += c[0] * mask[0] * dt * speedMul; p.velocity[1] += c[1] * mask[1] * dt * speedMul; p.velocity[2] += c[2] * mask[2] * dt * speedMul;
          }
          break;
        }
        case 'vortex': {
          let amp = 1;
          if (num(op.audioMode, 0) !== 0) { amp = audioLevel(this.audio(), 0, 15, vec(op.audioBounds, 2, [0, 1]), 1); if (amp === 0) break; amp = 1 + amp; }
          let axis = vec(op.axis, 3, [0, 0, 1]);
          const al = Math.hypot(...axis);
          axis = al > 0 ? axis.map((x) => x / al) : [0, 0, 1];
          const offset = vec(op.offset, 3, [0, 0, 0]);
          const cp = op.controlPoint;
          const base = cp >= 0 && cp < 8 ? this.controlPointPosition(cp) : [0, 0, 0];
          const center = [base[0] + offset[0], base[1] + offset[1], base[2] + offset[2]];
          const dIn = num(op.distanceInner, 500), dOut = num(op.distanceOuter, 650), sIn = num(op.speedInner, 2500) * amp, sOut = num(op.speedOuter, 0) * amp;
          const centerForce = num(op.centerForce, 1), ringRadius = num(op.ringRadius, 300), ringWidth = num(op.ringWidth, 50), ringPull = num(op.ringPullDistance, 50), ringForce = num(op.ringPullForce, 10);
          const infinite = (op.flags & 1) !== 0, maintain = (op.flags & 2) !== 0, ring = (op.flags & 4) !== 0;
          for (let i = 0; i < count; i++) {
            const p = particles[i]; if (!p.alive) continue;
            const to = [p.position[0] - center[0], p.position[1] - center[1], p.position[2] - center[2]];
            let radial = to;
            if (infinite) { const d = to[0] * axis[0] + to[1] * axis[1] + to[2] * axis[2]; radial = [to[0] - axis[0] * d, to[1] - axis[1] * d, to[2] - axis[2] * d]; }
            const dist = Math.hypot(...radial);
            let tangent = [axis[1] * radial[2] - axis[2] * radial[1], axis[2] * radial[0] - axis[0] * radial[2], axis[0] * radial[1] - axis[1] * radial[0]];
            const tl = Math.hypot(...tangent);
            if (tl <= 1e-3) continue;
            tangent = tangent.map((x) => x / tl);
            let speed = 0;
            let radialForce = [0, 0, 0];
            if (ring) {
              const inner = ringRadius - ringWidth / 2, outer = ringRadius + ringWidth / 2;
              if (dist < inner) speed = 0;
              else if (dist <= outer) { const t = (dist - inner) / ringWidth; speed = sIn + (sOut - sIn) * t; }
              else if (dist <= outer + ringPull) {
                const t = (dist - outer) / ringPull;
                speed = sOut * (1 - t);
                if (dist > 1e-3) radialForce = radial.map((x) => (-x / dist) * ringForce * t);
              }
            } else {
              const mid = dOut - dIn + 0.1;
              if (mid < 0 || dist < dIn) speed = sIn;
              else if (dist > dOut) speed = sOut;
              else speed = sIn + (sOut - sIn) * ((dist - dIn) / mid);
            }
            for (let k = 0; k < 3; k++) p.velocity[k] += (tangent[k] * speed + radialForce[k]) * dt * speedMul;
            if (maintain && dist > 1e-3) for (let k = 0; k < 3; k++) p.velocity[k] += (-radial[k] / dist) * centerForce * dt * speedMul;
          }
          break;
        }
        case 'controlpointattract': {
          const cp = op.controlPoint;
          if (cp < 0 || cp >= 8) break;
          const origin = vec(op.origin, 3, [0, 0, 0]), scale = num(op.scale, 100), threshold = num(op.threshold, 1000) / 2;
          const base = this.controlPointPosition(cp);
          const center = [base[0] + origin[0], base[1] + origin[1], base[2] + origin[2]];
          for (let i = 0; i < count; i++) {
            const p = particles[i]; if (!p.alive) continue;
            const to = [center[0] - p.position[0], center[1] - p.position[1], center[2] - p.position[2]];
            const dist = Math.hypot(...to);
            if (dist > 1e-3 && dist < threshold) for (let k = 0; k < 3; k++) p.velocity[k] += (to[k] / dist) * scale * dt * speedMul;
          }
          break;
        }
        case 'oscillatealpha': case 'oscillatesize': {
          const fMin = num(op.frequencyMin, 0), fMax = num(op.frequencyMax, 10), sMin = num(op.scaleMin, 0), sMax = num(op.scaleMax, 1), pMin = num(op.phaseMin, 0), pMax = num(op.phaseMax, TWO_PI);
          const key = op.name === 'oscillatealpha' ? 'oscAlpha' : 'oscSize';
          for (let i = 0; i < count; i++) {
            const p = particles[i];
            if (!p[key]) p[key] = { frequency: rand(fMin, fMax), scale: rand(sMin, sMax), phase: rand(pMin, pMax + TWO_PI), base: key === 'oscAlpha' ? p.alpha : p.size };
            const o = p[key];
            const cosVal = (Math.cos(o.frequency * p.age + o.phase) + 1) * 0.5;
            const mul = sMin + (sMax - sMin) * cosVal;
            if (key === 'oscAlpha') p.alpha = o.base * mul; else p.size = o.base * mul;
          }
          break;
        }
        case 'oscillateposition': {
          const fMin = num(op.frequencyMin, 0), fMax = num(op.frequencyMax, 5), sMin = num(op.scaleMin, 0), sMax = num(op.scaleMax, 10), pMin = num(op.phaseMin, 0), pMax = num(op.phaseMax, TWO_PI);
          const mask = vec(op.mask, 3, [1, 1, 0]);
          for (let i = 0; i < count; i++) {
            const p = particles[i];
            if (!p.oscPos) p.oscPos = { frequency: [rand(fMin, fMax), rand(fMin, fMax), rand(fMin, fMax)], scale: [rand(sMin, sMax), rand(sMin, sMax), rand(sMin, sMax)], phase: [rand(pMin, pMax + TWO_PI), rand(pMin, pMax + TWO_PI), rand(pMin, pMax + TWO_PI)] };
            const o = p.oscPos;
            for (let k = 0; k < 3; k++) {
              const w = o.frequency[k];
              const move = -o.scale[k] * w * Math.sin(w * p.age + o.phase[k]) * dt;
              p.position[k] += move * mask[k] * speedMul;
            }
          }
          break;
        }
        default: break;
      }
    }

    update(dt, time, mouse) {
      this.time = time;
      if (!this.playing) return;
      const w = this.sceneWidth, h = this.sceneHeight;
      this.refreshOrigin();
      if (mouse) {
        for (const cp of this.controlPoints) {
          if (!cp.linkMouse || cp.mirrored) continue;
          const pos = [mouse[0] * w - w / 2 + cp.offset[0], h / 2 - mouse[1] * h + cp.offset[1], cp.offset[2]];
          cp.position = [pos[0] - this.transformedOrigin[0], pos[1] - this.transformedOrigin[1], pos[2] - this.transformedOrigin[2]];
        }
      }
      this.emit(dt);
      for (let i = 0; i < this.count; i++) this.particles[i].age += dt;
      for (const op of this.def.operators) this.operate(op, dt);
      const ss = this.spritesheet;
      if (ss && ss.frames > 0) {
        const speed = this.def.sequenceMultiplier > 0 ? this.def.sequenceMultiplier : 1;
        for (let i = 0; i < this.count; i++) {
          const p = this.particles[i];
          const life = p.lifetime > 0 ? p.age / p.lifetime : 1;
          if (this.def.animationMode === 'randomframe') { if (p.frame < 0) p.frame = Math.floor(Math.random() * ss.frames); }
          else if (this.def.animationMode === 'once') p.frame = Math.min(life * ss.frames * speed, ss.frames - 1);
          else if (ss.duration > 0) { const cycle = (p.age * speed) % ss.duration; p.frame = ((cycle / ss.duration) * ss.frames) % ss.frames; }
          else p.frame = (life * ss.frames * speed) % ss.frames;
        }
      }
      let write = 0;
      for (let read = 0; read < this.count; read++) {
        const p = this.particles[read];
        if (p.alive && p.age < p.lifetime) {
          if (write !== read) { const tmp = this.particles[write]; this.particles[write] = p; this.particles[read] = tmp; }
          write++;
        } else p.alive = false;
      }
      this.count = write;
    }

    // Sprite quads in the layout genericparticle.vert reads.
    buildSprites() {
      const ss = this.spritesheet;
      const verts = new Float32Array(this.count * 4 * SPRITE_FLOATS);
      const indices = new Uint32Array(this.count * 6);
      let vi = 0, ii = 0;
      for (let i = 0; i < this.count; i++) {
        const p = this.particles[i];
        if (!p.alive) continue;
        if (![p.position[0], p.position[1], p.position[2], p.size].every(Number.isFinite) || p.size <= 0 || p.size > 10000) continue;
        let lifetime = p.lifetime > 0 ? p.age / p.lifetime : 1;
        if (ss && ss.frames > 0 && p.frame >= 0) lifetime = this.def.animationMode === 'randomframe' ? (p.frame + 0.5) / ss.frames : p.frame / ss.frames;
        const base = vi;
        const add = (u, v) => {
          const o = vi * SPRITE_FLOATS;
          verts[o] = p.position[0]; verts[o + 1] = p.position[1]; verts[o + 2] = p.position[2];
          verts[o + 3] = u; verts[o + 4] = v; verts[o + 5] = p.rotation[2]; verts[o + 6] = p.size;
          verts[o + 7] = p.color[0]; verts[o + 8] = p.color[1]; verts[o + 9] = p.color[2]; verts[o + 10] = p.alpha;
          verts[o + 11] = p.velocity[0]; verts[o + 12] = p.velocity[1]; verts[o + 13] = p.velocity[2]; verts[o + 14] = lifetime;
          verts[o + 15] = p.rotation[0]; verts[o + 16] = p.rotation[1];
          vi++;
        };
        add(0, 1); add(1, 1); add(1, 0); add(0, 0);
        indices[ii++] = base; indices[ii++] = base + 1; indices[ii++] = base + 2;
        indices[ii++] = base + 2; indices[ii++] = base + 3; indices[ii++] = base;
      }
      return { vertices: verts.subarray(0, vi * SPRITE_FLOATS), indices: indices.subarray(0, ii), floats: SPRITE_FLOATS, indexCount: ii };
    }

    // Rope segments with Catmull-Rom smoothing in the layout genericropeparticle.vert reads.
    buildRope(time) {
      const alive = this.count;
      if (alive < 2) return { vertices: new Float32Array(0), indices: new Uint32Array(0), floats: ROPE_FLOATS, indexCount: 0 };
      const r = this.renderer;
      const subdivision = Math.max(1, r.subdivision | 0);
      const segments = alive - 1;
      const total = segments * subdivision + 1;
      const pos = new Array(total), sizes = new Float32Array(total), colors = new Array(total);
      const cr = (p0, p1, p2, p3, t) => {
        const t2 = t * t, t3 = t2 * t;
        return [0, 1, 2].map((k) => 0.5 * (2 * p1[k] + (-p0[k] + p2[k]) * t + (2 * p0[k] - 5 * p1[k] + 4 * p2[k] - p3[k]) * t2 + (-p0[k] + 3 * p1[k] - 3 * p2[k] + p3[k]) * t3));
      };
      for (let i = 0; i < segments; i++) {
        const p1 = this.particles[i], p2 = this.particles[i + 1];
        const p0 = i > 0 ? this.particles[i - 1] : p1, p3 = i + 2 < alive ? this.particles[i + 2] : p2;
        for (let k = 0; k < subdivision; k++) {
          const t = k / subdivision, idx = i * subdivision + k;
          pos[idx] = cr(p0.position, p1.position, p2.position, p3.position, t);
          sizes[idx] = p1.size + (p2.size - p1.size) * t;
          colors[idx] = [p1.color[0] + (p2.color[0] - p1.color[0]) * t, p1.color[1] + (p2.color[1] - p1.color[1]) * t, p1.color[2] + (p2.color[2] - p1.color[2]) * t, p1.alpha + (p2.alpha - p1.alpha) * t];
        }
      }
      const last = this.particles[alive - 1];
      pos[total - 1] = last.position.slice(); sizes[total - 1] = last.size; colors[total - 1] = [last.color[0], last.color[1], last.color[2], last.alpha];
      const subSegments = total - 1;
      const uvScale = r.uvScale > 0 ? r.uvScale : 1;
      const trailLength = subSegments / uvScale + 1;
      const usable = trailLength - 1;
      const smoothing = r.uvSmoothing && this.uniformLifetimes && !r.uvScrolling;
      let arc = null, totalArc = 0;
      if (smoothing) {
        arc = new Float32Array(total);
        for (let i = 1; i < total; i++) { totalArc += Math.hypot(pos[i][0] - pos[i - 1][0], pos[i][1] - pos[i - 1][1], pos[i][2] - pos[i - 1][2]); arc[i] = totalArc; }
      }
      const scroll = r.uvScrolling && usable > 0 ? (time % 10000) * usable : 0;
      const verts = new Float32Array(subSegments * 4 * ROPE_FLOATS);
      const indices = new Uint32Array(subSegments * 6);
      let vi = 0, ii = 0;
      for (let s = 0; s < subSegments; s++) {
        const a = pos[s], b = pos[s + 1], prev = s > 0 ? pos[s - 1] : a, next = s + 2 < total ? pos[s + 2] : b;
        const ca = colors[s], cb = colors[s + 1];
        let trailPos = smoothing && totalArc > 0 ? (arc[s] / totalArc) * subSegments : s;
        trailPos += scroll;
        const base = vi;
        const add = (u, v) => {
          const o = vi * ROPE_FLOATS;
          verts[o] = a[0]; verts[o + 1] = a[1]; verts[o + 2] = a[2]; verts[o + 3] = sizes[s];
          verts[o + 4] = b[0]; verts[o + 5] = b[1]; verts[o + 6] = b[2]; verts[o + 7] = trailLength;
          verts[o + 8] = prev[0]; verts[o + 9] = prev[1]; verts[o + 10] = prev[2]; verts[o + 11] = trailPos;
          verts[o + 12] = next[0]; verts[o + 13] = next[1]; verts[o + 14] = next[2]; verts[o + 15] = sizes[s + 1];
          verts[o + 16] = cb[0]; verts[o + 17] = cb[1]; verts[o + 18] = cb[2]; verts[o + 19] = cb[3];
          verts[o + 20] = u; verts[o + 21] = v;
          verts[o + 22] = ca[0]; verts[o + 23] = ca[1]; verts[o + 24] = ca[2]; verts[o + 25] = ca[3];
          vi++;
        };
        add(0, 0); add(1, 0); add(1, 1); add(0, 1);
        indices[ii++] = base; indices[ii++] = base + 1; indices[ii++] = base + 2;
        indices[ii++] = base + 2; indices[ii++] = base + 3; indices[ii++] = base;
      }
      return { vertices: verts.subarray(0, vi * ROPE_FLOATS), indices: indices.subarray(0, ii), floats: ROPE_FLOATS, indexCount: ii };
    }

    // The system's local frame in the scene's centred space: origin, parallax, angles, scale.
    modelMatrix(parallaxOffset) {
      const scale = vec(this.scale, 3, [1, 1, 1]);
      const angles = vec(this.angles, 3, [0, 0, 0]);
      let model = M.translation(this.transformedOrigin[0], this.transformedOrigin[1], this.transformedOrigin[2]);
      if (parallaxOffset) model = M.translate(model, parallaxOffset[0], parallaxOffset[1], 0);
      model = M.rotate(model, -angles[2], 0, 0, 1);
      model = M.rotate(model, angles[1], 0, 1, 0);
      model = M.rotate(model, -angles[0], 1, 0, 0);
      return M.scale(model, scale[0], scale[1], scale[2]);
    }

    // CParticle::updateMatrices + updateParticleViewProjection: the model matrix from the
    // transformed origin, parallax, angles and scale; perspective systems (flags & 4) project
    // through the camera's field of view from an eye 1000 units out, others orthographically.
    matrices(scene, parallaxOffset) {
      const camera = scene.camera;
      const model = this.modelMatrix(parallaxOffset);
      let viewProjection, eye;
      if (this.def.flags & 4) {
        const aspect = camera.width / camera.height;
        const fov = camera.fov.getNumber() * Math.PI / 180;
        viewProjection = M.multiply(M.perspective(fov, aspect, camera.nearz.getNumber(), camera.farz.getNumber()), M.lookAt([0, 0, 1000], [0, 0, 0], [0, 1, 0]));
        eye = [0, 0, 1000];
      } else {
        viewProjection = camera.viewProjection;
        eye = [0, 0, 1000];
      }
      const mvp = M.multiply(viewProjection, model);
      return { model, modelInverse: M.inverse(model), mvp, mvpInverse: M.inverse(mvp), viewProjection, eye };
    }

    renderVars(texture) {
      const r = this.renderer;
      const var0 = [r.length, r.maxLength, r.minLength, 0];
      let var1;
      const ss = this.spritesheet;
      if (ss && ss.frames > 0 && ss.cols > 0 && ss.rows > 0) {
        const fw = 1 / ss.cols, fh = 1 / ss.rows;
        let ratio = 1;
        if (texture) { const w = texture.resolution[0], h = texture.resolution[1]; if (w > 0) ratio = (h * fh) / (w * fw); }
        var1 = [fw, fh, ss.frames, ratio];
      } else {
        let ratio = 1;
        if (texture && texture.realWidth > 0) ratio = texture.realHeight / texture.realWidth;
        var1 = [0, 0, 0, ratio];
      }
      return { var0, var1 };
    }
  }

  const api = { parseDefinition, System, SPRITE_FLOATS, ROPE_FLOATS, audioLevel };
  G.WEParticles = api;
  if (typeof module !== 'undefined' && module.exports) module.exports = api;
})();
