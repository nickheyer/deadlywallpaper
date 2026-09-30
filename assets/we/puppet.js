// Wallpaper Engine puppet models (`.mdl`): meshes, bones, attachments, animations, IK rigs
// and bone physics, plus the per-frame skinning matrices the shaders' SKINNING path reads.
(() => {
  'use strict';
  const G = typeof window !== 'undefined' ? window : globalThis;
  const M = G.WEM;

  const NO_PARENT = 0xffffffff;
  const FLAG = { NORMAL: 0x2, TANGENT: 0x4, UV: 0x8, UV2: 0x20, EXTRA4: 0x10000, SKIN_BLEND: 0x800000, SKIN_WEIGHT: 0x1000000 };
  const BONE_FRAME_BYTES = 36;

  class Reader {
    constructor(bytes, name) {
      this.b = bytes;
      this.v = new DataView(bytes.buffer, bytes.byteOffset, bytes.byteLength);
      this.pos = 0;
      this.name = name;
    }
    fail(msg) { throw new Error(this.name + ': ' + msg + ' (offset ' + this.pos + ' of ' + this.b.length + ')'); }
    need(n, what) { if (this.pos + n > this.b.length) this.fail('truncated ' + what); }
    u8(w) { this.need(1, w || 'byte'); return this.b[this.pos++]; }
    u16(w) { this.need(2, w || 'u16'); const x = this.v.getUint16(this.pos, true); this.pos += 2; return x; }
    i16(w) { this.need(2, w || 'i16'); const x = this.v.getInt16(this.pos, true); this.pos += 2; return x; }
    u32(w) { this.need(4, w || 'u32'); const x = this.v.getUint32(this.pos, true); this.pos += 4; return x; }
    i32(w) { this.need(4, w || 'i32'); const x = this.v.getInt32(this.pos, true); this.pos += 4; return x; }
    f32(w) { this.need(4, w || 'float'); const x = this.v.getFloat32(this.pos, true); this.pos += 4; return x; }
    peekU32(at) { if (at < 0 || at + 4 > this.b.length) return null; return this.v.getUint32(at, true); }
    peekU8(at) { if (at < 0 || at + 1 > this.b.length) return null; return this.b[at]; }
    cstring(w) {
      let end = this.pos;
      while (end < this.b.length && this.b[end] !== 0) end++;
      if (end >= this.b.length) this.fail('unterminated ' + (w || 'string'));
      const s = new TextDecoder().decode(this.b.subarray(this.pos, end));
      this.pos = end + 1;
      return s;
    }
    tag() {
      this.need(9, 'block tag');
      let s = '';
      for (let i = 0; i < 8; i++) s += String.fromCharCode(this.b[this.pos + i]);
      this.pos += 9;
      return s;
    }
    peekTag(prefix) {
      if (this.pos + 9 > this.b.length) return false;
      for (let i = 0; i < prefix.length; i++) if (this.b[this.pos + i] !== prefix.charCodeAt(i)) return false;
      return true;
    }
    version(prefix) {
      const t = this.tag();
      if (!t.startsWith(prefix)) this.fail('expected a ' + prefix + ' tag, found ' + JSON.stringify(t));
      const n = parseInt(t.slice(4), 10);
      if (!Number.isFinite(n)) this.fail('unreadable ' + prefix + ' version ' + JSON.stringify(t));
      return n;
    }
    mat4() {
      const m = new Float32Array(16);
      for (let i = 0; i < 16; i++) m[i] = this.f32('matrix');
      return m;
    }
    get remaining() { return this.b.length - this.pos; }
  }

  function strideOf(flag) {
    let s = 12;
    if (flag & FLAG.NORMAL) s += 12;
    if (flag & FLAG.TANGENT) s += 16;
    if (flag & FLAG.EXTRA4) s += 4;
    if (flag & FLAG.SKIN_BLEND) s += 16;
    if (flag & FLAG.SKIN_WEIGHT) s += 16;
    if (flag & (FLAG.UV | FLAG.UV2)) s += 8;
    if (flag & FLAG.UV2) s += 8;
    return s;
  }

  function parseMesh(r, header) {
    const mesh = { materials: [], positions: null, normals: null, tangents: null, blendIndices: null, blendWeights: null, uvs: null, uv2: null, indices: null, parts: [], masks: [], aabb: null };
    for (let i = 0; i < header.skinCount; i++) mesh.materials.push(r.cstring('material name'));
    mesh.flagA = r.u32('mesh flag');
    if (mesh.flagA === 2) r.u32('mesh flag extra');
    if (header.mdlv >= 17) {
      mesh.aabb = { min: [r.f32(), r.f32(), r.f32()], max: [r.f32(), r.f32(), r.f32()] };
    }
    const flag = header.mdlv > 14 ? r.u32('vertex layout') : header.flag;
    mesh.flag = flag;
    const vertexBytes = r.u32('vertex bytes');
    const stride = strideOf(flag);
    if (vertexBytes % stride !== 0) r.fail('vertex data of ' + vertexBytes + ' bytes is not a multiple of the ' + stride + '-byte layout 0x' + flag.toString(16));
    const count = vertexBytes / stride;
    r.need(vertexBytes, 'vertices');
    mesh.count = count;
    mesh.positions = new Float32Array(count * 3);
    if (flag & FLAG.NORMAL) mesh.normals = new Float32Array(count * 3);
    if (flag & FLAG.TANGENT) mesh.tangents = new Float32Array(count * 4);
    if (flag & FLAG.SKIN_BLEND) mesh.blendIndices = new Uint32Array(count * 4);
    if (flag & FLAG.SKIN_WEIGHT) mesh.blendWeights = new Float32Array(count * 4);
    if (flag & (FLAG.UV | FLAG.UV2)) mesh.uvs = new Float32Array(count * 2);
    if (flag & FLAG.UV2) mesh.uv2 = new Float32Array(count * 2);
    for (let i = 0; i < count; i++) {
      mesh.positions[i * 3] = r.f32(); mesh.positions[i * 3 + 1] = r.f32(); mesh.positions[i * 3 + 2] = r.f32();
      if (mesh.normals) { mesh.normals[i * 3] = r.f32(); mesh.normals[i * 3 + 1] = r.f32(); mesh.normals[i * 3 + 2] = r.f32(); }
      if (mesh.tangents) for (let k = 0; k < 4; k++) mesh.tangents[i * 4 + k] = r.f32();
      if (flag & FLAG.EXTRA4) r.u32();
      if (mesh.blendIndices) for (let k = 0; k < 4; k++) mesh.blendIndices[i * 4 + k] = r.u32();
      if (mesh.blendWeights) for (let k = 0; k < 4; k++) mesh.blendWeights[i * 4 + k] = r.f32();
      if (mesh.uvs) { mesh.uvs[i * 2] = r.f32(); mesh.uvs[i * 2 + 1] = r.f32(); }
      if (mesh.uv2) { mesh.uv2[i * 2] = r.f32(); mesh.uv2[i * 2 + 1] = r.f32(); }
    }
    const indexBytes = r.u32('index bytes');
    const wide = header.mdlv >= 23 && count > 65535;
    const indexStride = wide ? 12 : 6;
    if (indexBytes % indexStride !== 0) r.fail('index data of ' + indexBytes + ' bytes is not a multiple of ' + indexStride);
    r.need(indexBytes, 'indices');
    const indexCount = (indexBytes / indexStride) * 3;
    mesh.indices = new Uint32Array(indexCount);
    for (let i = 0; i < indexCount; i++) mesh.indices[i] = wide ? r.u32() : r.u16();
    for (let i = 0; i < indexCount; i++) if (mesh.indices[i] >= count) r.fail('index ' + mesh.indices[i] + ' exceeds ' + count + ' vertices');
    if (header.mdlv >= 21) {
      const unkA = r.u8('parts marker');
      if (unkA === 1) {
        const unkB = r.u8();
        if (unkB) {
          r.u16();
          r.u8();
          const payload = r.u32('part uv bytes');
          if (payload !== 12 * count) r.fail('part uv payload of ' + payload + ' bytes for ' + count + ' vertices');
          mesh.partUv2 = new Float32Array(count * 2);
          for (let i = 0; i < count; i++) { mesh.partUv2[i * 2] = r.f32(); mesh.partUv2[i * 2 + 1] = r.f32(); r.u32(); }
        }
      } else if (unkA !== 0) {
        r.fail('unknown parts marker ' + unkA);
      }
      const hasParts = r.u8('parts flag');
      if (hasParts) {
        const bytes = r.u32('parts bytes');
        if (bytes % 16 !== 0) r.fail('parts table of ' + bytes + ' bytes');
        for (let i = 0; i < bytes / 16; i++) {
          const id = r.u32(); r.u32(); const start = r.u32(); const size = r.u32();
          mesh.parts.push({ id, start, size });
        }
      }
      if (header.mdlv > 21) {
        const maskCount = r.u32('mask count');
        for (let i = 0; i < maskCount; i++) {
          const leading = r.u32(); r.u32();
          const material = r.cstring('mask material'); r.u32();
          const a = r.u32(); const partsA = []; for (let k = 0; k < a; k++) partsA.push(r.u32());
          const b = r.u32(); const partsB = []; for (let k = 0; k < b; k++) partsB.push(r.u32());
          mesh.masks.push({ leading, material, partsA, partsB });
        }
      }
    }
    return mesh;
  }

  function parseMDLS(r, mdl) {
    const version = r.version('MDLS');
    mdl.mdls = version;
    const end = r.u32('MDLS end');
    if (end < r.pos || end > r.b.length) r.fail('MDLS block end ' + end + ' out of range');
    const boneCount = r.u16('bone count');
    r.u16();
    const puppet = { bones: [], attachments: [], animations: [], ik: null, worldAnchored: mdl.header.mdlv === 21 };
    mdl.puppet = puppet;
    for (let i = 0; i < boneCount; i++) {
      const bone = { index: i, name: r.cstring('bone name'), simType: r.i32('bone simulation type') };
      let parent = r.u32('bone parent');
      if (parent >= i && parent !== NO_PARENT) parent = NO_PARENT;
      bone.parent = parent;
      const size = r.u32('bone matrix size');
      if (size !== 64) r.fail('bone ' + i + ' has a ' + size + '-byte matrix');
      bone.localBind = r.mat4();
      bone.simulationJson = r.cstring('bone simulation JSON');
      bone.simulation = parseSimulation(bone.simulationJson, r.name, bone);
      puppet.bones.push(bone);
    }
    if (version > 1) {
      const extras = r.u16('MDLS extras');
      if (version === 2) {
        r.pos += extras * (1 + 4 + 4 + 64);
        if (r.u8()) r.pos += boneCount * 64;
        r.pos += 4;
        const nodes = r.u16();
        r.pos += nodes * 4;
        for (let i = 0; i < nodes; i++) r.pos += r.u16() * 16;
        const chains = r.u16();
        for (let i = 0; i < chains; i++) { r.pos += 36; r.pos += r.u16() * 4; }
      } else if (extras !== 0) {
        if (mdl.header.mdlv !== 23 || version !== 4) r.fail('IK controllers in an MDLV' + mdl.header.mdlv + '/MDLS' + version + ' file have no known layout');
        parseIk(r, puppet, extras, boneCount, end);
      } else {
        r.u8(); r.u32(); r.u32();
      }
      if (r.pos < end) {
        if (r.u8('offset transform flag')) {
          for (let i = 0; i < boneCount; i++) {
            const bone = puppet.bones[i];
            bone.skinPivot = [r.f32(), r.f32(), r.f32()];
            bone.skinMat = r.mat4();
          }
        }
        if (r.u8('bone index flag')) r.pos += boneCount * 4;
        if (version >= 3 && r.u8('bone depth flag')) r.pos += boneCount * 4;
      }
    }
    if (end > 0) r.pos = end;
  }

  function parseIk(r, puppet, controllerCount, boneCount, end) {
    const ik = { controllers: [], nodes: [], chains: [] };
    for (let i = 0; i < controllerCount; i++) {
      r.u8();
      const c = { bone: r.u32('controller bone'), type: r.u32('controller type'), bind: r.mat4() };
      if (c.bone >= boneCount || c.type > 1) r.fail('IK controller ' + i + ' refers to bone ' + c.bone + ' with type ' + c.type);
      ik.controllers.push(c);
    }
    r.u8(); r.u32();
    const lengths = r.u16('IK node count');
    if (lengths !== boneCount) r.fail('IK graph lists ' + lengths + ' bones of ' + boneCount);
    for (let i = 0; i < lengths; i++) ik.nodes.push({ length: r.f32('IK bone length'), children: [] });
    for (let p = 0; p < boneCount; p++) {
      const n = r.u16('IK child count');
      for (let k = 0; k < n; k++) ik.nodes[p].children.push({ bone: r.u32(), direction: [r.f32(), r.f32(), r.f32()] });
    }
    const chains = r.u16('IK chain count');
    for (let i = 0; i < chains; i++) {
      const chain = { start: r.u32() };
      r.u32(); chain.target = r.u32(); r.u16(); r.u32(); r.u16(); chain.end = r.u32(); r.u32(); chain.length = r.f32(); r.u32();
      const pathCount = r.u16();
      chain.bones = [];
      for (let k = 0; k < pathCount; k++) chain.bones.push(r.u32());
      if (chain.target >= ik.controllers.length) r.fail('IK chain ' + i + ' targets controller ' + chain.target);
      ik.chains.push(chain);
    }
    puppet.ik = ik;
    if (r.pos > end) r.fail('IK rig overran its block');
  }

  function parseMDAT(r, mdl) {
    r.tag();
    if (!mdl.puppet) r.fail('MDAT block without bones');
    const end = r.u32('MDAT end');
    const n = r.u16('attachment count');
    for (let i = 0; i < n; i++) {
      const a = { bone: r.u16('attachment bone'), name: r.cstring('attachment name'), local: r.mat4() };
      mdl.puppet.attachments.push(a);
    }
    if (end > 0) r.pos = end;
  }

  function parseCurves(r, boneCount) {
    const has = r.u8('curve flag');
    if (!has) return null;
    const out = [];
    for (let b = 0; b < boneCount; b++) {
      r.u32();
      const bytes = r.u32('curve bytes');
      if (bytes % 4 !== 0) r.fail('curve of ' + bytes + ' bytes');
      const values = new Float32Array(bytes / 4);
      for (let i = 0; i < values.length; i++) values[i] = r.f32();
      out.push(values);
    }
    return out;
  }

  function parseAnimation(r, anim, version, end, controllerCount) {
    anim.id = r.i32('animation id');
    r.u32();
    anim.name = r.cstring('animation name');
    if (anim.name === '') anim.name = r.cstring('animation name');
    const mode = r.cstring('animation mode');
    anim.mode = mode === '' ? 'loop' : mode;
    if (!['loop', 'mirror', 'single'].includes(anim.mode)) r.fail('animation "' + anim.name + '" has play mode ' + JSON.stringify(anim.mode));
    anim.fps = r.f32('animation fps');
    anim.length = r.i32('animation length');
    anim.flags = r.u32('animation flags');
    const trackCount = r.u32('bone track count');
    anim.tracks = [];
    for (let t = 0; t < trackCount; t++) {
      r.i32();
      const bytes = r.u32('bone track bytes');
      if (bytes % BONE_FRAME_BYTES !== 0) r.fail('bone track of ' + bytes + ' bytes');
      const frames = bytes / BONE_FRAME_BYTES;
      const data = new Float32Array(frames * 9);
      for (let i = 0; i < data.length; i++) data[i] = r.f32();
      anim.tracks.push({ bone: t, frames, data });
    }
    anim.controllerTracks = [];
    const isMain = (bytes) => bytes > 0 && bytes % 4 === 0 && (bytes === (anim.length + 1) * BONE_FRAME_BYTES || bytes === (anim.length + 1) * 4);
    const isController = (bytes) => bytes === (anim.length + 1) * BONE_FRAME_BYTES;
    if (version >= 3) {
      const transFlag = r.u32('translation flag');
      if (transFlag === 1) {
        if (controllerCount !== 0) r.fail('animation "' + anim.name + '" mixes IK controllers with an unknown track layout');
        const extra = r.u32('extra track bytes');
        if (extra > 0) { r.pos += extra; r.u32(); }
        const main = r.u32('main track bytes');
        r.pos += main;
        if (extra > 0) r.u32();
      } else if (transFlag === 0) {
        const next = r.peekU32(r.pos);
        if (next !== null && isMain(next)) {
          let first = true;
          for (;;) {
            if (!first) r.u32();
            const bytes = r.peekU32(r.pos);
            if (controllerCount !== 0 && isController(bytes)) {
              r.u32();
              const frames = bytes / BONE_FRAME_BYTES;
              const data = new Float32Array(frames * 9);
              for (let i = 0; i < data.length; i++) data[i] = r.f32();
              anim.controllerTracks.push({ frames, data });
            } else {
              r.u32();
              r.pos += bytes;
            }
            first = false;
            const zero = r.peekU32(r.pos), after = r.peekU32(r.pos + 4);
            if (zero === 0 && after !== null && isMain(after)) continue;
            break;
          }
        }
      } else {
        r.fail('animation "' + anim.name + '" has translation flag ' + transFlag);
      }
      if (anim.controllerTracks.length !== controllerCount) r.fail('animation "' + anim.name + '" carries ' + anim.controllerTracks.length + ' IK controller tracks for ' + controllerCount + ' controllers');
      if (controllerCount !== 0) { if (r.u32() !== 0) r.fail('animation "' + anim.name + '" has an unknown IK trailer'); }
      anim.blendCurves = parseCurves(r, trackCount);
    }
    if (version >= 4) {
      const has = r.u8('v4 event flag');
      if (has === 1) {
        const n = r.u32('v4 event count');
        anim.morphEvents = [];
        for (let i = 0; i < n; i++) {
          const ev = { time: r.f32(), curves: [] };
          const curveCount = r.u16();
          ev.flags = r.u16();
          for (let c = 0; c < curveCount; c++) {
            const id = c === 0 ? 0 : r.u16();
            const bytes = r.u32();
            if (bytes % 4 !== 0) r.fail('morph curve of ' + bytes + ' bytes');
            const values = new Float32Array(bytes / 4);
            for (let k = 0; k < values.length; k++) values[k] = r.f32();
            ev.curves.push({ id, values });
          }
          anim.morphEvents.push(ev);
        }
      }
    }
    if (version >= 5) { anim.aabb = { min: [r.f32(), r.f32(), r.f32()], max: [r.f32(), r.f32(), r.f32()] }; }
    if (version === 6) {
      const has = r.peekU8(r.pos);
      if (has === 0) { r.u8(); }
      else if (has === 1 && r.peekU32(r.pos + 1) === 0) { anim.scalarCurves = parseCurves(r, trackCount); }
    }
    if (anim.flags & 0x400) { r.u32(); r.u16(); r.u32(); r.u32(); r.i32(); }
    const eventCount = r.u32('animation event count');
    anim.events = [];
    for (let i = 0; i < eventCount; i++) anim.events.push({ frame: r.u32(), json: r.cstring('animation event') });
    // Optional zero padding before the next record.
    if (end > 0 && r.pos + 12 <= end && r.peekU32(r.pos) === 0) {
      const nextId = r.peekU32(r.pos + 4), after = r.peekU32(r.pos + 8);
      if (nextId !== 0 && nextId <= 100000 && after === 0) r.u32();
    }
  }

  function parseMDLA(r, mdl) {
    const tag = r.tag();
    const version = parseInt(tag.slice(4), 10);
    mdl.mdla = version;
    if (version === 0) return;
    if (!mdl.puppet) r.fail('MDLA block without bones');
    const end = r.u32('MDLA end');
    const n = r.u32('animation count');
    const controllers = mdl.puppet.ik ? mdl.puppet.ik.controllers.length : 0;
    for (let i = 0; i < n; i++) {
      const anim = {};
      parseAnimation(r, anim, version, end, controllers);
      mdl.puppet.animations.push(anim);
    }
    if (end > 0 && r.pos + 4 === end) r.u32();
    if (end > 0) r.pos = end;
  }

  function parseMDMP(r, mdl) {
    r.tag();
    const end = r.u32('MDMP end');
    mdl.morphs = [];
    while (r.pos < end) {
      const count = r.u16();
      const section = { time: r.f32(), event: r.u16(), shapes: [] };
      r.u16();
      for (let i = 0; i < count; i++) {
        const shape = { id: r.u32() };
        r.u32();
        shape.tag = r.cstring('morph tag');
        const length = r.u32();
        shape.hash = r.u32();
        if (length % 6 !== 0) r.fail('morph section of ' + length + ' bytes');
        const vcount = length / 6;
        shape.vertices = new Uint16Array(vcount * 3);
        for (let k = 0; k < vcount * 3; k++) shape.vertices[k] = r.u16();
        if (shape.id === 0) r.pos += length;
        else r.pos += vcount * 2;
        section.shapes.push(shape);
      }
      mdl.morphs.push(section);
    }
    if (end > 0) r.pos = end;
  }

  function parseMDLE(r, mdl) {
    r.tag();
    if (!mdl.puppet) r.fail('MDLE block without bones');
    const end = r.u32('MDLE end');
    const bytes = r.u32('MDLE bytes');
    if (bytes !== mdl.puppet.bones.length * 64) r.fail('MDLE holds ' + bytes + ' bytes for ' + mdl.puppet.bones.length + ' bones');
    for (const bone of mdl.puppet.bones) bone.worldBindFile = r.mat4();
    if (end > 0) r.pos = end;
  }

  // Bone physics settings Wallpaper Engine's editor writes next to each bone.
  function parseSimulation(json, name, bone) {
    const text = (json || '').trim();
    if (text === '' || text === '{}' || text === 'null') return null;
    let data;
    try { data = JSON.parse(text); } catch (e) { throw new Error(name + ': bone "' + bone.name + '" carries simulation settings that are not JSON: ' + text); }
    if (!data || typeof data !== 'object') return null;
    const known = { gravity: 'gravity', stiffness: 'stiffness', damping: 'damping', mass: 'mass', drag: 'drag', windstrength: 'wind', windmultiplier: 'wind', windmultiplier2: 'wind', windangle: 'windAngle', angle: 'angle', anglemin: 'angleMin', anglemax: 'angleMax', limit: 'limit', limits: 'limit', enabled: 'enabled', type: 'type', priority: 'priority', bounce: 'bounce', friction: 'friction', origin: 'origin', length: 'length', radius: 'radius', constraint: 'constraint', constraints: 'constraint', simulationmode: 'mode', mode: 'mode', flags: 'flags', name: 'name', mass2: 'mass' };
    const sim = { enabled: true, gravity: 0, stiffness: 10, damping: 2, mass: 1, angleMin: -Math.PI, angleMax: Math.PI, raw: data };
    const unknown = [];
    for (const [k, v] of Object.entries(data)) {
      const key = known[k.toLowerCase()];
      if (!key) { unknown.push(k); continue; }
      if (key === 'enabled') sim.enabled = !!v;
      else if (typeof v === 'number') sim[key] = v;
      else if (typeof v === 'boolean') sim[key] = v ? 1 : 0;
      else if (typeof v === 'string' && v.trim() !== '' && Number.isFinite(Number(v.trim().split(/\s+/)[0]))) sim[key] = Number(v.trim().split(/\s+/)[0]);
      else sim[key] = v;
    }
    if (unknown.length) throw new Error(name + ': bone "' + bone.name + '" uses simulation settings without a known meaning: ' + unknown.join(', ') + ' in ' + text);
    return sim;
  }

  /** Parse a puppet or model file. */
  function parse(bytes, name) {
    const r = new Reader(bytes, name || 'model');
    const mdl = { header: {}, meshes: [], puppet: null, mdls: 1, mdla: 1, morphs: null };
    mdl.header.mdlv = r.version('MDLV');
    mdl.header.flag = r.u32('layout flag');
    mdl.header.skinCount = r.u32('skin count');
    mdl.header.meshCount = r.u32('mesh count');
    if (mdl.header.skinCount === 0) r.fail('the header names no material');
    if (mdl.header.meshCount === 0 || mdl.header.meshCount > 256) r.fail('the header declares ' + mdl.header.meshCount + ' meshes');
    for (let i = 0; i < mdl.header.meshCount; i++) mdl.meshes.push(parseMesh(r, mdl.header));
    if (r.peekTag('MDLS')) parseMDLS(r, mdl);
    if (r.peekTag('MDAT')) parseMDAT(r, mdl);
    if (r.peekTag('MDLA')) parseMDLA(r, mdl);
    if (r.peekTag('MDMP')) parseMDMP(r, mdl);
    if (r.peekTag('MDLE')) parseMDLE(r, mdl);
    if (mdl.puppet) prepare(mdl);
    return mdl;
  }

  // Bind-pose world matrices and their inverses; the vertex centroid each bone carries.
  function prepare(mdl) {
    const p = mdl.puppet;
    const n = p.bones.length;
    for (let i = 0; i < n; i++) {
      const b = p.bones[i];
      b.bindParent = p.worldAnchored ? NO_PARENT : b.parent;
      b.animParent = b.parent;
      b.worldBind = b.bindParent !== NO_PARENT ? M.multiply(p.bones[b.bindParent].worldBind, b.localBind) : M.clone(b.localBind);
      b.invBind = M.inverse(b.worldBind);
      b.centroid = [0, 0, 0];
    }
    if (mdl.mdls >= 3) {
      const sum = new Float64Array(n * 3), w = new Float64Array(n);
      for (const mesh of mdl.meshes) {
        if (!mesh.blendIndices) continue;
        const slots = mesh.blendWeights ? 4 : 1;
        for (let v = 0; v < mesh.count; v++) {
          for (let k = 0; k < slots; k++) {
            const wt = mesh.blendWeights ? mesh.blendWeights[v * 4 + k] : 1;
            const bi = mesh.blendIndices[v * 4 + k];
            if (wt <= 0 || bi >= n) continue;
            sum[bi * 3] += mesh.positions[v * 3] * wt; sum[bi * 3 + 1] += mesh.positions[v * 3 + 1] * wt; sum[bi * 3 + 2] += mesh.positions[v * 3 + 2] * wt;
            w[bi] += wt;
          }
        }
      }
      for (let i = 0; i < n; i++) {
        if (w[i] <= 0) continue;
        const b = p.bones[i];
        b.centroid = [sum[i * 3] / w[i] - b.localBind[12], sum[i * 3 + 1] / w[i] - b.localBind[13], sum[i * 3 + 2] / w[i] - b.localBind[14]];
      }
    }
    for (const a of p.attachments) {
      if (a.bone >= n) throw new Error('attachment "' + a.name + '" hangs on bone ' + a.bone + ' of ' + n);
    }
  }

  // ---- animation playback -------------------------------------------------------------------
  // Play modes as scene JSON names them, mapped onto the .mdl's own loop / single / mirror.
  const MODE_NAMES = { loop: 'loop', once: 'single', single: 'single', oneshot: 'single', pingpong: 'mirror', mirror: 'mirror' };

  function playMode(anim, override) {
    if (override === undefined || override === null || override === '') return anim.mode;
    const mode = MODE_NAMES[String(override).toLowerCase()];
    if (!mode) throw new Error('animation "' + anim.name + '" is asked to play in mode ' + JSON.stringify(override) + ', which is not loop, once or pingpong');
    return mode;
  }

  function frameOf(anim, time, rate, modeOverride) {
    const len = Math.max(0, anim.length);
    const fps = anim.fps > 0 ? anim.fps : 30;
    const f = time * fps * (rate || 1);
    if (len <= 0) return 0;
    const mode = playMode(anim, modeOverride);
    if (mode === 'single') return Math.min(f, len);
    if (mode === 'mirror') { const p = f % (2 * len); return p <= len ? p : 2 * len - p; }
    return f % len;
  }

  // Animation events between the previous and the current frame, in playback order.
  function eventsBetween(anim, previous, frame) {
    if (!anim.events || !anim.events.length || previous === null) return [];
    const hit = [];
    const inRange = (lo, hi, inclusiveLo) => anim.events.filter((e) => (inclusiveLo ? e.frame >= lo : e.frame > lo) && e.frame <= hi);
    if (frame >= previous) hit.push(...inRange(previous, frame, false));
    else { hit.push(...inRange(previous, anim.length, false)); hit.push(...inRange(0, frame, true)); }
    return hit;
  }

  function eventName(event) {
    try {
      const data = JSON.parse(event.json);
      if (data && typeof data === 'object' && typeof data.name === 'string') return data.name;
    } catch (e) { return event.json; }
    return event.json;
  }

  // Sampled local T/R/S for one bone track at a fractional frame.
  function sampleTrack(track, frame, out) {
    const n = track.frames;
    if (n === 0) return false;
    let a = Math.floor(frame), t = frame - a;
    if (a >= n - 1) { a = n - 1; t = 0; }
    if (a < 0) { a = 0; t = 0; }
    const b = Math.min(n - 1, a + 1);
    const d = track.data;
    for (let k = 0; k < 9; k++) {
      const va = d[a * 9 + k], vb = d[b * 9 + k];
      let v;
      if (k >= 3 && k < 6) {
        let delta = vb - va;
        while (delta > Math.PI) delta -= 2 * Math.PI;
        while (delta < -Math.PI) delta += 2 * Math.PI;
        v = va + delta * t;
      } else v = va + (vb - va) * t;
      out[k] = v;
    }
    return true;
  }

  function localFromTRS(trs, pivot) {
    let m = M.translation(trs[0], trs[1], trs[2]);
    if (pivot) m = M.translate(m, pivot[0], pivot[1], pivot[2]);
    if (trs[5] !== 0) m = M.rotate(m, trs[5], 0, 0, 1);
    if (trs[4] !== 0) m = M.rotate(m, trs[4], 0, 1, 0);
    if (trs[3] !== 0) m = M.rotate(m, trs[3], 1, 0, 0);
    if (trs[6] !== 1 || trs[7] !== 1 || trs[8] !== 1) m = M.scale(m, trs[6], trs[7], trs[8]);
    if (pivot) m = M.translate(m, -pivot[0], -pivot[1], -pivot[2]);
    return m;
  }

  function bindTRS(bone) {
    const m = bone.localBind;
    const sx = Math.hypot(m[0], m[1], m[2]) || 1, sy = Math.hypot(m[4], m[5], m[6]) || 1, sz = Math.hypot(m[8], m[9], m[10]) || 1;
    return [m[12], m[13], m[14], 0, 0, Math.atan2(m[1] / sx, m[0] / sx), sx, sy, sz];
  }

  /**
   * Live puppet state: the animation layers a scene object declares, physics and IK, and the
   * skinning matrices it yields.
   */
  class Instance {
    constructor(mdl, layers) {
      this.mdl = mdl;
      this.puppet = mdl.puppet;
      const n = this.puppet.bones.length;
      this.layers = [];
      this.bindLocal = this.puppet.bones.map(bindTRS);
      this.local = this.puppet.bones.map((b, i) => this.bindLocal[i].slice());
      this.world = new Array(n);
      this.skin = new Array(n);
      this.bones = new Float32Array(n * 12);
      this.physics = this.puppet.bones.map((b) => b.simulation && b.simulation.enabled !== false && b.simType !== 0 ? { angle: 0, velocity: 0, prevParentAngle: null } : null);
      this.setLayers(layers || []);
      this.time = 0;
      this.scratch = new Float32Array(9);
      this.controllers = [];
      // Script-set bone state: {origin, angles} folded into the local TRS, or a full local matrix.
      this.overrides = new Array(n).fill(null);
      // Called with (layer, {name, frame}) when playback crosses an animation event.
      this.onEvent = null;
    }

    resolveAnimation(ref, index) {
      const anims = this.puppet.animations;
      if (!anims.length) return null;
      if (typeof ref === 'number') {
        const byId = anims.find((a) => a.id === ref);
        if (byId) return byId;
        if (ref >= 0 && ref < anims.length) return anims[ref];
      }
      if (typeof ref === 'string') {
        const byName = anims.find((a) => a.name === ref);
        if (byName) return byName;
        const m = /^(\d+)$/.exec(ref.trim());
        if (m) { const k = parseInt(m[1], 10); if (k >= 0 && k < anims.length) return anims[k]; }
      }
      if (ref === undefined || ref === null) return anims[Math.min(index, anims.length - 1)];
      return null;
    }

    // `layers`: [{animation, rate, blend, visible, additive, name}] with Dynamic-like getters.
    setLayers(layers) {
      this.layers = layers.map((l, i) => ({
        source: l,
        anim: this.resolveAnimation(l.animation, i) || (() => { throw new Error('animation layer ' + (l.name || i) + ' names animation ' + JSON.stringify(l.animation) + ', which the puppet (' + this.puppet.animations.map((a) => a.name).join(', ') + ') does not have'); })(),
        start: 0,
        ended: false,
        endCallbacks: [],
        paused: false,
        frameOverride: null,
        lastFrame: null,
        pausedAt: 0,
      }));
    }

    addLayer(anim, opts) {
      const layer = { source: opts, anim, start: this.time, ended: false, endCallbacks: [], paused: false, frameOverride: null, lastFrame: null, pausedAt: 0, single: !!opts.single };
      this.layers.push(layer);
      return layer;
    }

    // IAnimationLayer playback: frames are timed from `start`; pausing freezes the frame.
    layerFrame(layer) {
      if (layer.frameOverride !== null) return layer.frameOverride;
      const t = (layer.paused ? layer.pausedAt : this.time) - layer.start;
      return frameOf(layer.anim, t, this.layerValue(layer, 'rate', 1), this.layerValue(layer, 'mode', null));
    }
    playLayer(layer) {
      if (layer.paused) { layer.start += this.time - layer.pausedAt; layer.paused = false; }
      if (layer.frameOverride !== null) { this.setLayerFrame(layer, layer.frameOverride); layer.frameOverride = null; }
      layer.ended = false;
    }
    pauseLayer(layer) { if (!layer.paused) { layer.paused = true; layer.pausedAt = this.time; } }
    stopLayer(layer) { layer.paused = true; layer.pausedAt = this.time; layer.frameOverride = 0; layer.ended = false; }
    setLayerFrame(layer, frame) {
      const fps = layer.anim.fps > 0 ? layer.anim.fps : 30;
      const rate = this.layerValue(layer, 'rate', 1) || 1;
      const now = layer.paused ? layer.pausedAt : this.time;
      layer.start = now - frame / (fps * rate);
      if (layer.paused) layer.frameOverride = frame;
    }
    isLayerPlaying(layer) { return !layer.paused && !layer.ended; }

    removeLayer(layer) { this.layers = this.layers.filter((l) => l !== layer); }

    layerValue(layer, key, dflt) {
      const s = layer.source || {};
      const v = s[key];
      if (v && typeof v.get === 'function') return v.get();
      return v === undefined ? dflt : v;
    }

    // Compose every visible layer into per-bone local transforms.
    update(time, dt) {
      this.time = time;
      const bones = this.puppet.bones;
      const n = bones.length;
      for (let i = 0; i < n; i++) for (let k = 0; k < 9; k++) this.local[i][k] = this.bindLocal[i][k];
      const s = this.scratch;
      for (const layer of this.layers) {
        if (!this.layerValue(layer, 'visible', true)) continue;
        const anim = layer.anim;
        const rate = this.layerValue(layer, 'rate', 1);
        const blend = Math.max(0, Math.min(1, this.layerValue(layer, 'blend', 1)));
        const additive = !!this.layerValue(layer, 'additive', false);
        const t = time - layer.start;
        const frame = this.layerFrame(layer);
        if (this.onEvent && !layer.paused) for (const ev of eventsBetween(anim, layer.lastFrame, frame)) this.onEvent(layer, { name: eventName(ev), frame: ev.frame });
        layer.lastFrame = frame;
        if (playMode(anim, this.layerValue(layer, 'mode', null)) === 'single' && !layer.ended && !layer.paused && t * (anim.fps || 30) * rate >= anim.length) {
          layer.ended = true;
          for (const cb of layer.endCallbacks) cb();
          if (layer.single) { layer.remove = true; }
        }
        if (blend <= 0) continue;
        for (let i = 0; i < n && i < anim.tracks.length; i++) {
          const track = anim.tracks[i];
          if (!sampleTrack(track, frame, s)) continue;
          const local = this.local[i];
          if (additive) {
            const ref = anim.tracks[i].data;
            for (let k = 0; k < 9; k++) {
              let delta = s[k] - ref[k];
              if (k >= 3 && k < 6) { while (delta > Math.PI) delta -= 2 * Math.PI; while (delta < -Math.PI) delta += 2 * Math.PI; }
              local[k] += delta * blend;
            }
          } else {
            for (let k = 0; k < 9; k++) {
              let delta = s[k] - local[k];
              if (k >= 3 && k < 6) { while (delta > Math.PI) delta -= 2 * Math.PI; while (delta < -Math.PI) delta += 2 * Math.PI; }
              local[k] += delta * blend;
            }
          }
        }
        if (anim.controllerTracks.length) {
          const ctrl = [];
          for (const track of anim.controllerTracks) { const c = new Float32Array(9); sampleTrack(track, frame, c); ctrl.push(c); }
          this.controllers = ctrl;
        }
      }
      this.layers = this.layers.filter((l) => !l.remove);
      for (let i = 0; i < n; i++) {
        const o = this.overrides[i];
        if (!o) continue;
        if (o.origin) { this.local[i][0] = o.origin[0]; this.local[i][1] = o.origin[1]; this.local[i][2] = o.origin[2]; }
        if (o.angles) { this.local[i][3] = o.angles[0]; this.local[i][4] = o.angles[1]; this.local[i][5] = o.angles[2]; }
      }
      this.applyPhysics(dt);
      this.applyIk();
      this.compose();
    }

    // IImageLayer bone setters: local origin / angles replace the animated TRS components, a
    // local or world matrix replaces the whole local transform.
    setLocalBoneOrigin(index, origin) { const o = this.overrides[index] || (this.overrides[index] = {}); o.origin = [origin[0], origin[1], origin[2]]; o.matrix = null; }
    setLocalBoneAngles(index, angles) { const o = this.overrides[index] || (this.overrides[index] = {}); o.angles = [angles[0], angles[1], angles[2]]; o.matrix = null; }
    setLocalBoneTransform(index, matrix) { this.overrides[index] = { matrix: M.clone(matrix) }; }
    setBoneTransform(index, world) {
      const p = this.puppet.bones[index].animParent;
      const parentWorld = p !== NO_PARENT ? this.world[p] : null;
      this.overrides[index] = { matrix: parentWorld ? M.multiply(M.inverse(parentWorld), world) : M.clone(world) };
    }
    localBoneTransform(index) {
      const o = this.overrides[index];
      if (o && o.matrix) return o.matrix;
      return localFromTRS(this.local[index], this.puppet.worldAnchored ? this.puppet.bones[index].centroid : null);
    }
    localBoneOrigin(index) { return [this.local[index][0], this.local[index][1], this.local[index][2]]; }
    localBoneAngles(index) { return [this.local[index][3], this.local[index][4], this.local[index][5]]; }

    // A directional impulse becomes torque about the bone's pivot; an angular one adds spin.
    applyPhysicsImpulse(index, directional, angular) {
      const ph = this.physics[index];
      if (!ph) throw new Error('bone "' + this.puppet.bones[index].name + '" has no physics simulation to push');
      const sim = this.puppet.bones[index].simulation;
      const length = sim.length > 0 ? sim.length : 100;
      const mass = sim.mass > 0 ? sim.mass : 1;
      const angle = this.local[index][5] + ph.angle;
      const rx = Math.cos(angle) * length, ry = Math.sin(angle) * length;
      ph.velocity += (rx * directional[1] - ry * directional[0]) / (mass * length * length) + angular[2] / mass;
    }
    resetPhysics(index) {
      const reset = (ph) => { if (ph) { ph.angle = 0; ph.velocity = 0; ph.prevParentAngle = null; } };
      if (index === undefined || index === null) for (const ph of this.physics) reset(ph); else reset(this.physics[index]);
    }

    // Spring-damper swing on physics bones, driven by their parent's motion and gravity.
    applyPhysics(dt) {
      if (!(dt > 0)) return;
      const bones = this.puppet.bones;
      const step = Math.min(dt, 1 / 30);
      for (let i = 0; i < bones.length; i++) {
        const ph = this.physics[i];
        if (!ph) continue;
        const sim = bones[i].simulation;
        const parent = bones[i].animParent;
        const parentAngle = parent !== NO_PARENT ? this.local[parent][5] : 0;
        if (ph.prevParentAngle === null) ph.prevParentAngle = parentAngle;
        const drive = (parentAngle - ph.prevParentAngle);
        ph.prevParentAngle = parentAngle;
        const stiffness = sim.stiffness || 10, damping = sim.damping || 2, mass = sim.mass || 1;
        const gravity = (sim.gravity || 0) * Math.sin(this.local[i][5] + ph.angle + parentAngle);
        const accel = (-stiffness * ph.angle - damping * ph.velocity - drive * stiffness * 0.5 - gravity) / mass;
        ph.velocity += accel * step;
        ph.angle += ph.velocity * step;
        const lo = sim.angleMin === undefined ? -Math.PI : sim.angleMin, hi = sim.angleMax === undefined ? Math.PI : sim.angleMax;
        if (ph.angle < lo) { ph.angle = lo; ph.velocity = 0; }
        if (ph.angle > hi) { ph.angle = hi; ph.velocity = 0; }
        this.local[i][5] += ph.angle;
      }
    }

    // Two-bone planar IK: bend the middle joint so the chain's end reaches its controller.
    applyIk() {
      const ik = this.puppet.ik;
      if (!ik || !this.controllers.length) return;
      const bones = this.puppet.bones;
      const worldOf = (i, local) => {
        const m = localFromTRS(local[i], null);
        const p = bones[i].animParent;
        return p !== NO_PARENT ? M.multiply(worldOf(p, local), m) : m;
      };
      for (const chain of ik.chains) {
        const target = this.controllers[chain.target];
        if (!target) continue;
        const [s, mid, e] = chain.bones;
        const l1 = ik.nodes[mid].length, l2 = ik.nodes[e].length;
        const rootW = worldOf(s, this.local);
        const rootPos = [rootW[12], rootW[13]];
        const ctrl = ik.controllers[chain.target];
        const targetPos = [ctrl.bind[12] + target[0], ctrl.bind[13] + target[1]];
        const dx = targetPos[0] - rootPos[0], dy = targetPos[1] - rootPos[1];
        const dist = Math.min(Math.hypot(dx, dy), l1 + l2 - 1e-4);
        const cosMid = Math.max(-1, Math.min(1, (l1 * l1 + l2 * l2 - dist * dist) / (2 * l1 * l2)));
        const midAngle = Math.PI - Math.acos(cosMid);
        const cosRoot = Math.max(-1, Math.min(1, (l1 * l1 + dist * dist - l2 * l2) / (2 * l1 * dist || 1)));
        const rootAngle = Math.atan2(dy, dx) - Math.acos(cosRoot);
        const parentW = bones[s].animParent !== NO_PARENT ? worldOf(bones[s].animParent, this.local) : M.identity();
        const parentAngle = Math.atan2(parentW[1], parentW[0]);
        const dir = ik.nodes[s].children.find((c) => c.bone === mid);
        const restAngle = dir ? Math.atan2(dir.direction[1], dir.direction[0]) : 0;
        this.local[s][5] = rootAngle - parentAngle - restAngle;
        const dir2 = ik.nodes[mid].children.find((c) => c.bone === e);
        const rest2 = dir2 ? Math.atan2(dir2.direction[1], dir2.direction[0]) : 0;
        this.local[mid][5] = midAngle - rest2;
      }
    }

    compose() {
      const bones = this.puppet.bones;
      const n = bones.length;
      const anchored = this.puppet.worldAnchored;
      for (let i = 0; i < n; i++) {
        const b = bones[i];
        const pivot = anchored ? b.centroid : null;
        const override = this.overrides[i];
        const local = override && override.matrix ? override.matrix : localFromTRS(this.local[i], pivot);
        const p = b.animParent;
        if (anchored) {
          this.world[i] = p !== NO_PARENT ? M.multiply(this.skin[p], local) : local;
        } else {
          this.world[i] = p !== NO_PARENT ? M.multiply(this.world[p], local) : local;
        }
        this.skin[i] = M.multiply(this.world[i], b.invBind);
        const m = this.skin[i];
        const o = i * 12;
        // mat4x3: four columns of three rows.
        this.bones[o] = m[0]; this.bones[o + 1] = m[1]; this.bones[o + 2] = m[2];
        this.bones[o + 3] = m[4]; this.bones[o + 4] = m[5]; this.bones[o + 5] = m[6];
        this.bones[o + 6] = m[8]; this.bones[o + 7] = m[9]; this.bones[o + 8] = m[10];
        this.bones[o + 9] = m[12]; this.bones[o + 10] = m[13]; this.bones[o + 11] = m[14];
      }
    }

    boneIndex(name) {
      if (typeof name === 'number') return name;
      const i = this.puppet.bones.findIndex((b) => b.name === name);
      return i;
    }

    // World transform of a bone within the puppet's local space (its skin matrix applied to bind).
    boneWorld(index) {
      if (index < 0 || index >= this.world.length || !this.world[index]) return M.identity();
      return this.world[index];
    }

    attachmentIndex(name) {
      if (typeof name === 'number') return name;
      return this.puppet.attachments.findIndex((a) => a.name === name);
    }

    attachmentWorld(index) {
      const a = this.puppet.attachments[index];
      if (!a) return null;
      return M.multiply(M.multiply(this.skin[a.bone] || M.identity(), this.puppet.bones[a.bone].worldBind), a.local);
    }
  }

  const api = { parse, Instance, NO_PARENT, FLAG, frameOf, strideOf, playMode, eventsBetween, eventName, localFromTRS, MODE_NAMES };
  G.WEPuppet = api;
  if (typeof module !== 'undefined' && module.exports) module.exports = api;
})();
