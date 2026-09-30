// Image layers: the quad, its composite framebuffers, the material copy pass, the effect chain
// with ping-pong buffers and targets, and the final draw into the scene. Port of
// Render/Objects/CImage.cpp and ObjectParser::parseImage/parseEffects/parseEffectPass.
(() => {
  'use strict';
  const G = typeof window !== 'undefined' ? window : globalThis;
  const M = G.WEM;

  const TEXCOORD_PASS = new Float32Array([0, 1, 0, 0, 1, 1, 1, 1, 0, 0, 1, 0]);
  const PASS_SPACE_POSITION = new Float32Array([-1, 1, 0, -1, -1, 0, 1, 1, 0, 1, 1, 0, -1, -1, 0, 1, -1, 0]);
  const COPY_MATERIAL_PASS = () => ({
    blending: G.WEMaterial.BLENDING.NORMAL, cullmode: G.WEMaterial.CULL.DISABLE,
    depthtest: G.WEMaterial.DEPTHTEST.DISABLED, depthwrite: G.WEMaterial.DEPTHWRITE.DISABLED,
    shader: 'commands/copy', textures: {}, usertextures: {}, textureUsers: {}, combos: {}, constants: {},
  });

  // ObjectParser::parseEffectPass: a scene.json override for one pass of an effect
  function parseEffectPassOverride(json, ctx, where) {
    if (!json || typeof json !== 'object') throw new Error(where + ': effect pass override is not an object');
    const textures = G.WEMaterial.parseTextureMap(json.textures, where + '.textures');
    const usertextures = G.WEMaterial.parseTextureMap(json.usertextures, where + '.usertextures');
    return {
      combos: G.WEMaterial.parseCombos(json.combos, where),
      constants: G.WEMaterial.parseConstants(json.constantshadervalues, ctx, where + '.constantshadervalues'),
      textures: textures.names,
      usertextures: usertextures.names,
      textureUsers: Object.assign({}, textures.users, usertextures.users),
      shaderOverride: null,
    };
  }

  class Image extends G.WEObjects.SceneObject {
    constructor(scene, json) {
      super(scene, json);
      this.alpha = this.setting('alpha', 'float', 1);
      this.color = this.setting('color', 'color', [1, 1, 1], true);
      this.brightness = this.setting('brightness', 'float', 1);
      this.parallaxDepth = this.setting('parallaxDepth', 'vec2', [0, 0]);
      this.colorBlendMode = this.setting('colorBlendMode', 'int', 0);
      this.sizeSetting = this.setting('size', 'vec2', [0, 0]);
      const align = json.horizontalalign !== undefined ? json.horizontalalign : json.alignment;
      this.alignment = typeof align === 'string' ? align : 'center';
      this.copyBackground = !!json.copybackground;
      this.perspective = !!json.perspective;
      this.model = null;
      this.material = null;
      this.effects = [];
      this.texture = null;
      this.ownsTexture = false;
      this.size = [0, 0];
      this.pos = [0, 0, 0, 0];
      this.sceneCenter = [0, 0, 0];
      this.mvpScreen = M.identity();
      this.mvpScreenInverse = M.identity();
      this.mvpPass = M.identity();
      this.mvpPassInverse = M.identity();
      this.mvpCopy = M.identity();
      this.mvpCopyInverse = M.identity();
      this.modelMatrix = M.identity();
      this.viewProjectionMatrix = M.identity();
      this.sceneSpace = new Float32Array(18);
      this.copySpace = new Float32Array(18);
      this.texcoordCopy = new Float32Array(12);
      this.buffers = null;
      this.mainFBO = null;
      this.subFBO = null;
      this.currentMainFBO = null;
      this.currentSubFBO = null;
      this.fboProvider = null;
      this.materialPasses = [];
      this.effectPasses = [];
      this.blendPass = null;
      this.activePasses = [];
      this.passesDirty = true;
      this.rebuilding = null;
      this.firstPassGeometry = null;
      // ITextureAnimation playback state for animated textures: the clock the passes read.
      this.textureAnimation = { rate: 1, playing: true, offset: 0, pausedAt: 0 };
    }

    // Seconds into the texture animation, honouring rate, pause and frame seeks.
    animationClock() {
      const a = this.textureAnimation;
      return (a.playing ? this.scene.time : a.pausedAt) * a.rate + a.offset;
    }

    ctx() { return { loader: this.scene.loader, properties: this.scene.properties }; }

    // ---- data loading (ObjectParser::parseImage, parseEffects) ------------------------------
    async loadModel() {
      const json = this.json, ctx = this.ctx();
      if (typeof json.image === 'string') return G.WEMaterial.loadModel(ctx, json.image);
      if (json.image && typeof json.image === 'object') return G.WEMaterial.parseModel(json.image, 'object ' + this.id + ' inline model', ctx);
      if (typeof json.material === 'string' || (json.material && typeof json.material === 'object')) {
        return G.WEMaterial.parseModel({ material: json.material }, 'object ' + this.id + ' material', ctx);
      }
      throw new Error('object ' + this.id + ' has neither an "image" model nor a "material"');
    }

    async loadEffects() {
      const list = this.json.effects;
      if (list === undefined || list === null) return [];
      if (!Array.isArray(list)) throw new Error('object ' + this.id + ': "effects" is not a list');
      const ctx = this.ctx();
      const effects = [];
      for (let i = 0; i < list.length; i++) {
        const cur = list[i];
        const where = 'object ' + this.id + ' effect ' + i;
        if (!cur || typeof cur !== 'object') throw new Error(where + ' is not an object');
        if (typeof cur.file !== 'string') throw new Error(where + ': image effect must have an effect file');
        const overrides = Array.isArray(cur.passes) ? cur.passes.map((p, k) => parseEffectPassOverride(p, ctx, where + ' pass ' + k)) : [];
        effects.push({
          id: typeof cur.id === 'number' ? cur.id : -1,
          name: typeof cur.name === 'string' ? cur.name : 'Effect without name',
          visible: G.WEProps.setting(cur.visible, { kind: 'bool', default: true, where: where + '.visible', properties: this.scene.properties }),
          passOverrides: overrides,
          effect: await G.WEMaterial.loadEffect(ctx, cur.file),
        });
      }
      return effects;
    }

    // ObjectParser::parseImage: "instance" textures land in the material's first pass.
    applyInstance() {
      const instance = this.json.instance;
      if (!instance || typeof instance !== 'object' || !this.material.passes.length) return;
      const first = this.material.passes[0];
      const where = 'object ' + this.id + '.instance';
      const textures = G.WEMaterial.parseTextureMap(instance.textures, where + '.textures');
      const usertextures = G.WEMaterial.parseTextureMap(instance.usertextures, where + '.usertextures');
      Object.assign(first.textures, textures.names);
      Object.assign(first.usertextures, usertextures.names);
      Object.assign(first.textureUsers, textures.users, usertextures.users);
    }

    // "blendmode" on the layer overrides the material's first pass blending.
    applyBlendMode() {
      const mode = this.json.blendmode;
      if (mode === undefined || mode === null) return;
      if (typeof mode !== 'string') throw new Error('object ' + this.id + ': blendmode ' + JSON.stringify(mode) + ' is not a blending name');
      if (!this.material.passes.length) return;
      this.material.passes[0].blending = G.WEMaterial.parseBlendMode(mode, 'object ' + this.id + '.blendmode');
    }

    // CRenderable::detectTexture: the material's first texture slot names the base texture;
    // "copybackground" layers read the scene framebuffer; layers without one get a blank one.
    async detectTexture() {
      const pass = this.material.passes[0];
      const keys = Object.keys(pass.textures).map((k) => parseInt(k, 10)).sort((a, b) => a - b);
      let name = keys.length ? pass.textures[keys[0]] : null;
      if (!name && this.copyBackground) name = '_rt_FullFrameBuffer';
      if (name) {
        const provider = G.WEPass.isFBOName(name) ? this.scene.findFBO(name) : await this.scene.textures.resolve(name);
        this.texture = new G.WETextures.TextureRef('object ' + this.id, provider);
        const property = pass.textureUsers[keys.length ? keys[0] : 0];
        if (property) this.bindTextureProperty(property, provider);
        return;
      }
      let size = this.sizeSetting.getVec(2);
      if (this.model.solidlayer && size[0] === 0 && size[1] === 0) size = [this.scene.width, this.scene.height];
      if (!(size[0] > 0 && size[1] > 0)) throw new Error('object ' + this.id + ' (' + this.name + ') has no texture and no size to draw with');
      this.texture = new G.WEFBO.FBO(this.scene.gl, '', 'rgba8888', 0, size[0], size[1], size[0], size[1]);
      this.ownsTexture = true;
    }

    // A texture user property on the base texture swaps it (the pass swaps its own slot too).
    bindTextureProperty(property, original) {
      const dyn = G.WEProps.setting({ value: '', user: property }, { kind: 'string', where: 'object ' + this.id + ' texture property ' + property, properties: this.scene.properties });
      const apply = (value) => {
        if (typeof value !== 'string' || !value.trim().length) { this.texture.current = original; return; }
        const name = value.trim().replace(/\.tex$/i, '');
        this.scene.textures.resolve(name).then((provider) => { this.texture.current = provider; }).catch((e) => this.scene.fail(e));
      };
      dyn.listen((value) => apply(value));
      if (dyn.get()) apply(dyn.get());
    }

    // ---- setup (CImage constructor + CImage::setup) ------------------------------------------
    async setup() {
      const gl = this.scene.gl;
      this.model = await this.loadModel();
      for (const hook of api.modelHooks) await hook(this);
      this.material = this.model.material;
      if (!this.material.passes.length) throw new Error('object ' + this.id + ': material ' + this.material.filename + ' has no passes');
      this.applyInstance();
      this.applyBlendMode();
      this.effects = await this.loadEffects();
      await this.detectTexture();
      this.fboProvider = new G.WEFBO.Provider(gl, this.scene.fbos);
      this.buffers = {
        sceneSpace: gl.createBuffer(), copySpace: gl.createBuffer(), passSpace: gl.createBuffer(),
        texcoordCopy: gl.createBuffer(), texcoordPass: gl.createBuffer(),
      };
      gl.bindBuffer(gl.ARRAY_BUFFER, this.buffers.passSpace);
      gl.bufferData(gl.ARRAY_BUFFER, PASS_SPACE_POSITION, gl.STATIC_DRAW);
      gl.bindBuffer(gl.ARRAY_BUFFER, this.buffers.texcoordPass);
      gl.bufferData(gl.ARRAY_BUFFER, TEXCOORD_PASS, gl.STATIC_DRAW);
      this.viewProjectionMatrix = M.identity();
      this.mvpPass = M.identity();
      this.mvpPassInverse = M.identity();
      this.updateGeometryBuffers();
      this.createCompositeFramebuffers();
      await this.buildPasses();
      const markDirty = () => { this.passesDirty = true; };
      this.visible.listen(markDirty);
      for (const effect of this.effects) effect.visible.listen(markDirty);
      this.colorBlendMode.listen(() => { this.rebuildBlendPass(); });
      this.initialized = true;
    }

    // CImage constructor: _rt_imageLayerComposite_<id>_a / _b, registered on the scene so
    // other layers can bind them.
    createCompositeFramebuffers() {
      const size = this.size;
      const flags = this.texture.flags;
      this.mainFBO = this.scene.fbos.create('_rt_imageLayerComposite_' + this.id + '_a', 'rgba8888', flags, size, size);
      this.subFBO = this.scene.fbos.create('_rt_imageLayerComposite_' + this.id + '_b', 'rgba8888', flags, size, size);
      this.currentMainFBO = this.mainFBO;
      this.currentSubFBO = this.subFBO;
      this.fboSize = [size[0], size[1]];
    }

    // CImage::setup: material passes, every effect's passes and the colour blending pass.
    async buildPasses() {
      const gl = this.scene.gl;
      const Mat = G.WEMaterial;
      this.materialPasses = [];
      for (const pass of this.material.passes) {
        const p = new G.WEPass.Pass(this, new G.WEFBO.Provider(gl, this.fboProvider), pass, null, null, null);
        await p.build();
        this.materialPasses.push(p);
      }
      this.effectPasses = [];
      for (const entry of this.effects) {
        const provider = new G.WEFBO.Provider(gl, this.fboProvider);
        for (const fbo of entry.effect.fbos) provider.createFromDefinition(fbo, this.texture.flags, this.size);
        const passes = [];
        let overrideIndex = 0;
        for (let i = 0; i < entry.effect.passes.length; i++) {
          const effectPass = entry.effect.passes[i];
          const where = entry.effect.filename + ' pass ' + i;
          if (!effectPass.material) {
            if (effectPass.command === null) throw new Error(where + ': pass without material and command is not supported');
            if (!effectPass.source) throw new Error(where + ': pass without material and source is not supported');
            if (!effectPass.target) throw new Error(where + ': pass without material and target is not supported');
            if (effectPass.command !== Mat.COMMAND.COPY) throw new Error(where + ': only the copy command is supported for a pass without material');
            const virtualPass = COPY_MATERIAL_PASS();
            virtualPass.textures[0] = effectPass.source;
            const p = new G.WEPass.Pass(this, provider, virtualPass, null, null, effectPass.target);
            await p.build();
            passes.push(p);
            continue;
          }
          const override = overrideIndex < entry.passOverrides.length ? entry.passOverrides[overrideIndex] : null;
          for (const pass of effectPass.material.passes) {
            const p = new G.WEPass.Pass(this, provider, pass, override, effectPass.binds, effectPass.target);
            await p.build();
            passes.push(p);
          }
          if (overrideIndex < entry.passOverrides.length) overrideIndex++;
        }
        this.effectPasses.push({ entry, provider, passes });
      }
      this.blendPass = await this.buildBlendPass();
      this.passesDirty = true;
    }

    // Extra pass when colorBlendMode > 0: materials/util/effectpassthrough.json with BLENDMODE.
    async buildBlendPass() {
      const mode = Math.trunc(this.colorBlendMode.getNumber());
      if (mode <= 0) return null;
      const material = await G.WEMaterial.loadMaterial(this.ctx(), 'materials/util/effectpassthrough.json');
      const override = { combos: { BLENDMODE: mode }, constants: {}, textures: {}, usertextures: {}, textureUsers: {}, shaderOverride: null };
      const p = new G.WEPass.Pass(this, new G.WEFBO.Provider(this.scene.gl, this.fboProvider), material.passes[0], override, null, null);
      await p.build();
      return p;
    }

    rebuildBlendPass() {
      const pending = this.buildBlendPass().then((pass) => {
        if (this.rebuilding !== pending) { if (pass) pass.dispose(); return; }
        if (this.blendPass) this.blendPass.dispose();
        this.blendPass = pass;
        this.passesDirty = true;
        this.rebuilding = null;
      });
      this.rebuilding = pending;
      pending.catch((e) => this.scene.fail(e));
    }

    // Composite and effect framebuffers follow the layer size (texture swaps, autosize).
    resizeFramebuffers() {
      const size = this.size;
      const pending = (async () => {
        this.disposePasses();
        this.mainFBO.dispose();
        this.subFBO.dispose();
        this.createCompositeFramebuffers();
        await this.buildPasses();
      })().then(() => { if (this.rebuilding === pending) this.rebuilding = null; });
      this.rebuilding = pending;
      pending.catch((e) => this.scene.fail(e));
      return size;
    }

    disposePasses() {
      for (const p of this.materialPasses) p.dispose();
      for (const e of this.effectPasses) { for (const p of e.passes) p.dispose(); e.provider.dispose(); }
      if (this.blendPass) this.blendPass.dispose();
      this.materialPasses = [];
      this.effectPasses = [];
      this.blendPass = null;
      this.activePasses = [];
    }

    // The passes that draw this frame: material copy, visible effects, colour blending.
    collectActivePasses() {
      const Mat = G.WEMaterial;
      const visibleEffects = this.effectPasses.filter((e) => e.entry.visible.getBool());
      if (this.model.passthrough && !visibleEffects.length) return [];
      const list = this.materialPasses.slice();
      for (const e of visibleEffects) for (const p of e.passes) list.push(p);
      if (this.blendPass) list.push(this.blendPass);
      for (const p of list) p.setBlendingMode(p.pass.blending);
      // with more than one pass the blend mode moves from the first pass to the last one
      if (list.length > 1) {
        list[list.length - 1].setBlendingMode(list[0].getBlendingMode());
        list[0].setBlendingMode(Mat.BLENDING.NORMAL);
      }
      return list;
    }

    // CImage::setupPasses
    setupPasses() {
      this.activePasses = this.collectActivePasses();
      this.currentMainFBO = this.mainFBO;
      this.currentSubFBO = this.subFBO;
      let drawTo = this.currentMainFBO;
      let asInput = this.texture;
      let texcoord = this.buffers.texcoordCopy;
      let first = true;
      let inTargetEffectSequence = false;
      let effectInput = null;
      const passes = this.activePasses;
      for (let i = 0; i < passes.length; i++) {
        const pass = passes[i];
        const prevDrawTo = drawTo;
        const isFirstPass = first;
        let spacePosition = isFirstPass ? (this.firstPassGeometry ? this.firstPassGeometry.positionBuffer : this.buffers.copySpace) : this.buffers.passSpace;
        let projection = isFirstPass ? this.mvpCopy : this.mvpPass;
        let inverseProjection = isFirstPass ? this.mvpCopyInverse : this.mvpPassInverse;
        first = false;
        if (isFirstPass && this.firstPassGeometry) {
          pass.setBlendingMode(G.WEMaterial.BLENDING.TRANSLUCENT);
          pass.setGeometryCallback(this.firstPassGeometry.setup, this.firstPassGeometry.draw, this.firstPassGeometry.cleanup);
        }
        pass.setModelMatrix(this.modelMatrix);
        pass.setViewProjectionMatrix(this.viewProjectionMatrix);
        const target = this.configurePassTarget(pass, asInput, inTargetEffectSequence, effectInput);
        const writesToTarget = target !== null;
        if (writesToTarget) {
          drawTo = target.fbo;
          inTargetEffectSequence = true;
          effectInput = target.effectInput;
        }
        if (!writesToTarget && i === passes.length - 1 && this.visible.getBool()) {
          spacePosition = this.buffers.sceneSpace;
          drawTo = this.scene.fbo;
          projection = this.mvpScreen;
          inverseProjection = this.mvpScreenInverse;
        }
        pass.setDestination(drawTo);
        pass.setInput(asInput);
        pass.setPreviousInput(inTargetEffectSequence ? effectInput : null);
        pass.setPosition(spacePosition);
        pass.setTexCoord(texcoord);
        pass.setModelViewProjectionMatrix(projection);
        pass.setModelViewProjectionMatrixInverse(inverseProjection);
        texcoord = this.buffers.texcoordPass;
        if (writesToTarget) {
          asInput = drawTo;
          drawTo = prevDrawTo;
        } else {
          drawTo = prevDrawTo;
          const swapped = this.pingpongFramebuffer();
          drawTo = swapped.drawTo;
          asInput = swapped.asInput;
          inTargetEffectSequence = false;
          effectInput = null;
        }
      }
      this.passesDirty = false;
    }

    // CImage::configurePassTarget: a pass with a target draws into that FBO (effect-local, then
    // image, then scene) and starts or continues a target sequence.
    configurePassTarget(pass, asInput, inTargetEffectSequence, effectInput) {
      if (!pass.target) return null;
      let resolved = pass.fboProvider.find(pass.target);
      if (!resolved) resolved = this.scene.fbos.find(pass.target);
      if (!resolved) throw new Error('pass target FBO "' + pass.target + '" could not be resolved for object ' + this.id + ' shader ' + pass.pass.shader);
      return { fbo: resolved, effectInput: inTargetEffectSequence ? effectInput : asInput };
    }

    // CImage::pinpongFramebuffer
    pingpongFramebuffer() {
      const currentMain = this.currentMainFBO, currentSub = this.currentSubFBO;
      this.currentMainFBO = currentSub;
      this.currentSubFBO = currentMain;
      return { drawTo: currentSub, asInput: currentMain };
    }

    // ---- geometry (CImage::resolveGeometrySize, updateScenePosition, uploadGeometryBuffers) --
    // Layer size: the scene's "size" unless the model autosizes to its texture; a zero size
    // comes from the texture, then the model's width/height, fullscreen layers span the scene.
    resolveGeometrySize(sceneWidth, sceneHeight, origin) {
      let size = this.sizeSetting.getVec(2);
      if (this.model.autosize || size[0] === 0 || size[1] === 0) {
        if (this.texture) size = [this.texture.realWidth, this.texture.realHeight];
      }
      if ((size[0] === 0 || size[1] === 0) && this.model.width !== null && this.model.height !== null) size = [this.model.width, this.model.height];
      if (this.model.fullscreen) {
        size = [sceneWidth, sceneHeight];
        origin[0] = sceneWidth / 2; origin[1] = sceneHeight / 2; origin[2] = 0;
      }
      return size;
    }

    updateScenePosition(origin, size, scale, sceneWidth, sceneHeight) {
      const sx = size[0] * scale[0], sy = size[1] * scale[1];
      const pos = this.pos;
      pos[0] = origin[0] - sx / 2;
      pos[3] = origin[1] + sy / 2;
      pos[2] = origin[0] + sx / 2;
      pos[1] = origin[1] - sy / 2;
      if (this.alignment.indexOf('top') >= 0) { pos[1] -= sy / 2; pos[3] -= sy / 2; }
      else if (this.alignment.indexOf('bottom') >= 0) { pos[1] += sy / 2; pos[3] += sy / 2; }
      if (this.alignment.indexOf('left') >= 0) { pos[0] += sx / 2; pos[2] += sx / 2; }
      else if (this.alignment.indexOf('right') >= 0) { pos[0] -= sx / 2; pos[2] -= sx / 2; }
      pos[0] -= sceneWidth / 2;
      pos[1] = sceneHeight / 2 - pos[1];
      pos[2] -= sceneWidth / 2;
      pos[3] = sceneHeight / 2 - pos[3];
    }

    uploadGeometryBuffers(size) {
      const gl = this.scene.gl;
      const pos = this.pos;
      this.sceneSpace.set([pos[0], pos[1], 0, pos[0], pos[3], 0, pos[2], pos[1], 0, pos[2], pos[1], 0, pos[0], pos[3], 0, pos[2], pos[3], 0]);
      let width = 1, height = 1;
      const tex = this.texture;
      if (tex && !tex.animated && (tex.textureWidth(0) !== tex.realWidth || tex.textureHeight(0) !== tex.realHeight)) {
        width = tex.realWidth / tex.textureWidth(0);
        height = tex.realHeight / tex.textureHeight(0);
      }
      const x = 0, y = 0;
      let realWidth = size[0], realHeight = size[1], realX = 0, realY = 0;
      if (this.model.passthrough) {
        width = 1; height = 1;
        realX = pos[0]; realY = pos[3]; realWidth = pos[2]; realHeight = pos[1];
        if (this.model.fullscreen) { realX = -1; realY = -1; realWidth = 1; realHeight = 1; }
      }
      this.texcoordCopy.set([x, height, x, y, width, height, width, height, x, y, width, y]);
      this.copySpace.set([realX, realHeight, 0, realX, realY, 0, realWidth, realHeight, 0, realWidth, realHeight, 0, realX, realY, 0, realWidth, realY, 0]);
      gl.bindBuffer(gl.ARRAY_BUFFER, this.buffers.sceneSpace);
      gl.bufferData(gl.ARRAY_BUFFER, this.sceneSpace, gl.DYNAMIC_DRAW);
      gl.bindBuffer(gl.ARRAY_BUFFER, this.buffers.copySpace);
      gl.bufferData(gl.ARRAY_BUFFER, this.copySpace, gl.DYNAMIC_DRAW);
      gl.bindBuffer(gl.ARRAY_BUFFER, this.buffers.texcoordCopy);
      gl.bufferData(gl.ARRAY_BUFFER, this.texcoordCopy, gl.DYNAMIC_DRAW);
      this.sceneCenter = [(pos[0] + pos[2]) / 2, (pos[1] + pos[3]) / 2, 0];
      if (this.model.passthrough) {
        this.mvpCopy.set(this.mvpScreen);
      } else {
        this.mvpCopy.set(M.ortho(0, size[0], 0, size[1], -1, 1));
      }
      this.mvpCopyInverse.set(M.inverse(this.mvpCopy));
      this.modelMatrix.set(M.ortho(0, size[0], 0, size[1], -1, 1));
    }

    // CImage::updateGeometryBuffers
    updateGeometryBuffers() {
      const sceneWidth = this.scene.width, sceneHeight = this.scene.height;
      const transform = this.resolveTransform();
      const origin = transform.origin.slice();
      const size = this.resolveGeometrySize(sceneWidth, sceneHeight, origin);
      const previous = this.size;
      this.size = size;
      if (this.mainFBO && (size[0] !== this.fboSize[0] || size[1] !== this.fboSize[1]) && !this.rebuilding) this.resizeFramebuffers();
      if (this.firstPassGeometry && (size[0] !== previous[0] || size[1] !== previous[1])) this.firstPassGeometry.resize(size);
      this.updateScenePosition(origin, size, transform.scale, sceneWidth, sceneHeight);
      this.uploadGeometryBuffers(size);
      return transform;
    }

    // CImage::updateScreenSpacePosition: rotation about the layer centre (angles in radians,
    // x and z negated for the y-flipped space), the camera, then parallax.
    updateScreenSpacePosition() {
      const transform = this.updateGeometryBuffers();
      const angles = transform.angles;
      let rotModel = M.identity();
      if (angles[0] !== 0 || angles[1] !== 0 || angles[2] !== 0) {
        const c = this.sceneCenter;
        rotModel = M.translate(rotModel, c[0], c[1], c[2]);
        if (angles[2] !== 0) rotModel = M.rotate(rotModel, -angles[2], 0, 0, 1);
        if (angles[1] !== 0) rotModel = M.rotate(rotModel, angles[1], 0, 1, 0);
        if (angles[0] !== 0) rotModel = M.rotate(rotModel, -angles[0], 1, 0, 0);
        rotModel = M.translate(rotModel, -c[0], -c[1], -c[2]);
      }
      const camera = this.scene.camera;
      let mvp = M.multiply(this.perspective ? camera.perspectiveViewProjection : camera.viewProjection, rotModel);
      if (this.scene.parallax.enabled.getBool()) {
        const amount = this.scene.parallax.amount.getNumber();
        const depth = this.parallaxDepth.getVec(2);
        const displacement = this.scene.parallaxDisplacement;
        const reference = this.scene.width;
        const x = (depth[0] + amount) * displacement[0] * reference;
        const y = (depth[1] + amount) * displacement[1] * reference;
        mvp = M.translate(mvp, x, y, 0);
      }
      this.mvpScreen.set(mvp);
      this.mvpScreenInverse.set(M.inverse(mvp));
      if (this.model.passthrough) {
        this.mvpCopy.set(this.mvpScreen);
        this.mvpCopyInverse.set(this.mvpScreenInverse);
      }
    }

    // CImage::render
    render() {
      if (!this.initialized || this.rebuilding) return;
      if (!this.visible.getBool()) return;
      const gl = this.scene.gl;
      gl.colorMask(true, true, true, true);
      this.updateScreenSpacePosition();
      if (this.passesDirty) this.setupPasses();
      const passes = this.activePasses;
      // A mesh covers only part of the composite buffer: start every frame from transparent.
      if (this.firstPassGeometry && passes.length > 1) passes[0].drawTo.clear();
      for (let i = 0; i < passes.length; i++) {
        if (i === passes.length - 1) gl.colorMask(true, true, true, false);
        passes[i].render();
      }
      gl.colorMask(true, true, true, true);
    }

    setPaused(paused) { if (this.texture) this.texture.setPaused(paused); }

    // CRenderable / CImage accessors the passes read
    getBrightness() { return this.brightness.getNumber(); }
    getUserAlpha() { return this.alpha.getNumber(); }
    getAlpha() { return this.alpha.getNumber(); }
    getColor() { return this.color.getVec(3); }
    getColor4() { return this.color.getVec(4); }
    getCompositeColor() { return this.color.getVec(3); }
    get animationTime() { return this.texture ? this.texture.animationTime : 0; }

    /**
     * Extension point for custom first-pass geometry (puppet meshes): `positionBuffer` replaces
     * the copy-space quad, `setup(pass)`, `draw(pass)`, `cleanup(pass)` replace the attribute
     * binding and draw call, `resize(size)` is told when the layer size changes.
     */
    setFirstPassGeometry(geometry) {
      this.firstPassGeometry = geometry;
      this.passesDirty = true;
    }

    dispose() {
      this.disposePasses();
      if (this.mainFBO) this.mainFBO.dispose();
      if (this.subFBO) this.subFBO.dispose();
      if (this.ownsTexture && this.texture) this.texture.dispose();
      if (this.buffers) for (const b of Object.values(this.buffers)) this.scene.gl.deleteBuffer(b);
      if (this.fboProvider) this.fboProvider.dispose();
    }
  }

  G.WEObjects.register('image', Image, 10);
  G.WEObjects.register('material', Image, -1);

  /**
   * `modelHooks`: `async (image) => {}` callbacks run in setup() once `image.model` is loaded and
   * before its material passes are built (puppet warps attach their meshes and combos here).
   */
  const api = { Image, parseEffectPassOverride, TEXCOORD_PASS, PASS_SPACE_POSITION, modelHooks: [] };
  G.WEImage = api;
  if (typeof module !== 'undefined' && module.exports) module.exports = api;
})();
