// SceneScript in the scene: every bound value with a "script" source becomes a module with
// engine / input / thisScene / thisLayer / thisObject / shared / console / localStorage, its
// init and update hooks run each frame with the bound value, and the events reach every module:
// applyUserProperties, applyGeneralSettings, resizeScreen, the cursor events, the media
// integration events from the host bridge and puppet animation events. Timeline animations on
// bound values are driven here too, and scripts can create, destroy and reorder layers.
(() => {
  'use strict';
  const G = typeof window !== 'undefined' ? window : globalThis;
  const S = G.WEScript;
  const L = G.WEScriptLayers;
  const { Vec2, Vec3 } = S;

  const MEDIA = [
    ['status', 'wallpaperRegisterMediaStatusListener', 'mediaStatusChanged', (d) => new S.MediaStatusEvent(d && d.enabled)],
    ['properties', 'wallpaperRegisterMediaPropertiesListener', 'mediaPropertiesChanged', (d) => new S.MediaPropertiesEvent(d)],
    ['thumbnail', 'wallpaperRegisterMediaThumbnailListener', 'mediaThumbnailChanged', (d) => new S.MediaThumbnailEvent(d)],
    ['playback', 'wallpaperRegisterMediaPlaybackListener', 'mediaPlaybackChanged', (d) => new S.MediaPlaybackEvent(d && typeof d.state === 'number' ? d.state : Number(d))],
    ['timeline', 'wallpaperRegisterMediaTimelineListener', 'mediaTimelineChanged', (d) => new S.MediaTimelineEvent(d && d.position, d && d.duration)],
  ];
  // Object members that hold GL state or back-references rather than bound values.
  const SKIP_KEYS = new Set(['scene', 'json', 'gl', 'loader', 'effects', 'puppet', 'canvasTexture', 'texture', 'system', 'pass', 'materialPasses', 'effectPasses', 'blendPass', 'activePasses', 'buffers', 'mainFBO', 'subFBO', 'currentMainFBO', 'currentSubFBO', 'fboProvider', 'refractFBO', 'elements']);
  const VEC_KEYS = ['origin', 'angles', 'scale', 'color', 'size', 'parallaxDepth', 'backgroundcolor'];

  function isPlain(v) {
    const p = Object.getPrototypeOf(v);
    return p === Object.prototype || p === null || Array.isArray(v) || v instanceof Map;
  }

  // Every Dynamic reachable from `root` through plain objects, arrays and maps.
  function collectDynamics(root, skip) {
    const out = [], seen = new Set(), stack = [root];
    while (stack.length) {
      const v = stack.pop();
      if (!v || typeof v !== 'object' || seen.has(v)) continue;
      seen.add(v);
      if (v instanceof G.WEProps.Dynamic) {
        out.push(v);
        for (const sp of Object.values(v.scriptProperties || {})) stack.push(sp);
        continue;
      }
      if (ArrayBuffer.isView(v) || v instanceof ArrayBuffer) continue;
      if (v !== root && !isPlain(v)) continue;
      if (v instanceof Map) { for (const e of v.values()) stack.push(e); continue; }
      for (const [k, e] of Object.entries(v)) if (!skip.has(k)) stack.push(e);
    }
    return out;
  }

  // Script values (Vec2/Vec3 objects, degrees) in a layer configuration -> scene.json values.
  function configurationToJson(configuration) {
    const json = {};
    for (const [k, v] of Object.entries(configuration)) {
      if (v && typeof v === 'object' && typeof v.x === 'number' && typeof v.y === 'number') {
        const a = typeof v.z === 'number' ? [v.x, v.y, v.z] : [v.x, v.y];
        json[k] = (k === 'angles' ? a.map((n) => n * S.DEG) : a).join(' ');
      } else if (VEC_KEYS.includes(k) && Array.isArray(v)) {
        json[k] = (k === 'angles' ? v.map((n) => n * S.DEG) : v).join(' ');
      } else json[k] = v;
    }
    return json;
  }

  class Scripting {
    constructor(scene) {
      this.scene = scene;
      this.cameraZoom = 1;
      this.animations = [];
      this.cursorIsDown = false;
      this.inside = new Map();
      this.mediaRegistered = new Set();
      this.unsubscribe = null;
      this.engine = new S.Engine(this.host());
      this.sceneObject = L.sceneHandle(this);
      this.engine.scene = this.sceneObject;
      this.engine.onFirstRun = (m) => this.firstRun(m);
    }

    host() {
      const scene = this.scene;
      return {
        log: (text) => console.log(text),
        error: (text) => console.error(text),
        storageKey: (typeof G.location === 'object' && G.location ? new URLSearchParams(G.location.search).get('scene') : null) || 'scene',
        screenSize: () => [G.screen ? G.screen.width : scene.canvas.width, G.screen ? G.screen.height : scene.canvas.height],
        canvasSize: () => [scene.canvas.width, scene.canvas.height],
        userProperties: () => this.userProperties(Object.keys(scene.properties.values)),
        cursorWorld: () => this.cursorWorld(),
        cursorScreen: () => { const p = scene.viewport.pointer; return [p[0] * G.innerWidth, p[1] * G.innerHeight]; },
        cursorDown: () => this.cursorIsDown,
        wantsAudio: () => scene.requireAudio(),
        registerAsset: (handle) => { if (handle.precache) scene.loader.bytes(handle.path).catch((e) => scene.fail(e)); },
        openUserShortcut: (name) => { throw new Error('openUserShortcut(' + name + '): this host cannot open user shortcuts'); },
        sceneObject: null,
      };
    }

    // ---- layers --------------------------------------------------------------------------------
    layers() { return this.scene.renderOrder.filter((o) => o.id !== G.WEScene.BLOOM_ID); }

    requireObject(id) {
      const o = this.scene.objects.get(id);
      if (!o) throw new Error('no layer has the id ' + id);
      return o;
    }

    // A layer by handle, index in the layer list, or name.
    resolveLayer(ref) {
      if (ref && typeof ref === 'object') {
        const object = L.objectOf(ref);
        if (!object) throw new Error('not a layer handle: ' + JSON.stringify(ref).slice(0, 80));
        return object;
      }
      const layers = this.layers();
      if (typeof ref === 'number') {
        if (Number.isInteger(ref) && ref >= 0 && ref < layers.length) return layers[ref];
        throw new Error('no layer at index ' + ref + ' (' + layers.length + ' layers)');
      }
      if (typeof ref === 'string') {
        const byName = layers.find((o) => o.name === ref) || Array.from(this.scene.objects.values()).find((o) => o.name === ref);
        if (byName) return byName;
        throw new Error('no layer named ' + JSON.stringify(ref));
      }
      throw new Error('a layer is named by its handle, index or name, not ' + JSON.stringify(ref));
    }

    findAnimation(name, layerObject) {
      const own = this.animations.filter((a) => layerObject === null || a.owner === layerObject);
      if (name === undefined || name === null) {
        if (own.length === 1) return L.animationHandle(this, own[0].animation);
        throw new Error('getAnimation() without a name needs exactly one animation on ' + (layerObject ? 'layer ' + layerObject.name : 'the scene') + ', which has ' + own.length);
      }
      const hit = own.find((a) => a.animation.name === name) || this.animations.find((a) => a.animation.name === name);
      if (!hit) throw new Error('no animation is named ' + JSON.stringify(name));
      return L.animationHandle(this, hit.animation);
    }

    attachDynamic(dyn, layerObject, layerHandle, ownerHandle) {
      if (dyn.animation) this.animations.push({ dyn, animation: dyn.animation, owner: layerObject });
      if (!dyn.script) return null;
      const thisObject = Object.create(ownerHandle, {
        getAnimation: { value: (name) => ((name === undefined || name === null) && dyn.animation ? L.animationHandle(this, dyn.animation) : this.findAnimation(name, layerObject)) },
      });
      const degrees = /\.angles$/.test(dyn.where || '');
      return this.engine.attach(dyn, { layer: layerHandle, object: thisObject, key: dyn.where || 'value', degrees, layerObject });
    }

    attachObjectScripts(object) {
      const layer = L.layerHandle(this, object);
      for (const dyn of collectDynamics(object, SKIP_KEYS)) this.attachDynamic(dyn, object, layer, layer);
      for (const entry of (object.effects || [])) {
        const effect = L.effectHandle(this, object, entry);
        for (const dyn of collectDynamics(entry, new Set())) this.attachDynamic(dyn, object, layer, effect);
      }
    }

    attachSceneScripts() {
      for (const key of ['colors', 'camera']) for (const dyn of collectDynamics(this.scene.data[key], new Set())) this.attachDynamic(dyn, null, null, this.sceneObject);
    }

    createLayer(configuration) {
      const scene = this.scene;
      let json;
      if (typeof configuration === 'string') json = { image: configuration };
      else if (configuration && typeof configuration === 'object' && typeof configuration.path === 'string' && 'precache' in configuration) json = { image: configuration.path };
      else if (configuration && typeof configuration === 'object') json = configurationToJson(configuration);
      else throw new Error('createLayer needs a layer configuration, an asset path or an asset handle');
      let next = 1;
      for (const id of scene.objects.keys()) next = Math.max(next, id + 1);
      for (const o of scene.data.objects) next = Math.max(next, o.id + 1);
      if (typeof json.id !== 'number' || scene.objects.has(json.id)) json.id = next;
      if (typeof json.name !== 'string') json.name = 'layer ' + json.id;
      scene.data.objects.push(json);
      const object = G.WEObjects.create(scene, json);
      scene.objects.set(object.id, object);
      const handle = L.layerHandle(this, object);
      object.setup().then(() => {
        scene.addObjectToRenderOrder(json);
        this.attachObjectScripts(object);
        this.ensureMedia();
      }).catch((e) => scene.fail(e));
      return handle;
    }

    destroyLayer(object) {
      const scene = this.scene;
      if (object.id === G.WEScene.BLOOM_ID) throw new Error('the bloom layer cannot be destroyed');
      object.dispose();
      scene.objects.delete(object.id);
      scene.renderOrder = scene.renderOrder.filter((o) => o !== object);
      scene.data.objects = scene.data.objects.filter((o) => o.id !== object.id);
      this.engine.detach((m) => m.context.layerObject === object);
      this.animations = this.animations.filter((a) => a.owner !== object);
      this.inside.delete(object);
      for (const child of scene.objects.values()) if (child.parent === object.id) { child.parent = null; child.parentAttachment = null; }
      return true;
    }

    // ---- values --------------------------------------------------------------------------------
    convertProperty(name, value) {
      const decl = this.scene.projectProperties[name];
      const type = decl && typeof decl.type === 'string' ? decl.type : null;
      if (type === 'color') { const c = G.WEProps.toVec(value, 3, true); return new Vec3(c[0], c[1], c[2]); }
      if (type === 'bool') return G.WEProps.coerce(value, 'bool');
      if (type === 'slider') return G.WEProps.toNumber(value);
      return value;
    }

    userProperties(names) {
      const out = {};
      for (const name of names) out[name] = this.convertProperty(name, this.scene.properties.values[name]);
      return out;
    }

    firstRun(module) {
      module.call('applyUserProperties', this.userProperties(Object.keys(this.scene.properties.values)));
      module.call('applyGeneralSettings', Object.assign({}, this.scene.generalSettings));
    }

    // ---- cursor --------------------------------------------------------------------------------
    // Scene coordinates of the pointer (pixels from the top left, as layer origins are given).
    cursorWorld() {
      const v = this.scene.viewport, uvs = v.uvs;
      const mx = Math.min(1, Math.max(0, v.pointer[0])), my = Math.min(1, Math.max(0, v.pointer[1]));
      const u = uvs.ustart + mx * (uvs.uend - uvs.ustart);
      const top = uvs.vstart + (1 - my) * (uvs.vend - uvs.vstart);
      return [u * this.scene.width, top * this.scene.height, 0];
    }

    insideLayer(object, world) {
      if (!(object instanceof G.WEImage.Image) || !object.initialized) return false;
      const pos = object.pos;
      const cx = world[0] - this.scene.width / 2, cy = this.scene.height / 2 - world[1];
      return cx >= Math.min(pos[0], pos[2]) && cx <= Math.max(pos[0], pos[2]) && cy >= Math.min(pos[1], pos[3]) && cy <= Math.max(pos[1], pos[3]);
    }

    cursorEvent(module, world) {
      const wv = new Vec3(world[0], world[1], world[2]);
      const lo = module.context.layerObject;
      const local = lo ? lo.resolveTransform().origin : [0, 0, 0];
      return new S.CursorEvent(wv, new Vec3(world[0] - local[0], world[1] - local[1], world[2] - local[2]));
    }

    cursor(hook) {
      const world = this.cursorWorld();
      if (hook === 'cursorMove') {
        const seen = new Set();
        for (const m of this.engine.modules.slice()) {
          const lo = m.context.layerObject;
          if (!lo || seen.has(lo)) continue;
          seen.add(lo);
          const inside = this.insideLayer(lo, world);
          const was = this.inside.get(lo) || false;
          if (inside === was) continue;
          this.inside.set(lo, inside);
          for (const mm of this.engine.modules) if (mm.context.layerObject === lo) mm.call(inside ? 'cursorEnter' : 'cursorLeave', this.cursorEvent(mm, world));
        }
      }
      for (const m of this.engine.modules.slice()) m.call(hook, this.cursorEvent(m, world));
    }

    // ---- events --------------------------------------------------------------------------------
    installEvents() {
      const scene = this.scene;
      scene.properties.listen((names) => { if (names.length) this.engine.notify('applyUserProperties', this.userProperties(names)); });
      G.WEScene.onGeneralSettings((general) => this.engine.notify('applyGeneralSettings', Object.assign({}, general)));
      G.addEventListener('resize', () => this.engine.notify('resizeScreen', new Vec2(scene.canvas.width, scene.canvas.height)));
      G.addEventListener('mousemove', () => this.cursor('cursorMove'));
      G.addEventListener('mousedown', () => { this.cursorIsDown = true; this.cursor('cursorDown'); });
      G.addEventListener('mouseup', () => { this.cursorIsDown = false; this.cursor('cursorUp'); });
      G.addEventListener('click', () => this.cursor('cursorClick'));
      G.addEventListener('beforeunload', () => this.engine.destroy());
      for (const object of scene.objects.values()) this.watchPuppet(object);
    }

    // Puppet animation events reach the scripts of that layer as AnimationEvent.
    watchPuppet(object) {
      if (!object.puppet || object.puppetWatched) return;
      object.puppetWatched = true;
      object.puppet.onAnimationEvent((layer, event) => {
        this.engine.notify('animationEvent', new S.AnimationEvent(event.name, event.frame), (m) => m.context.layerObject === object);
      });
    }

    // The host's media listeners, registered once a script handles the event.
    ensureMedia() {
      for (const [kind, register, hook, convert] of MEDIA) {
        if (this.mediaRegistered.has(kind) || !this.engine.anyDefines(hook)) continue;
        const fn = G[register];
        if (typeof fn !== 'function') throw new Error('the host page offers no ' + register + ', which a SceneScript ' + hook + ' handler needs');
        this.mediaRegistered.add(kind);
        fn((data) => { try { this.engine.notify(hook, convert(data)); } catch (e) { this.scene.fail(e); } });
      }
    }

    // ---- frame ---------------------------------------------------------------------------------
    tick(dt, time) {
      for (const a of this.animations) a.animation.apply(a.dyn, time);
      if (this.engine.audioBuffers.length) this.engine.feedAudio(this.scene.audio.left64, this.scene.audio.right64);
      this.engine.tick(time, dt);
      for (const object of this.scene.objects.values()) if (object.puppet && !object.puppetWatched) this.watchPuppet(object);
    }

    start() {
      for (const object of this.scene.objects.values()) if (object.id !== G.WEScene.BLOOM_ID) this.attachObjectScripts(object);
      this.attachSceneScripts();
      this.installEvents();
      this.ensureMedia();
      this.unsubscribe = this.scene.onUpdate((dt, time) => this.tick(dt, time));
    }

    dispose() {
      if (this.unsubscribe) this.unsubscribe();
      this.engine.destroy();
    }
  }

  G.WEScene.onLoad(async (scene) => {
    const scripting = new Scripting(scene);
    scene.scripting = scripting;
    scripting.start();
  });

  const api = { Scripting, collectDynamics, configurationToJson, MEDIA, SKIP_KEYS };
  G.WEScripting = api;
  if (typeof module !== 'undefined' && module.exports) module.exports = api;
})();
