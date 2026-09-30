// Particle layers: scene.json objects carrying a "particle" definition (a file under particles/
// or inline JSON) with "instanceoverride" multipliers. WEParticles.System simulates them; this
// module owns the material, the render pass with Wallpaper Engine's particle shaders and the
// sprite / rope vertex streams, for the root system and every static child system its
// definition lists. Ports Render/Objects/CParticle.cpp (setup, setupPass, setupGeometryCallbacks,
// setupParticleUniforms, update, render, renderSprites, renderRope) and
// ObjectParser::parseParticleChild.
(() => {
  'use strict';
  const G = typeof window !== 'undefined' ? window : globalThis;
  const M = G.WEM;

  const MINIMUM_PARTICLE_DEPTH = 0.65;
  const DT_CAP = 0.1;
  const PREWARM_STEP = 1 / 30;
  const MAX_CHILD_DEPTH = 8;
  const CONTROL_POINTS = 8;
  const UP = new Float32Array([0, 1, 0]);
  const RIGHT = new Float32Array([1, 0, 0]);
  const FORWARD = new Float32Array([0, 0, 1]);
  const DEFAULT_TEXTURE = 'util/white';
  const REFRACT_AMOUNT = 0.05;

  // Attribute layouts the generic particle shaders declare (floats per vertex, name, size, offset).
  const SPRITE_LAYOUT = [['a_Position', 3, 0], ['a_TexCoordVec4', 4, 3], ['a_Color', 4, 7], ['a_TexCoordVec4C1', 4, 11], ['a_TexCoordC2', 2, 15]];
  const ROPE_LAYOUT = [['a_PositionVec4', 4, 0], ['a_TexCoordVec4', 4, 4], ['a_TexCoordVec4C1', 4, 8], ['a_TexCoordVec4C2', 4, 12], ['a_TexCoordVec4C3', 4, 16], ['a_TexCoordC4', 2, 20], ['a_Color', 4, 22]];

  function usesAudio(def) {
    for (const e of def.emitters) if (e.audioMode !== 0) return true;
    for (const op of def.operators) if (op.audioMode && op.audioMode.getNumber() !== 0) return true;
    return false;
  }

  // A child's origin/angles/scale composed onto its parent system's transform, the way object
  // parents compose (CImage::resolveTransform): the offset rotates and scales with the parent.
  function composeTransform(parent, child) {
    const offset = G.WEObjects.rotateVec2(child.origin[0] * parent.scale[0], child.origin[1] * parent.scale[1], parent.angles[2]);
    return {
      origin: [parent.origin[0] + offset[0], parent.origin[1] + offset[1], parent.origin[2] + child.origin[2] * parent.scale[2]],
      scale: [parent.scale[0] * child.scale[0], parent.scale[1] * child.scale[1], parent.scale[2] * child.scale[2]],
      angles: [parent.angles[0] + child.angles[0], parent.angles[1] + child.angles[1], parent.angles[2] + child.angles[2]],
    };
  }

  /**
   * One particle system with its material, render pass and vertex streams: the layer's root
   * system or a static child of it. It is the renderable its pass reads (scene, id, texture,
   * colour accessors, passUniforms). `transform()` yields the system's scene-space transform.
   */
  class Unit {
    constructor(owner, file, json, where, transform, parent, spec) {
      this.owner = owner;
      this.scene = owner.scene;
      this.id = owner.id;
      this.file = file;
      this.json = json;
      this.where = where;
      this.transform = transform;
      this.parent = parent;
      this.spec = spec;
      this.def = null;
      this.material = null;
      this.texture = null;
      this.system = null;
      this.pass = null;
      this.fboProvider = null;
      this.refractFBO = null;
      this.hasRefract = false;
      this.overbright = 1;
      this.vao = null;
      this.vbo = null;
      this.ebo = null;
      this.indexCount = 0;
      this.mats = null;
      this.vars = { var0: new Float32Array(4), var1: new Float32Array([0, 0, 0, 1]) };
      this.children = [];
    }

    // CRenderable::detectTexture for particles: the material's first texture slot, else the
    // shader's default texture.
    async detectTexture() {
      const pass = this.material.passes[0];
      const keys = Object.keys(pass.textures).map((k) => parseInt(k, 10)).sort((a, b) => a - b);
      const name = keys.length ? pass.textures[keys[0]] : DEFAULT_TEXTURE;
      const provider = G.WEPass.isFBOName(name) ? this.scene.findFBO(name) : await this.scene.textures.resolve(name);
      this.texture = new G.WETextures.TextureRef('object ' + this.id, provider);
      const property = pass.textureUsers[keys.length ? keys[0] : 0];
      if (property) G.WEImage.Image.prototype.bindTextureProperty.call(this, property, provider);
    }

    async setup(depth) {
      const where = this.where;
      const setting = (value, opts) => G.WEProps.setting(value, Object.assign({}, opts, { where: where + ' ' + (opts && opts.where ? opts.where : 'particle'), properties: this.scene.properties }));
      // Instance overrides belong to the layer: children share the root's.
      this.def = G.WEParticles.parseDefinition(this.json, this.parent ? {} : this.owner.json, setting);
      if (this.parent) this.def.instance = this.parent.def.instance;
      if (!this.def.material) throw new Error(where + ': the particle definition names no material');
      this.material = await G.WEMaterial.loadMaterial(this.owner.ctx(), this.def.material);
      if (!this.material.passes.length) throw new Error(where + ': material ' + this.material.filename + ' has no passes');
      const first = this.material.passes[0];
      if (first.constants.ui_editor_properties_overbright) this.overbright = first.constants.ui_editor_properties_overbright.getNumber();
      await this.detectTexture();
      const spritesheet = this.texture.spritesheet && this.texture.spritesheet.frames > 0 ? this.texture.spritesheet : null;
      if (usesAudio(this.def)) this.scene.requireAudio();
      this.system = new G.WEParticles.System(this.def, {
        origin: { getVec: () => this.transform().origin },
        scale: { getVec: () => this.transform().scale },
        angles: { getVec: () => this.transform().angles },
        sceneWidth: this.scene.width,
        sceneHeight: this.scene.height,
        audio: () => this.owner.audio(),
        spritesheet,
      });
      await this.setupPass(first, spritesheet);
      this.setupGeometry();
      this.prewarm();
      await this.setupChildren(depth);
    }

    // ObjectParser::parseParticleChild: static children are whole systems of their own, created
    // with the parent when their probability draw passes, placed by origin/angles/scale relative
    // to the parent, with the definition's maxcount replaced when the child names one.
    async setupChildren(depth) {
      for (const child of this.def.children) {
        const where = this.where + ' child "' + (child.name || child.particle) + '"';
        if (child.type !== 'static') throw new Error(where + ': child particle type "' + child.type + '" has no reference behaviour this renderer can follow (static children are drawn)');
        if (!child.particle) throw new Error(where + ' names no particle definition file');
        if (depth >= MAX_CHILD_DEPTH) throw new Error(where + ': child particle systems nest deeper than ' + MAX_CHILD_DEPTH + ' levels');
        if (!(Math.random() < child.probability)) continue;
        let json = await this.scene.loader.json(child.particle);
        if (!json || typeof json !== 'object') throw new Error(where + ': ' + child.particle + ' is not a particle definition');
        if (child.maxCount !== null) json = Object.assign({}, json, { maxcount: child.maxCount });
        const unit = new Unit(this.owner, child.particle, json, where, () => composeTransform(this.transform(), child), this, child);
        await unit.setup(depth + 1);
        this.children.push(unit);
      }
    }

    // CParticle::setupPass: the material's first pass with the particle combos, the rope shader
    // for rope renderers, slot 0 bound to the particle texture, and a scene copy for REFRACT.
    async setupPass(first, spritesheet) {
      const gl = this.scene.gl;
      const override = { combos: { THICKFORMAT: 1 }, constants: {}, textures: {}, usertextures: {}, textureUsers: {}, shaderOverride: this.system.rope ? 'genericropeparticle' : null };
      if (spritesheet) override.combos.SPRITESHEET = 1;
      if (this.system.trail) override.combos.TRAILRENDERER = 1;
      this.hasRefract = first.combos.REFRACT !== undefined && first.combos.REFRACT !== 0;
      this.fboProvider = new G.WEFBO.Provider(gl, this.scene.fbos);
      if (this.hasRefract) {
        const w = this.scene.fbo.realWidth, h = this.scene.fbo.realHeight;
        this.refractFBO = this.fboProvider.create('_rt_FullFrameBuffer', 'rgba8888', G.WEFBO.FLAG.CLAMP_UVS, [w, h], [w, h]);
      }
      this.pass = new G.WEPass.Pass(this, this.fboProvider, first, override, { 0: 'previous' }, null);
      await this.pass.build();
      this.pass.setDestination(this.scene.fbo);
      this.pass.setInput(this.texture);
    }

    // CParticle::setupPass (buffers) + setupGeometryCallbacks: one interleaved stream in the
    // layout the shader declares, drawn indexed from the system's VAO.
    setupGeometry() {
      const gl = this.scene.gl;
      const layout = this.system.rope ? ROPE_LAYOUT : SPRITE_LAYOUT;
      const floats = this.system.rope ? G.WEParticles.ROPE_FLOATS : G.WEParticles.SPRITE_FLOATS;
      this.vao = gl.createVertexArray();
      this.vbo = gl.createBuffer();
      this.ebo = gl.createBuffer();
      gl.bindVertexArray(this.vao);
      gl.bindBuffer(gl.ARRAY_BUFFER, this.vbo);
      gl.bindBuffer(gl.ELEMENT_ARRAY_BUFFER, this.ebo);
      let bound = 0;
      for (const [name, size, offset] of layout) {
        const location = this.pass.attributeLocation(name);
        if (location < 0) continue;
        gl.enableVertexAttribArray(location);
        gl.vertexAttribPointer(location, size, gl.FLOAT, false, floats * 4, offset * 4);
        bound++;
      }
      gl.bindVertexArray(null);
      if (!bound) throw new Error(this.where + ': shader ' + this.pass.pass.shader + ' reads none of the particle vertex attributes (' + layout.map((l) => l[0]).join(', ') + ')');
      this.pass.setGeometryCallback(
        () => { gl.bindVertexArray(this.vao); },
        () => { gl.drawElements(gl.TRIANGLES, this.indexCount, gl.UNSIGNED_INT, 0); },
        () => { gl.bindVertexArray(this.pass.vao); },
      );
    }

    // "starttime": the system starts this many seconds into its run.
    prewarm() {
      const start = this.def.startTime;
      if (!(start > 0)) return;
      for (let t = 0; t < start; t += PREWARM_STEP) this.system.update(Math.min(PREWARM_STEP, start - t), t, null);
    }

    // CParticle::setupParticleUniforms: common_particles.h uniforms beyond the engine set.
    passUniforms() {
      const uniforms = {
        g_ModelMatrixInverse: () => this.mats.modelInverse,
        g_OrientationUp: () => UP,
        g_OrientationRight: () => RIGHT,
        g_OrientationForward: () => FORWARD,
        g_ViewUp: () => UP,
        g_ViewRight: () => RIGHT,
        g_EyePosition: () => this.mats.eye,
        g_RenderVar0: () => this.vars.var0,
        g_RenderVar1: () => this.vars.var1,
      };
      if (this.hasRefract) uniforms.g_RefractAmount = () => REFRACT_AMOUNT;
      return uniforms;
    }

    // Control points from the child's start index follow the parent's, expressed in the child's
    // local frame, so a child emitter can track the parent's pointer or world-space points.
    mirrorControlPoints() {
      const parent = this.parent.system, child = this.system;
      child.refreshOrigin();
      const toWorld = parent.modelMatrix(null), toLocal = M.inverse(child.modelMatrix(null));
      for (let i = Math.max(0, this.spec.controlPointStartIndex | 0); i < CONTROL_POINTS; i++) {
        const world = M.transformPoint(toWorld, parent.controlPointPosition(i));
        child.mirrorControlPoint(i, M.transformPoint(toLocal, world));
      }
    }

    // The parent steps first so its children see this frame's control points.
    update(dt, time, mouse) {
      this.system.update(dt, time, mouse);
      for (const child of this.children) {
        child.mirrorControlPoints();
        child.update(dt, time, mouse);
      }
    }

    // CParticle::render -> renderSprites / renderRope, then the children right after.
    render(parallaxOffset) {
      const gl = this.scene.gl, sys = this.system;
      this.mats = sys.matrices(this.scene, parallaxOffset);
      const vars = sys.renderVars(this.texture);
      this.vars.var0.set(vars.var0);
      this.vars.var1.set(vars.var1);
      const stream = sys.rope ? sys.buildRope(this.scene.time) : sys.buildSprites();
      this.indexCount = stream.indexCount;
      if (this.indexCount) {
        gl.bindVertexArray(this.vao);
        gl.bindBuffer(gl.ARRAY_BUFFER, this.vbo);
        gl.bufferData(gl.ARRAY_BUFFER, stream.vertices, gl.DYNAMIC_DRAW);
        gl.bindBuffer(gl.ELEMENT_ARRAY_BUFFER, this.ebo);
        gl.bufferData(gl.ELEMENT_ARRAY_BUFFER, stream.indices, gl.DYNAMIC_DRAW);
        gl.bindVertexArray(null);
        if (this.hasRefract) this.copySceneForRefraction();
        const pass = this.pass;
        pass.setDestination(this.scene.fbo);
        pass.setInput(this.texture);
        pass.setModelViewProjectionMatrix(this.mats.mvp);
        pass.setModelViewProjectionMatrixInverse(this.mats.mvpInverse);
        pass.setModelMatrix(this.mats.model);
        pass.setViewProjectionMatrix(this.mats.viewProjection);
        pass.render();
      }
      for (const child of this.children) child.render(parallaxOffset);
    }

    // REFRACT shaders read the scene while the pass writes to it: they get a copy taken now.
    copySceneForRefraction() {
      const gl = this.scene.gl, src = this.scene.fbo, dst = this.refractFBO;
      gl.bindFramebuffer(gl.READ_FRAMEBUFFER, src.framebuffer);
      gl.bindFramebuffer(gl.DRAW_FRAMEBUFFER, dst.framebuffer);
      gl.blitFramebuffer(0, 0, src.realWidth, src.realHeight, 0, 0, dst.realWidth, dst.realHeight, gl.COLOR_BUFFER_BIT, gl.NEAREST);
      gl.bindFramebuffer(gl.FRAMEBUFFER, src.framebuffer);
    }

    setPaused(paused) {
      if (this.texture) this.texture.setPaused(paused);
      for (const child of this.children) child.setPaused(paused);
    }

    // Every definition in this tree (the scripts and bindings inside them are found through it).
    definitions(out) {
      out.push(this.def);
      for (const child of this.children) child.definitions(out);
      return out;
    }

    // CRenderable accessors the pass reads; overbright is the particle material's brightness.
    getBrightness() { return this.overbright; }
    getUserAlpha() { return 1; }
    getAlpha() { return 1; }
    getColor() { return [1, 1, 1]; }
    getColor4() { return [1, 1, 1, 1]; }
    getCompositeColor() { return [1, 1, 1]; }
    get animationTime() { return this.texture ? this.texture.animationTime : 0; }

    dispose() {
      const gl = this.scene.gl;
      for (const child of this.children) child.dispose();
      this.children = [];
      if (this.pass) this.pass.dispose();
      if (this.vao) gl.deleteVertexArray(this.vao);
      if (this.vbo) gl.deleteBuffer(this.vbo);
      if (this.ebo) gl.deleteBuffer(this.ebo);
      if (this.fboProvider) this.fboProvider.dispose();
      this.pass = null;
      this.vao = this.vbo = this.ebo = null;
    }
  }

  class Particle extends G.WEObjects.SceneObject {
    constructor(scene, json) {
      super(scene, json);
      if (json.parentmodifiers !== undefined && json.parentmodifiers !== null) {
        throw new Error('object ' + this.id + ' (' + this.name + '): particle "parentmodifiers" have no reference behaviour this renderer can follow');
      }
      this.parallaxDepth = this.setting('parallaxDepth', 'vec2', [0, 0]);
      this.file = null;
      this.root = null;
      // The root and child definitions as a plain list, so their bindings and scripts are found.
      this.defs = [];
      this.audioBins = new Float32Array(128);
      this.audioStamp = -1;
      // The system reads the resolved (parent-folded) transform, so grouped particles follow their group.
      this.resolved = { origin: [0, 0, 0], scale: [1, 1, 1], angles: [0, 0, 0] };
      this.resolvedStamp = -1;
    }

    ctx() { return { loader: this.scene.loader, properties: this.scene.properties }; }

    // The root system's definition and simulation (what the SceneScript layer API drives).
    get def() { return this.root ? this.root.def : null; }
    get system() { return this.root ? this.root.system : null; }

    refreshTransform() {
      if (this.resolvedStamp === this.scene.time) return this.resolved;
      this.resolvedStamp = this.scene.time;
      const t = this.resolveTransform();
      this.resolved.origin = t.origin;
      this.resolved.scale = t.scale;
      this.resolved.angles = t.angles;
      return this.resolved;
    }

    // The 64-band stereo spectrum as one array (left bands, then right) for the systems' audio
    // driven emitters and operators.
    audio() {
      if (this.audioStamp === this.scene.time) return this.audioBins;
      this.audioStamp = this.scene.time;
      this.audioBins.set(this.scene.audio.left64, 0);
      this.audioBins.set(this.scene.audio.right64, 64);
      return this.audioBins;
    }

    async loadDefinition() {
      const raw = this.json.particle;
      if (typeof raw === 'string') {
        if (!raw.length) throw new Error('object ' + this.id + ': "particle" names an empty file');
        this.file = raw;
        return this.scene.loader.json(raw);
      }
      if (raw && typeof raw === 'object') {
        this.file = 'object ' + this.id + ' inline particle';
        return raw;
      }
      throw new Error('object ' + this.id + ': "particle" is neither a file name nor a definition object');
    }

    async setup() {
      const json = await this.loadDefinition();
      this.root = new Unit(this, this.file, json, 'object ' + this.id + ' (' + this.file + ')', () => this.refreshTransform(), null, null);
      await this.root.setup(0);
      this.defs = this.root.definitions([]);
      this.initialized = true;
    }

    // CParticle::applyParallaxToModelMatrix: depths below the minimum are pushed out to it.
    parallaxOffset() {
      const scene = this.scene;
      if (!scene.parallax.enabled.getBool()) return null;
      const amount = scene.parallax.amount.getNumber();
      const depth = this.parallaxDepth.getVec(2).slice(0, 2);
      for (let i = 0; i < 2; i++) if (Math.abs(depth[i]) < MINIMUM_PARTICLE_DEPTH) depth[i] = depth[i] < 0 ? -MINIMUM_PARTICLE_DEPTH : MINIMUM_PARTICLE_DEPTH;
      const d = scene.parallaxDisplacement, reference = scene.width;
      return [(depth[0] + amount) * d[0] * reference, (depth[1] + amount) * d[1] * reference];
    }

    // CParticle::update, with the frame time capped as the reference does; children share the clock.
    update(dt) {
      if (!this.initialized || !this.visible.getBool()) return;
      const step = Math.min(Math.max(dt, 0), DT_CAP);
      this.root.update(step, this.scene.time, this.scene.mouse.position);
    }

    render() {
      if (!this.initialized || !this.visible.getBool()) return;
      this.root.render(this.parallaxOffset());
    }

    setPaused(paused) { if (this.root) this.root.setPaused(paused); }

    dispose() {
      if (this.root) this.root.dispose();
      this.root = null;
    }
  }

  G.WEObjects.register('particle', Particle, 20);

  const api = { Particle, Unit, composeTransform, SPRITE_LAYOUT, ROPE_LAYOUT, MINIMUM_PARTICLE_DEPTH, MAX_CHILD_DEPTH, usesAudio };
  G.WEParticle = api;
  if (typeof module !== 'undefined' && module.exports) module.exports = api;
})();
