// The scene: scene.json parsing, the camera, objects in dependency order, the frame loop, the
// scene framebuffer, bloom, the present pass to the canvas, the host's property/audio/pointer
// events and the error overlay. Port of Render/Wallpapers/CScene.cpp, Render/CWallpaper.cpp
// and Data/Parsers/WallpaperParser.cpp.
(() => {
  'use strict';
  const G = typeof window !== 'undefined' ? window : globalThis;
  const M = G.WEM;

  const LIGHT_SLOTS = 16;
  const BLOOM_ID = -1;

  // ---- error overlay ---------------------------------------------------------------------------
  function showError(err) {
    console.error(err);
    if (typeof document === 'undefined') return;
    let el = document.getElementById('we-error');
    if (!el) {
      el = document.createElement('pre');
      el.id = 'we-error';
      el.style.cssText = 'position:fixed;left:0;top:0;right:0;bottom:0;margin:0;padding:24px;overflow:auto;background:#000;color:#f66;font:14px/1.4 monospace;white-space:pre-wrap;z-index:10';
      document.body.appendChild(el);
    }
    const text = err && err.stack ? err.stack : String(err);
    el.textContent += (el.textContent ? '\n\n' : '') + 'Wallpaper Engine scene error\n\n' + text;
  }

  // ---- WallpaperParser::parseScene ------------------------------------------------------------
  function requireObject(parent, key, message) {
    const v = parent[key];
    if (!v || typeof v !== 'object' || Array.isArray(v)) throw new Error('scene.json: ' + message);
    return v;
  }

  function parseSceneData(json, properties) {
    if (!json || typeof json !== 'object') throw new Error('scene.json is not an object');
    const camera = requireObject(json, 'camera', 'scenes must have a camera section');
    const general = requireObject(json, 'general', 'scenes must have a general section');
    const projection = requireObject(general, 'orthogonalprojection', 'general section must have orthogonal projection info');
    if (!Array.isArray(json.objects)) throw new Error('scene.json: scenes must have an objects section');
    const setting = (obj, prefix, key, kind, dflt, expectColor) => G.WEProps.setting(obj[key], { kind, default: dflt, expectColor: !!expectColor, where: 'scene.json ' + prefix + '.' + key, properties });
    const g = (key, kind, dflt, expectColor) => setting(general, 'general', key, kind, dflt, expectColor);
    const c = (key, kind, dflt) => setting(camera, 'camera', key, kind, dflt);
    const requiredVec3 = (key, message) => {
      if (camera[key] === undefined) throw new Error('scene.json: ' + message);
      return G.WEProps.toVec(camera[key], 3);
    };
    const isAuto = !!projection.auto;
    const requiredInt = (key, message) => {
      if (isAuto) return 0;
      if (typeof projection[key] !== 'number') throw new Error('scene.json: ' + message);
      return Math.trunc(projection[key]);
    };
    return {
      colors: {
        ambient: g('ambientcolor', 'color', [0, 0, 0], true),
        skylight: g('skylightcolor', 'color', [0, 0, 0], true),
        clear: g('clearcolor', 'color', [1, 1, 1], true),
      },
      camera: {
        fade: g('camerafade', 'bool', false),
        bloom: {
          enabled: g('bloom', 'bool', false),
          strength: g('bloomstrength', 'float', 0),
          threshold: g('bloomthreshold', 'float', 0),
          hdrFeather: g('bloomhdrfeather', 'float', 0),
          hdrScatter: g('bloomhdrscatter', 'float', 0),
          hdrStrength: g('bloomhdrstrength', 'float', 0),
          hdrThreshold: g('bloomhdrthreshold', 'float', 0),
          hdrIterations: g('bloomhdriterations', 'int', 0),
        },
        parallax: {
          enabled: g('cameraparallax', 'bool', false),
          amount: g('cameraparallaxamount', 'float', 1),
          delay: g('cameraparallaxdelay', 'float', 0),
          mouseInfluence: g('cameraparallaxmouseinfluence', 'float', 1),
        },
        shake: {
          enabled: g('camerashake', 'bool', false),
          amplitude: g('camerashakeamplitude', 'float', 0),
          roughness: g('camerashakeroughness', 'float', 0),
          speed: g('camerashakespeed', 'float', 0),
        },
        configuration: {
          center: requiredVec3('center', 'camera must have a center position'),
          eye: requiredVec3('eye', 'camera must have an eye position'),
          up: requiredVec3('up', 'camera must have an up position'),
        },
        projection: {
          width: requiredInt('width', 'projection must have a width'),
          height: requiredInt('height', 'projection must have a height'),
          isAuto,
          nearz: c('nearz', 'float', 0),
          farz: c('farz', 'float', 1000),
          fov: c('fov', 'float', 50),
        },
      },
      objects: json.objects,
    };
  }

  // CScene constructor: an "auto" projection spans the images' extents, else the window.
  function autoProjectionSize(objects, fallbackWidth, fallbackHeight) {
    let maxX = 0, maxY = 0;
    for (const o of objects) {
      if (!o || typeof o.image !== 'string' || o.origin === undefined) continue;
      const origin = G.WEProps.toVec(o.origin, 3);
      const size = o.size === undefined ? [0, 0] : G.WEProps.toVec(o.size, 2);
      maxX = Math.max(maxX, Math.abs(origin[0]) + size[0] / 2);
      maxY = Math.max(maxY, Math.abs(origin[1]) + size[1] / 2);
    }
    if (maxX > 0 && maxY > 0) return [maxX * 2, maxY * 2];
    return [fallbackWidth, fallbackHeight];
  }

  // Modules loaded after this one register here: `onLoad(fn)` runs `await fn(scene)` once the
  // scene is loaded, `onGeneralSettings(fn)` on every applyGeneralProperties from the host.
  const loadHooks = [];
  const generalHooks = [];

  const PRESENT_VS = '#version 300 es\nprecision highp float;\nin vec3 a_Position;\nin vec2 a_TexCoord;\nout vec2 v_TexCoord;\nvoid main () {\ngl_Position = vec4 (a_Position, 1.0);\nv_TexCoord = a_TexCoord;\n}';
  const PRESENT_FS = '#version 300 es\nprecision highp float;\nuniform sampler2D g_Texture0;\nuniform float u_Fade;\nin vec2 v_TexCoord;\nout vec4 out_FragColor;\nvoid main () {\nvec4 c = texture (g_Texture0, v_TexCoord);\nout_FragColor = vec4 (c.rgb * u_Fade, 1.0);\n}';
  const PRESENT_POSITION = new Float32Array([-1, 1, 0, 1, 1, 0, -1, -1, 0, -1, -1, 0, 1, 1, 0, 1, -1, 0]);

  class Scene {
    constructor(canvas, loader) {
      this.canvas = canvas;
      this.loader = loader;
      const gl = canvas.getContext('webgl2', { alpha: false, antialias: false, depth: true, stencil: false, premultipliedAlpha: false, preserveDrawingBuffer: false, powerPreference: 'high-performance' });
      if (!gl) throw new Error('WebGL2 is not available in this web view');
      this.gl = gl;
      this.properties = new G.WEProps.PropertyStore();
      this.textures = new G.WETextures.Cache(gl, loader, this.properties, (e) => this.fail(e));
      this.fbos = new G.WEFBO.Provider(gl, null);
      this.objects = new Map();
      this.renderOrder = [];
      this.updateHooks = [];
      this.viewport = new G.WECamera.Viewport();
      this.mouse = this.viewport.mouse;
      this.time = 0;
      this.timeLast = 0;
      this.dt = 0;
      this.daytime = 0;
      this.startStamp = null;
      this.audio = { left16: new Float32Array(16), right16: new Float32Array(16), left32: new Float32Array(32), right32: new Float32Array(32), left64: new Float32Array(64), right64: new Float32Array(64) };
      this.audioRegistered = false;
      this.lights = { positions: new Float32Array(3 * LIGHT_SLOTS), colors: new Float32Array(3 * LIGHT_SLOTS), radii: new Float32Array(LIGHT_SLOTS) };
      this.parallaxDisplacement = new Float32Array(2);
      this.texelSize = new Float32Array(2);
      this.texelSizeHalf = new Float32Array(2);
      this.screen = new Float32Array(4);
      this.paused = false;
      this.fpsLimit = 0;
      this.lastFrameStamp = 0;
      this.frameQueued = false;
      this.failed = false;
      this.bloomObject = null;
      this.bloomLoading = null;
      this.media = null;
      this.width = 0;
      this.height = 0;
      this.loaded = false;
      this.clearEnabled = true;
      this.projectProperties = {};
      this.generalSettings = {};
    }

    ctx() { return { loader: this.loader, properties: this.properties }; }

    fail(err) {
      this.failed = true;
      showError(err);
    }

    // ---- loading --------------------------------------------------------------------------------
    async load(sceneRel) {
      const gl = this.gl;
      await this.loadProject();
      const json = await this.loader.json(sceneRel);
      const data = parseSceneData(json, this.properties);
      this.data = data;
      this.colors = data.colors;
      this.bloom = data.camera.bloom;
      this.camera = new G.WECamera.Camera(Object.assign({}, data.camera.configuration, data.camera.projection));
      let width = data.camera.projection.width, height = data.camera.projection.height;
      if (data.camera.projection.isAuto) {
        const dpr = G.devicePixelRatio || 1;
        [width, height] = autoProjectionSize(data.objects, Math.round(G.innerWidth * dpr), Math.round(G.innerHeight * dpr));
      }
      if (!(width > 0 && height > 0)) throw new Error('scene.json: orthogonal projection is ' + width + 'x' + height);
      this.width = width;
      this.height = height;
      this.camera.setOrthogonalProjection(width, height);
      this.parallax = new G.WECamera.Parallax(data.camera.parallax);
      this.parallaxDisplacement = this.parallax.displacement;
      this.shake = new G.WECamera.Shake(data.camera.shake);
      this.fade = new G.WECamera.Fade(data.camera.fade);
      this.texelSize.set([1 / width, 1 / height]);
      this.texelSizeHalf.set([0.5 / width, 0.5 / height]);
      this.screen.set([width, height, width / height, 1]);
      this.setupFramebuffers();
      for (const o of data.objects) await this.createObject(o);
      for (const o of data.objects) this.addObjectToRenderOrder(o);
      // CScene constructor: bloom framebuffers at quarter and eighth resolution
      this.fbos.create('_rt_4FrameBuffer', 'rgba8888', G.WEFBO.FLAG.CLAMP_UVS, [width / 4, height / 4], [width / 4, height / 4]);
      this.fbos.create('_rt_8FrameBuffer', 'rgba8888', G.WEFBO.FLAG.CLAMP_UVS, [width / 8, height / 8], [width / 8, height / 8]);
      this.fbos.create('_rt_Bloom', 'rgba8888', G.WEFBO.FLAG.CLAMP_UVS, [width / 8, height / 8], [width / 8, height / 8]);
      this.setupPresent();
      if (this.bloom.enabled.getBool()) await this.ensureBloom();
      this.bloom.enabled.listen(() => { if (this.bloom.enabled.getBool()) this.ensureBloom().catch((e) => this.fail(e)); });
      if (this.supportsAudioProcessing) this.requireAudio();
      gl.bindFramebuffer(gl.FRAMEBUFFER, null);
      this.loaded = true;
      for (const fn of loadHooks) await fn(this);
    }

    // project.json is optional for rendering: it seeds property defaults and declares audio use.
    async loadProject() {
      this.supportsAudioProcessing = false;
      let project = null;
      try {
        project = await this.loader.json('project.json');
      } catch (e) {
        return;
      }
      const general = project && project.general && typeof project.general === 'object' ? project.general : {};
      this.supportsAudioProcessing = !!general.supportsaudioprocessing;
      const props = general.properties && typeof general.properties === 'object' ? general.properties : {};
      this.projectProperties = props;
      const seed = {};
      for (const [name, p] of Object.entries(props)) if (p && typeof p === 'object' && 'value' in p) seed[name] = { value: p.value };
      this.properties.apply(seed);
    }

    // CWallpaper::setupFramebuffers + CScene's shadow atlas
    setupFramebuffers() {
      const w = this.width, h = this.height;
      const clamp = G.WEFBO.FLAG.CLAMP_UVS;
      this.fbo = this.fbos.create('_rt_FullFrameBuffer', 'rgba8888', clamp, [w, h], [w, h]);
      this.fbos.alias('_rt_MipMappedFrameBuffer', '_rt_FullFrameBuffer');
      this.fbos.create('_rt_shadowAtlas', 'rgba8888', clamp, [w, h], [w, h]);
      this.fbos.alias('_alias_lightCookie', '_rt_shadowAtlas');
    }

    // CScene::createObject: dependencies and the parent are created first.
    async createObject(json) {
      if (!json || typeof json !== 'object' || typeof json.id !== 'number') throw new Error('scene.json object without an id: ' + JSON.stringify(json).slice(0, 120));
      if (this.objects.has(json.id)) return this.objects.get(json.id);
      for (const dep of G.WEObjects.parseDependencies(json)) {
        if (dep === json.id) continue;
        const found = this.data.objects.find((o) => o && o.id === dep);
        if (found) await this.createObject(found);
      }
      if (typeof json.parent === 'number') {
        const parent = this.data.objects.find((o) => o && o.id === json.parent);
        if (!parent) throw new Error('cannot find parent ' + json.parent + ' for object ' + json.id);
        await this.createObject(parent);
      }
      const object = G.WEObjects.create(this, json);
      await object.setup();
      this.objects.set(object.id, object);
      return object;
    }

    // CScene::addObjectToRenderOrder
    addObjectToRenderOrder(json) {
      const object = this.objects.get(json.id);
      if (!object) return;
      for (const dep of object.dependencies) {
        if (dep === json.id) continue;
        const found = this.data.objects.find((o) => o && o.id === dep);
        if (!found) throw new Error('cannot find dependency ' + dep + ' for object ' + json.id);
        this.addObjectToRenderOrder(found);
      }
      if (!this.renderOrder.includes(object)) this.renderOrder.push(object);
    }

    // CScene constructor: the bloom image (id -1) that reads the scene, blurs it and combines.
    async ensureBloom() {
      if (this.bloomObject || this.bloomLoading) return this.bloomLoading;
      const w = this.width, h = this.height;
      const constants = {
        bloomstrength: this.bloom.strength, bloomthreshold: this.bloom.threshold,
        bloomhdrfeather: this.bloom.hdrFeather, bloomhdrscatter: this.bloom.hdrScatter, bloomhdrstrength: this.bloom.hdrStrength,
        bloomhdrthreshold: this.bloom.hdrThreshold, bloomhdriterations: this.bloom.hdrIterations,
      };
      const json = {
        image: 'models/wpenginelinux.json', name: 'bloomimagewpenginelinux', visible: true, scale: '1.0 1.0 1.0', angles: '0.0 0.0 0.0',
        origin: (w / 2) + ' ' + (h / 2) + ' 0', size: w + ' ' + h, id: BLOOM_ID,
        effects: [{ file: 'effects/wpenginelinux/bloomeffect.json', id: 15242000, name: '', passes: [
          { constantshadervalues: constants }, { constantshadervalues: constants }, { constantshadervalues: constants }, { constantshadervalues: constants },
        ] }],
      };
      this.bloomLoading = (async () => {
        const object = G.WEObjects.create(this, json);
        await object.setup();
        this.bloomObject = object;
        this.bloomLoading = null;
      })();
      return this.bloomLoading;
    }

    // CWallpaper::setupShaders: the pass that draws the scene texture onto the canvas.
    setupPresent() {
      const gl = this.gl;
      const compile = (type, src) => {
        const s = gl.createShader(type);
        gl.shaderSource(s, src);
        gl.compileShader(s);
        if (!gl.getShaderParameter(s, gl.COMPILE_STATUS)) throw new Error('present shader: ' + gl.getShaderInfoLog(s));
        return s;
      };
      const program = gl.createProgram();
      gl.attachShader(program, compile(gl.VERTEX_SHADER, PRESENT_VS));
      gl.attachShader(program, compile(gl.FRAGMENT_SHADER, PRESENT_FS));
      gl.linkProgram(program);
      if (!gl.getProgramParameter(program, gl.LINK_STATUS)) throw new Error('present program: ' + gl.getProgramInfoLog(program));
      this.present = {
        program,
        vao: gl.createVertexArray(),
        position: gl.createBuffer(),
        texcoord: gl.createBuffer(),
        aPosition: gl.getAttribLocation(program, 'a_Position'),
        aTexCoord: gl.getAttribLocation(program, 'a_TexCoord'),
        uTexture: gl.getUniformLocation(program, 'g_Texture0'),
        uFade: gl.getUniformLocation(program, 'u_Fade'),
        texcoords: new Float32Array(12),
      };
      gl.bindBuffer(gl.ARRAY_BUFFER, this.present.position);
      gl.bufferData(gl.ARRAY_BUFFER, PRESENT_POSITION, gl.STATIC_DRAW);
    }

    // ---- host integration -----------------------------------------------------------------------
    installListeners() {
      G.wallpaperPropertyListener = {
        applyUserProperties: (props) => { try { this.properties.apply(props); } catch (e) { this.fail(e); } },
        applyGeneralProperties: (general) => {
          if (general && typeof general.fps === 'number') this.fpsLimit = general.fps;
          if (general && typeof general === 'object') Object.assign(this.generalSettings, general);
          for (const fn of generalHooks) fn(general || {});
        },
        setPaused: (paused) => this.setPaused(!!paused),
      };
      const pointer = (e) => { this.viewport.setPointer(e.clientX, e.clientY, G.innerWidth, G.innerHeight); };
      G.addEventListener('mousemove', pointer);
      G.addEventListener('mousedown', pointer);
      G.addEventListener('mouseup', pointer);
    }

    // g_AudioSpectrum*: 64 bands per channel from the host, averaged down to 32 and 16.
    requireAudio() {
      if (this.audioRegistered) return;
      this.audioRegistered = true;
      if (typeof G.wallpaperRegisterAudioListener !== 'function') throw new Error('the host page offers no wallpaperRegisterAudioListener, which this scene needs for its audio spectrum');
      G.wallpaperRegisterAudioListener((bins) => {
        const a = this.audio;
        for (let i = 0; i < 64; i++) { a.left64[i] = bins[i] || 0; a.right64[i] = bins[64 + i] || 0; }
        for (let i = 0; i < 32; i++) { a.left32[i] = (a.left64[2 * i] + a.left64[2 * i + 1]) / 2; a.right32[i] = (a.right64[2 * i] + a.right64[2 * i + 1]) / 2; }
        for (let i = 0; i < 16; i++) { a.left16[i] = (a.left32[2 * i] + a.left32[2 * i + 1]) / 2; a.right16[i] = (a.right32[2 * i] + a.right32[2 * i + 1]) / 2; }
      });
    }

    // Sound layers keep their media elements in the document so the host's volume reaches them.
    mediaContainer() {
      if (!this.media) {
        this.media = document.createElement('div');
        this.media.id = 'we-media';
        this.media.hidden = true;
        document.body.appendChild(this.media);
      }
      return this.media;
    }

    setPaused(paused) {
      this.paused = paused;
      this.textures.setPaused(paused);
      for (const object of this.objects.values()) object.setPaused(paused);
      if (!paused) { this.lastFrameStamp = 0; this.schedule(); }
    }

    getObject(id) { return this.objects.get(id) || null; }

    // CWallpaper::findFBO
    findFBO(name) {
      const fbo = this.fbos.find(name);
      if (!fbo) throw new Error('cannot find FBO ' + name);
      return fbo;
    }

    /** Per-frame hook `fn(dt, time)` run before objects update; returns the unsubscribe function. */
    onUpdate(fn) {
      this.updateHooks.push(fn);
      return () => { this.updateHooks = this.updateHooks.filter((f) => f !== fn); };
    }

    // ---- frame loop -----------------------------------------------------------------------------
    // The hosts unpause can arrive while load() is still awaiting objects
    schedule() {
      if (!this.loaded || this.frameQueued || this.paused || this.failed) return;
      this.frameQueued = true;
      G.requestAnimationFrame((stamp) => { this.frameQueued = false; this.frame(stamp); });
    }

    frame(stamp) {
      if (this.paused || this.failed) return;
      if (this.fpsLimit > 0 && this.lastFrameStamp && stamp - this.lastFrameStamp < 1000 / this.fpsLimit - 0.5) { this.schedule(); return; }
      this.lastFrameStamp = stamp;
      try {
        this.renderFrame(stamp);
      } catch (e) {
        this.fail(e);
        return;
      }
      this.schedule();
    }

    updateLights() {
      const l = this.lights;
      l.positions.fill(0); l.colors.fill(0); l.radii.fill(0);
      let n = 0;
      for (const object of this.renderOrder) {
        if (!(object instanceof G.WEObjects.Light) || !object.visible.getBool() || n >= LIGHT_SLOTS) continue;
        const p = object.worldPosition(), c = object.premultipliedColor();
        l.positions[3 * n] = p[0]; l.positions[3 * n + 1] = p[1]; l.positions[3 * n + 2] = p[2];
        l.colors[3 * n] = c[0]; l.colors[3 * n + 1] = c[1]; l.colors[3 * n + 2] = c[2];
        l.radii[n] = object.radius.getNumber();
        n++;
      }
    }

    resizeCanvas() {
      const dpr = G.devicePixelRatio || 1;
      const w = Math.max(1, Math.round(G.innerWidth * dpr)), h = Math.max(1, Math.round(G.innerHeight * dpr));
      if (this.canvas.width !== w || this.canvas.height !== h) { this.canvas.width = w; this.canvas.height = h; }
    }

    // CScene::renderFrame + WallpaperApplication's per-frame globals
    renderFrame(stamp) {
      const gl = this.gl;
      if (this.startStamp === null) this.startStamp = stamp;
      this.timeLast = this.time;
      this.time = (stamp - this.startStamp) / 1000;
      this.dt = this.time - this.timeLast;
      const now = new Date();
      this.daytime = (now.getHours() * 60 + now.getMinutes()) / (24 * 60);
      this.resizeCanvas();
      this.viewport.update(this.canvas.width, this.canvas.height, this.width, this.height);
      this.viewport.updateMouse();
      this.parallax.update(this.dt, this.mouse.position);
      this.shake.update(this.time);
      this.camera.updateFrame(this.shake.offset);
      for (const fn of this.updateHooks) fn(this.dt, this.time);
      this.textures.update();
      for (const object of this.renderOrder) object.update(this.dt);
      if (this.bloomObject) this.bloomObject.update(this.dt);
      this.updateLights();
      gl.bindFramebuffer(gl.FRAMEBUFFER, this.fbo.framebuffer);
      gl.viewport(0, 0, this.fbo.realWidth, this.fbo.realHeight);
      gl.disable(gl.SCISSOR_TEST);
      gl.colorMask(true, true, true, true);
      gl.depthMask(true);
      const clear = this.colors.clear.getVec(3);
      gl.clearColor(clear[0], clear[1], clear[2], 1);
      gl.clear(this.clearEnabled ? gl.COLOR_BUFFER_BIT | gl.DEPTH_BUFFER_BIT : gl.DEPTH_BUFFER_BIT);
      for (const object of this.renderOrder) object.render();
      if (this.bloomObject && this.bloom.enabled.getBool()) this.bloomObject.render();
      this.presentFrame();
    }

    // CWallpaper::render: the scene texture onto the canvas with the fit mapping and fade.
    presentFrame() {
      const gl = this.gl, p = this.present, uvs = this.viewport.uvs;
      p.texcoords.set([uvs.ustart, uvs.vstart, uvs.uend, uvs.vstart, uvs.ustart, uvs.vend, uvs.ustart, uvs.vend, uvs.uend, uvs.vstart, uvs.uend, uvs.vend]);
      gl.bindFramebuffer(gl.FRAMEBUFFER, null);
      gl.viewport(0, 0, this.canvas.width, this.canvas.height);
      gl.bindVertexArray(p.vao);
      gl.disable(gl.BLEND);
      gl.disable(gl.DEPTH_TEST);
      gl.disable(gl.CULL_FACE);
      gl.useProgram(p.program);
      gl.activeTexture(gl.TEXTURE0);
      gl.bindTexture(gl.TEXTURE_2D, this.fbo.textureId(0));
      gl.enableVertexAttribArray(p.aTexCoord);
      gl.bindBuffer(gl.ARRAY_BUFFER, p.texcoord);
      gl.bufferData(gl.ARRAY_BUFFER, p.texcoords, gl.DYNAMIC_DRAW);
      gl.vertexAttribPointer(p.aTexCoord, 2, gl.FLOAT, false, 0, 0);
      gl.enableVertexAttribArray(p.aPosition);
      gl.bindBuffer(gl.ARRAY_BUFFER, p.position);
      gl.vertexAttribPointer(p.aPosition, 3, gl.FLOAT, false, 0, 0);
      gl.uniform1i(p.uTexture, 0);
      gl.uniform1f(p.uFade, this.fade.factor(this.time));
      gl.drawArrays(gl.TRIANGLES, 0, 6);
      gl.disableVertexAttribArray(p.aTexCoord);
      gl.disableVertexAttribArray(p.aPosition);
      gl.bindVertexArray(null);
    }
  }

  // ---- page entry -----------------------------------------------------------------------------
  async function start() {
    const params = new URLSearchParams(G.location.search);
    const sceneRel = params.get('scene');
    if (!sceneRel) throw new Error('the page URL has no ?scene= parameter naming scene.json');
    const canvas = document.getElementById('scene');
    if (!canvas) throw new Error('the page has no <canvas id="scene">');
    const loader = new G.WELoader.Loader(G.WELoader.pageBase());
    const scene = new Scene(canvas, loader);
    G.WEScene.current = scene;
    scene.installListeners();
    await scene.load(sceneRel);
    scene.schedule();
    return scene;
  }

  const api = {
    Scene, start, parseSceneData, autoProjectionSize, showError, LIGHT_SLOTS, BLOOM_ID,
    registerObject: (key, ctor, priority) => G.WEObjects.register(key, ctor, priority),
    onLoad: (fn) => { loadHooks.push(fn); if (api.current && api.current.loaded) Promise.resolve().then(() => fn(api.current)).catch((e) => api.current.fail(e)); },
    onGeneralSettings: (fn) => { generalHooks.push(fn); },
    current: null,
  };
  G.WEScene = api;
  if (typeof module !== 'undefined' && module.exports) module.exports = api;

  if (typeof document !== 'undefined' && document.getElementById('scene')) {
    G.addEventListener('error', (e) => { if (e.error) showError(e.error); });
    G.addEventListener('unhandledrejection', (e) => showError(e.reason));
    start().catch(showError);
  }
})();
