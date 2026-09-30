// Puppet warp: images whose model names a "puppet" .mdl draw the skinned mesh instead of the
// layer quad. The mesh, its bone indices and weights feed the first material pass through
// image.setFirstPassGeometry, the pass compiles with the SKINNING / BONECOUNT combos and reads
// the per-frame g_Bones skinning matrices; animation layers (scene "animationlayers",
// "puppetanimations", the model's "animations"), IK and bone physics run in WEPuppet.Instance,
// updated every frame from the scene. Also the bone, attachment and animation-layer surface the
// SceneScript image layer API exposes.
(() => {
  'use strict';
  const G = typeof window !== 'undefined' ? window : globalThis;
  const M = G.WEM;
  const Puppet = G.WEPuppet;

  const ATTRIBUTES = { position: 'a_Position', texcoord: 'a_TexCoord', indices: 'a_BlendIndices', weights: 'a_BlendWeights' };

  // uv = s * position + t along one axis, from the mesh's extreme vertices (puppet meshes are
  // the image plane triangulated, so their texture coordinates are affine in position).
  function fitAxis(positions, uvs, count, axis, name) {
    let imin = 0, imax = 0;
    for (let i = 1; i < count; i++) {
      if (positions[i * 3 + axis] < positions[imin * 3 + axis]) imin = i;
      if (positions[i * 3 + axis] > positions[imax * 3 + axis]) imax = i;
    }
    const p0 = positions[imin * 3 + axis], p1 = positions[imax * 3 + axis];
    const u0 = uvs[imin * 2 + axis], u1 = uvs[imax * 2 + axis];
    if (p1 - p0 < 1e-6) throw new Error(name + ': the puppet mesh is flat along axis ' + axis + ', so it cannot map onto the layer');
    const s = (u1 - u0) / (p1 - p0);
    return [s, u0 - s * p0];
  }

  function parseLayerSpec(json, where, properties, defaults) {
    if (!json || typeof json !== 'object') throw new Error(where + ' is not an object');
    const setting = (key, kind, dflt, expectColor) => G.WEProps.setting(json[key], { kind, default: dflt, expectColor: !!expectColor, where: where + '.' + key, properties });
    let animation = json.animation;
    if (animation === undefined) animation = defaults.nameIsAnimation && typeof json.name === 'string' ? json.name : 0;
    if (animation && typeof animation === 'object') animation = setting('animation', 'int', 0).get();
    return {
      id: typeof json.id === 'number' ? json.id : null,
      name: typeof json.name === 'string' ? json.name : '',
      animation,
      rate: setting('rate', 'float', 1),
      blend: setting('blend', 'float', 1),
      visible: setting('visible', 'bool', defaults.visible),
      additive: setting('additive', 'bool', false),
      mode: typeof json.mode === 'string' ? json.mode : (typeof json.playbackmode === 'string' ? json.playbackmode : null),
    };
  }

  // Animation layers from the scene object ("animationlayers" as the editor writes them, the
  // "puppetanimations" alias) and the model file's own "animations".
  function layerSpecs(image) {
    const properties = image.scene.properties;
    const specs = [];
    for (const key of ['animationlayers', 'puppetanimations']) {
      const list = image.json[key];
      if (list === undefined || list === null) continue;
      if (!Array.isArray(list)) throw new Error('object ' + image.id + ': "' + key + '" is not a list');
      list.forEach((entry, i) => specs.push(parseLayerSpec(entry, 'object ' + image.id + '.' + key + '[' + i + ']', properties, { visible: false, nameIsAnimation: false })));
    }
    image.model.animations.forEach((entry, i) => specs.push(parseLayerSpec(entry, image.model.filename + '.animations[' + i + ']', properties, { visible: true, nameIsAnimation: true })));
    return specs;
  }

  /** The GL side of one puppet: vertex streams in copy space and the skinning matrices. */
  class PuppetGeometry {
    constructor(image, mdl, instance) {
      this.image = image;
      this.gl = image.scene.gl;
      this.mdl = mdl;
      this.instance = instance;
      this.name = image.model.puppet;
      this.buffers = { position: this.gl.createBuffer(), scenePosition: this.gl.createBuffer(), texcoord: this.gl.createBuffer(), indices: this.gl.createBuffer(), weights: this.gl.createBuffer(), elements: this.gl.createBuffer() };
      this.positionBuffer = this.buffers.position;
      this.vaos = new WeakMap();
      this.indexCount = 0;
      this.vertexCount = 0;
      // uv = fit.s * position + fit.t per axis; `map` places the mesh in copy space (0..size),
      // `sceneMap` on the layer's quad in scene space for a pass that draws to the scene.
      this.fit = null;
      this.map = M.identity();
      this.sceneMap = M.identity();
      this.scenePos = null;
      this.size = [0, 0];
      this.boneCache = new Map();
      this.meshPositions = null;
      this.meshUvs = null;
      this.uploadStatic();
    }

    // Concatenate every mesh's streams once; positions are re-mapped whenever the layer resizes.
    uploadStatic() {
      const gl = this.gl;
      const meshes = this.mdl.meshes;
      let total = 0, indexTotal = 0;
      for (const m of meshes) {
        if (!m.uvs) throw new Error(this.name + ': a puppet mesh without texture coordinates cannot be drawn');
        if (!m.blendIndices) throw new Error(this.name + ': a puppet mesh without bone indices cannot be skinned');
        total += m.count;
        indexTotal += m.indices.length;
      }
      this.vertexCount = total;
      this.indexCount = indexTotal;
      this.meshPositions = new Float32Array(total * 3);
      this.meshUvs = new Float32Array(total * 2);
      const indices = new Uint32Array(total * 4), weights = new Float32Array(total * 4), elements = new Uint32Array(indexTotal);
      let v = 0, e = 0;
      for (const m of meshes) {
        this.meshPositions.set(m.positions, v * 3);
        this.meshUvs.set(m.uvs, v * 2);
        indices.set(m.blendIndices, v * 4);
        if (m.blendWeights) weights.set(m.blendWeights, v * 4);
        else for (let i = 0; i < m.count; i++) weights[(v + i) * 4] = 1;
        for (let i = 0; i < m.indices.length; i++) elements[e + i] = m.indices[i] + v;
        v += m.count;
        e += m.indices.length;
      }
      const boneCount = this.instance.puppet.bones.length;
      for (let i = 0; i < indices.length; i++) if (weights[i] > 0 && indices[i] >= boneCount) throw new Error(this.name + ': vertex ' + (i >> 2) + ' is bound to bone ' + indices[i] + ' of ' + boneCount);
      this.blendIndices = indices;
      this.blendWeights = weights;
      gl.bindBuffer(gl.ARRAY_BUFFER, this.buffers.indices);
      gl.bufferData(gl.ARRAY_BUFFER, indices, gl.STATIC_DRAW);
      gl.bindBuffer(gl.ARRAY_BUFFER, this.buffers.weights);
      gl.bufferData(gl.ARRAY_BUFFER, weights, gl.STATIC_DRAW);
      gl.bindBuffer(gl.ELEMENT_ARRAY_BUFFER, this.buffers.elements);
      gl.bufferData(gl.ELEMENT_ARRAY_BUFFER, elements, gl.STATIC_DRAW);
      gl.bindBuffer(gl.ELEMENT_ARRAY_BUFFER, null);
      gl.bindBuffer(gl.ARRAY_BUFFER, null);
    }

    // Mesh -> target-space map: the uv fit stretched over [left..right] x [bottom..top].
    mapFor(left, right, bottom, top) {
      const f = this.fit;
      const map = M.identity();
      map[0] = f.sx * (right - left); map[12] = left + f.tx * (right - left);
      map[5] = f.sy * (top - bottom); map[13] = bottom + f.ty * (top - bottom);
      return map;
    }

    uploadPositions(buffer, map) {
      const gl = this.gl, n = this.vertexCount;
      const positions = new Float32Array(n * 3);
      for (let i = 0; i < n; i++) {
        positions[i * 3] = this.meshPositions[i * 3] * map[0] + map[12];
        positions[i * 3 + 1] = this.meshPositions[i * 3 + 1] * map[5] + map[13];
        positions[i * 3 + 2] = this.meshPositions[i * 3 + 2];
      }
      gl.bindBuffer(gl.ARRAY_BUFFER, buffer);
      gl.bufferData(gl.ARRAY_BUFFER, positions, gl.STATIC_DRAW);
      gl.bindBuffer(gl.ARRAY_BUFFER, null);
    }

    // The scene-space map follows the layer's quad (image.pos: left, top, right, bottom, y up).
    sceneMapFor(pos) {
      const p = this.scenePos;
      if (p && p[0] === pos[0] && p[1] === pos[1] && p[2] === pos[2] && p[3] === pos[3]) return this.sceneMap;
      this.scenePos = [pos[0], pos[1], pos[2], pos[3]];
      this.sceneMap = this.mapFor(pos[0], pos[2], pos[3], pos[1]);
      this.uploadPositions(this.buffers.scenePosition, this.sceneMap);
      this.boneCache.delete('scene');
      return this.sceneMap;
    }

    drawsToScene(pass) { return pass.positionBuffer === this.image.buffers.sceneSpace; }

    // The mesh -> copy-space map: mesh uv spans the image, copy space spans the layer size.
    resize(size) {
      const gl = this.gl;
      this.size = [size[0], size[1]];
      const n = this.vertexCount;
      const [sx, tx] = fitAxis(this.meshPositions, this.meshUvs, n, 0, this.name);
      const [sy, ty] = fitAxis(this.meshPositions, this.meshUvs, n, 1, this.name);
      this.fit = { sx, tx, sy, ty };
      this.map = this.mapFor(0, size[0], 0, size[1]);
      this.uploadPositions(this.buffers.position, this.map);
      this.scenePos = null;
      this.boneCache.clear();
      // Texture coordinates follow the layer's copy quad: padded textures cover only part of it.
      const tex = this.image.texture;
      let width = 1, height = 1;
      if (tex && !tex.animated && (tex.textureWidth(0) !== tex.realWidth || tex.textureHeight(0) !== tex.realHeight)) {
        width = tex.realWidth / tex.textureWidth(0);
        height = tex.realHeight / tex.textureHeight(0);
      }
      const uvs = new Float32Array(n * 2);
      for (let i = 0; i < n; i++) { uvs[i * 2] = this.meshUvs[i * 2] * width; uvs[i * 2 + 1] = this.meshUvs[i * 2 + 1] * height; }
      gl.bindBuffer(gl.ARRAY_BUFFER, this.buffers.texcoord);
      gl.bufferData(gl.ARRAY_BUFFER, uvs, gl.STATIC_DRAW);
      gl.bindBuffer(gl.ARRAY_BUFFER, null);
    }

    // Skinning matrices in the pass's space: map * skin * map^-1, packed as the shader declares.
    boneMatrices(pass, columns) {
      const scene = this.drawsToScene(pass);
      const map = scene ? this.sceneMapFor(this.image.pos) : this.map;
      const key = scene ? 'scene' : 'copy';
      const stamp = this.image.scene.time;
      let cache = this.boneCache.get(key);
      if (!cache || cache.stamp !== stamp) {
        const n = this.instance.skin.length;
        cache = cache || { stamp: null, b12: new Float32Array(n * 12), b16: new Float32Array(n * 16) };
        cache.stamp = stamp;
        const inverse = M.inverse(map);
        const skin = this.instance.skin;
        for (let i = 0; i < skin.length; i++) {
          const m = M.multiply(M.multiply(map, skin[i]), inverse);
          cache.b16.set(m, i * 16);
          const o = i * 12;
          cache.b12[o] = m[0]; cache.b12[o + 1] = m[1]; cache.b12[o + 2] = m[2];
          cache.b12[o + 3] = m[4]; cache.b12[o + 4] = m[5]; cache.b12[o + 5] = m[6];
          cache.b12[o + 6] = m[8]; cache.b12[o + 7] = m[9]; cache.b12[o + 8] = m[10];
          cache.b12[o + 9] = m[12]; cache.b12[o + 10] = m[13]; cache.b12[o + 11] = m[14];
        }
        this.boneCache.set(key, cache);
      }
      return columns === 16 ? cache.b16 : cache.b12;
    }

    // Uniform getters for a pass: g_Bones in the type the program declares.
    passUniforms(pass) {
      const info = pass.programUniforms.g_Bones;
      if (!info) return {};
      const gl = this.gl;
      if (info.type === gl.FLOAT_MAT4) return { g_Bones: () => this.boneMatrices(pass, 16) };
      if (info.type === gl.FLOAT_MAT4x3) return { g_Bones: () => this.boneMatrices(pass, 12) };
      throw new Error(this.name + ': shader ' + pass.pass.shader + ' declares g_Bones with a type (0x' + info.type.toString(16) + ') that is neither mat4 nor mat4x3');
    }

    vaoFor(pass) {
      const cached = this.vaos.get(pass);
      if (cached) return cached;
      const gl = this.gl;
      const vao = gl.createVertexArray();
      gl.bindVertexArray(vao);
      gl.bindBuffer(gl.ELEMENT_ARRAY_BUFFER, this.buffers.elements);
      const bind = (name, buffer, size, integer) => {
        const info = pass.programAttributes[name];
        if (!info || info.location < 0) return false;
        gl.enableVertexAttribArray(info.location);
        gl.bindBuffer(gl.ARRAY_BUFFER, buffer);
        if (integer && (info.type === gl.UNSIGNED_INT_VEC4 || info.type === gl.INT_VEC4)) gl.vertexAttribIPointer(info.location, size, gl.UNSIGNED_INT, 0, 0);
        else if (integer) {
          const floats = new Float32Array(this.blendIndices);
          gl.bufferData(gl.ARRAY_BUFFER, floats, gl.STATIC_DRAW);
          gl.vertexAttribPointer(info.location, size, gl.FLOAT, false, 0, 0);
        } else gl.vertexAttribPointer(info.location, size, gl.FLOAT, false, 0, 0);
        return true;
      };
      if (!bind(ATTRIBUTES.position, this.buffers.position, 3, false)) throw new Error(this.name + ': shader ' + pass.pass.shader + ' reads no ' + ATTRIBUTES.position);
      bind(ATTRIBUTES.texcoord, this.buffers.texcoord, 2, false);
      const skinned = bind(ATTRIBUTES.indices, this.buffers.indices, 4, true) && bind(ATTRIBUTES.weights, this.buffers.weights, 4, false);
      if (!skinned) throw new Error(this.name + ': shader ' + pass.pass.shader + ' compiled without the SKINNING vertex attributes ' + ATTRIBUTES.indices + ' / ' + ATTRIBUTES.weights);
      gl.bindVertexArray(null);
      gl.bindBuffer(gl.ARRAY_BUFFER, null);
      this.vaos.set(pass, vao);
      return vao;
    }

    // Bind the pass's VAO with the positions of the space it draws in (copy buffer or scene).
    setup(pass) {
      const gl = this.gl;
      gl.bindVertexArray(this.vaoFor(pass));
      const info = pass.programAttributes[ATTRIBUTES.position];
      let buffer = this.buffers.position;
      if (this.drawsToScene(pass)) { this.sceneMapFor(this.image.pos); buffer = this.buffers.scenePosition; }
      gl.bindBuffer(gl.ARRAY_BUFFER, buffer);
      gl.vertexAttribPointer(info.location, 3, gl.FLOAT, false, 0, 0);
    }
    draw() { this.gl.drawElements(this.gl.TRIANGLES, this.indexCount, this.gl.UNSIGNED_INT, 0); }
    cleanup(pass) { this.gl.bindVertexArray(pass.vao); }

    dispose() {
      for (const b of Object.values(this.buffers)) this.gl.deleteBuffer(b);
    }
  }

  /** The puppet attached to one image layer: model, live instance, geometry and the layer API. */
  class PuppetWarp {
    constructor(image, mdl, specs) {
      this.image = image;
      this.mdl = mdl;
      this.puppet = mdl.puppet;
      this.specs = specs;
      this.instance = new Puppet.Instance(mdl, specs);
      this.geometry = new PuppetGeometry(image, mdl, this.instance);
      this.unsubscribe = image.scene.onUpdate((dt, time) => this.instance.update(time, dt));
      this.instance.update(image.scene.time, 0);
      this.eventListeners = [];
      this.instance.onEvent = (layer, event) => { for (const fn of this.eventListeners) fn(layer, event); };
    }

    onAnimationEvent(fn) { this.eventListeners.push(fn); }

    boneIndex(ref) {
      const index = this.instance.boneIndex(ref);
      if (typeof index !== 'number' || index < 0 || index >= this.puppet.bones.length) throw new Error('puppet ' + this.image.model.puppet + ' has no bone ' + JSON.stringify(ref));
      return index;
    }

    attachmentIndex(ref) {
      const index = this.instance.attachmentIndex(ref);
      if (typeof index !== 'number' || index < 0 || index >= this.puppet.attachments.length) throw new Error('puppet ' + this.image.model.puppet + ' has no attachment ' + JSON.stringify(ref));
      return index;
    }

    // Attachment transform in layer space (pixels from the layer centre, y up, angle in radians).
    attachmentTransform(ref) {
      const index = this.attachmentIndex(ref);
      const world = M.multiply(this.geometry.map, this.instance.attachmentWorld(index));
      const size = this.geometry.size;
      return { origin: [world[12] - size[0] / 2, world[13] - size[1] / 2, world[14]], angle: Math.atan2(world[1], world[0]), matrix: world };
    }

    findLayer(ref) {
      const layers = this.instance.layers;
      if (typeof ref === 'number') {
        const byId = layers.find((l) => l.source && l.source.id === ref);
        if (byId) return byId;
        if (ref >= 0 && ref < layers.length) return layers[ref];
      }
      if (typeof ref === 'string') {
        const byName = layers.find((l) => (l.source && l.source.name === ref) || l.anim.name === ref);
        if (byName) return byName;
      }
      if (ref && typeof ref === 'object' && layers.includes(ref)) return ref;
      throw new Error('puppet ' + this.image.model.puppet + ' has no animation layer ' + JSON.stringify(ref));
    }

    createLayer(animation, config, single) {
      const spec = animation && typeof animation === 'object' ? Object.assign({}, animation, config || {}) : Object.assign({ animation }, config || {});
      const anim = this.instance.resolveAnimation(spec.animation !== undefined ? spec.animation : spec.name, this.instance.layers.length);
      if (!anim) throw new Error('puppet ' + this.image.model.puppet + ' has no animation ' + JSON.stringify(spec.animation !== undefined ? spec.animation : spec.name));
      const source = {
        id: typeof spec.id === 'number' ? spec.id : null,
        name: typeof spec.name === 'string' ? spec.name : anim.name,
        rate: typeof spec.rate === 'number' ? spec.rate : 1,
        blend: typeof spec.blend === 'number' ? spec.blend : 1,
        visible: spec.visible === undefined ? true : !!spec.visible,
        additive: !!spec.additive,
        mode: typeof spec.mode === 'string' ? spec.mode : (single ? 'once' : null),
        single: !!single,
      };
      return this.instance.addLayer(anim, source);
    }

    dispose() {
      this.unsubscribe();
      this.geometry.dispose();
    }
  }

  // Image.modelHooks entry: models with a puppet load it, mark the first pass for skinning and
  // hand the mesh to the image before its passes are built.
  async function attach(image) {
    if (!image.model.puppet) return;
    const bytes = await image.scene.loader.bytes(image.model.puppet);
    const mdl = Puppet.parse(bytes, image.model.puppet);
    if (!mdl.puppet) throw new Error(image.model.puppet + ': the model has no bones; an image "puppet" must name a puppet warp model');
    if (!mdl.puppet.bones.length) throw new Error(image.model.puppet + ': the puppet declares no bones');
    if (!image.model.material.passes.length) throw new Error('object ' + image.id + ': material ' + image.model.material.filename + ' has no passes');
    // The layer specs hang on the image so scripts bound to their rate/blend/visible are found.
    image.puppetLayers = layerSpecs(image);
    const warp = new PuppetWarp(image, mdl, image.puppetLayers);
    image.puppet = warp;
    const first = image.model.material.passes[0];
    first.combos.SKINNING = 1;
    first.combos.BONECOUNT = mdl.puppet.bones.length;
    image.passUniforms = (pass) => warp.geometry.passUniforms(pass);
    image.attachmentTransform = (ref) => warp.attachmentTransform(ref);
    const geometry = warp.geometry;
    image.setFirstPassGeometry({
      positionBuffer: geometry.positionBuffer,
      setup: (pass) => geometry.setup(pass),
      draw: (pass) => geometry.draw(pass),
      cleanup: (pass) => geometry.cleanup(pass),
      resize: (size) => geometry.resize(size),
    });
    const dispose = image.dispose.bind(image);
    image.dispose = () => { warp.dispose(); dispose(); };
  }

  G.WEImage.modelHooks.push(attach);

  const api = { attach, PuppetWarp, PuppetGeometry, fitAxis, layerSpecs, parseLayerSpec, ATTRIBUTES };
  G.WEPuppetWarp = api;
  if (typeof module !== 'undefined' && module.exports) module.exports = api;
})();
