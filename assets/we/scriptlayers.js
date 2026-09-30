// SceneScript object handles: the ILayer surface of every scene object (transform, image,
// text, sound, particle, effect and puppet members), IEffect, IMaterial, ITextureAnimation,
// IVideoTexture, IAnimationLayer, IAnimation and the IScene handle scripts see as thisScene.
// Every member maps onto the live renderer objects; a member a layer kind does not have throws.
(() => {
  'use strict';
  const G = typeof window !== 'undefined' ? window : globalThis;
  const M = G.WEM;
  const S = G.WEScript;
  const { Vec2, Vec3, Mat4, Mat3, toScript, fromScript, DEG, RAD } = S;

  const layerHandles = new WeakMap();
  const handleObjects = new WeakMap();
  const effectHandles = new WeakMap();
  const animationHandles = new WeakMap();

  function vec3(a) { return new Vec3(a[0], a[1], a[2]); }
  function arr3(v) { const a = fromScript(v, 'vec3'); return Array.isArray(a) ? a : (typeof a === 'number' ? [a, a, a] : [0, 0, 0]); }
  function degrees(a) { return new Vec3(a[0] * RAD, a[1] * RAD, a[2] * RAD); }
  function radians(v) { const a = arr3(v); return [a[0] * DEG, a[1] * DEG, a[2] * DEG]; }
  function isDynamic(v) { return v instanceof G.WEProps.Dynamic; }
  function kindOf(object) {
    if (G.WETextLayer && object instanceof G.WETextLayer.Text) return 'text';
    if (object instanceof G.WEImage.Image) return 'image';
    if (G.WEParticle && object instanceof G.WEParticle.Particle) return 'particle';
    if (object instanceof G.WEObjects.Sound) return 'sound';
    if (object instanceof G.WEObjects.Light) return 'light';
    return 'group';
  }

  // Accessor over a Dynamic: scripts read/write in their own types (angles in degrees).
  function dynamicProperty(target, name, dyn, opts) {
    const o = opts || {};
    Object.defineProperty(target, name, {
      enumerable: true, configurable: true,
      get() { const v = toScript(dyn.get(), dyn.kind); return o.degrees ? S.scaleVec(v, RAD) : v; },
      set(value) { dyn.set(fromScript(o.degrees ? S.scaleVec(value, DEG) : value, dyn.kind), 'script'); },
    });
  }

  function plainProperty(target, name, get, set) {
    Object.defineProperty(target, name, { enumerable: true, configurable: true, get, set });
  }

  // Members other layer kinds have: reading or writing them on this one is an error.
  function absent(target, names, what) {
    for (const name of names) {
      if (Object.prototype.hasOwnProperty.call(target, name)) continue;
      const fail = () => { throw new Error(what + ' has no ' + name); };
      Object.defineProperty(target, name, { enumerable: false, configurable: true, get: fail, set: fail });
    }
  }

  const LAYER_MEMBERS = ['origin', 'angles', 'scale', 'parallaxDepth', 'name', 'visible', 'id', 'getTransformMatrix', 'rotateObjectSpace', 'lookAt', 'lookAtYaw', 'setParent', 'getParent', 'getChildren', 'getAttachmentIndex', 'getAttachmentMatrix', 'getAttachmentOrigin', 'getAttachmentAngles', 'getAnimation',
    'alpha', 'color', 'alignment', 'getTextureAnimation', 'getVideoTexture', 'getAnimationLayerCount', 'getAnimationLayer', 'createAnimationLayer', 'playSingleAnimation', 'destroyAnimationLayer', 'getBoneCount', 'getBoneTransform', 'setBoneTransform', 'getLocalBoneTransform', 'setLocalBoneTransform', 'getLocalBoneAngles', 'setLocalBoneAngles', 'getLocalBoneOrigin', 'setLocalBoneOrigin', 'getBoneIndex', 'getBoneParentIndex', 'applyBonePhysicsImpulse', 'resetBonePhysicsSimulation', 'getBlendShapeIndex', 'getBlendShapeWeight', 'setBlendShapeWeight',
    'isPlaying', 'play', 'stop', 'pause', 'volume', 'getEffect', 'getEffectCount', 'transformAttachmentToTexture', 'size', 'perspective', 'solid',
    'text', 'opaquebackground', 'backgroundcolor', 'pointsize', 'font', 'padding', 'horizontalalign', 'verticalalign', 'anchor', 'limitrows', 'maxrows', 'limitwidth', 'maxwidth',
    'emitParticles', 'instance', 'rootmotion', 'fov', 'zoom', 'intensity', 'radius'];

  // ---- IAnimation over a timeline animation bound to a value -----------------------------------
  function animationHandle(ctx, anim) {
    const cached = animationHandles.get(anim);
    if (cached) return cached;
    const state = { pausedAt: 0 };
    const now = () => ctx.scene.time;
    const h = {
      get fps() { return anim.fps; },
      get frameCount() { return anim.length; },
      get duration() { return anim.length / anim.fps; },
      get name() { return anim.name; },
      get rate() { return anim.rate; },
      set rate(v) { anim.rate = Number(v) || 0; },
      play() { if (!anim.playing) { anim.offset -= (now() - state.pausedAt) * anim.rate; anim.playing = true; } },
      pause() { if (anim.playing) { state.pausedAt = now(); anim.playing = false; } },
      stop() { state.pausedAt = now(); anim.playing = false; anim.offset = -now() * anim.rate; },
      isPlaying() { return anim.playing; },
      getFrame() { return anim.frameAt(anim.playing ? now() : state.pausedAt); },
      setFrame(frame) { const t = anim.playing ? now() : state.pausedAt; anim.offset = Number(frame) / anim.fps - t * anim.rate; },
    };
    animationHandles.set(anim, h);
    return h;
  }

  // ---- IMaterial / IEffect --------------------------------------------------------------------
  function materialHandle(ctx, effectEntry, index, layerObject) {
    const pass = effectEntry.effect.passes[index];
    if (!pass) throw new Error('effect ' + effectEntry.name + ' has no material ' + index);
    return {
      getAnimation: (name) => ctx.findAnimation(name, layerObject),
      get name() { return pass.material ? pass.material.filename : 'command ' + pass.command; },
    };
  }

  function effectHandle(ctx, layerObject, entry) {
    const cached = effectHandles.get(entry);
    if (cached) return cached;
    const constants = () => {
      const found = [];
      for (const o of entry.passOverrides) for (const [k, d] of Object.entries(o.constants)) found.push([k, d]);
      for (const p of entry.effect.passes) if (p.material) for (const mp of p.material.passes) for (const [k, d] of Object.entries(mp.constants)) found.push([k, d]);
      return found;
    };
    const h = {
      get name() { return entry.name; },
      set name(v) { entry.name = String(v); },
      get id() { return entry.id; },
      getMaterialCount: () => entry.effect.passes.length,
      getMaterial: (index) => materialHandle(ctx, entry, Math.trunc(Number(index)), layerObject),
      setMaterialProperty: (propertyName, value) => {
        const hits = constants().filter(([k]) => k === propertyName);
        if (!hits.length) throw new Error('effect ' + entry.name + ' has no material property ' + propertyName + ' (it has ' + constants().map(([k]) => k).join(', ') + ')');
        for (const [, dyn] of hits) dyn.set(fromScript(value, dyn.kind), 'script');
      },
      executeMaterialFunction: (propertyName) => { throw new Error('effect ' + entry.name + ': material functions (' + propertyName + ') have no reference behaviour this renderer can follow'); },
      getAnimation: (name) => ctx.findAnimation(name, layerObject),
    };
    dynamicProperty(h, 'visible', entry.visible);
    effectHandles.set(entry, h);
    return h;
  }

  // ---- ITextureAnimation / IVideoTexture -------------------------------------------------------
  function textureAnimationHandle(ctx, image) {
    const tex = image.texture;
    if (!tex || !tex.animated) throw new Error('layer ' + image.name + ' has no animated texture');
    const a = image.textureAnimation;
    const now = () => ctx.scene.time;
    return {
      get frameCount() { return tex.frames.length; },
      get duration() { return tex.animationTime; },
      get rate() { return a.rate; },
      set rate(v) { const clock = image.animationClock(); a.rate = Number(v) || 0; a.offset = clock - (a.playing ? now() : a.pausedAt) * a.rate; },
      play() { if (!a.playing) { a.offset -= (now() - a.pausedAt) * a.rate; a.playing = true; } },
      pause() { if (a.playing) { a.pausedAt = now(); a.playing = false; } },
      stop() { a.pausedAt = now(); a.playing = false; a.offset = -a.pausedAt * a.rate; },
      isPlaying() { return a.playing; },
      getFrame() {
        const total = tex.animationTime;
        if (!(total > 0)) return 0;
        let remaining = ((image.animationClock() % total) + total) % total;
        for (let i = 0; i < tex.frames.length; i++) { remaining -= tex.frames[i].frametime; if (remaining <= 0) return i; }
        return tex.frames.length - 1;
      },
      setFrame(frame) {
        let t = 0;
        for (let i = 0; i < Math.min(tex.frames.length, Math.trunc(Number(frame))); i++) t += tex.frames[i].frametime;
        a.offset = t - (a.playing ? now() : a.pausedAt) * a.rate;
      },
      join() { a.offset = -(a.playing ? now() : a.pausedAt) * a.rate + (ctx.scene.time % Math.max(tex.animationTime, 1e-6)); },
    };
  }

  function videoTextureHandle(image) {
    const current = image.texture && image.texture.current;
    if (!current || !current.isVideo || !current.video) throw new Error('layer ' + image.name + ' has no video texture');
    const video = current.video;
    return {
      get duration() { return video.duration || 0; },
      get rate() { return video.playbackRate; },
      set rate(v) { video.playbackRate = Number(v) || 0; },
      get loop() { return video.loop; },
      set loop(v) { video.loop = !!v; },
      play() { const p = video.play(); if (p && p.catch) p.catch((e) => console.error('video texture of layer ' + image.name + ': ' + e)); },
      pause() { video.pause(); },
      stop() { video.pause(); video.currentTime = 0; },
      isPlaying() { return !video.paused && !video.ended; },
      getCurrentTime() { return video.currentTime; },
      setCurrentTime(t) { video.currentTime = Number(t) || 0; },
      addEndedCallback(cb) { if (typeof cb !== 'function') throw new Error('addEndedCallback needs a function'); video.addEventListener('ended', cb); },
    };
  }

  // ---- IAnimationLayer over a puppet instance layer --------------------------------------------
  function animationLayerHandle(ctx, image, layer) {
    const warp = image.puppet, inst = warp.instance;
    const h = {
      get fps() { return layer.anim.fps; },
      get frameCount() { return layer.anim.length; },
      get duration() { return layer.anim.length / (layer.anim.fps || 30); },
      get name() { return layer.source.name; },
      set name(v) { layer.source.name = String(v); },
      play: () => inst.playLayer(layer),
      pause: () => inst.pauseLayer(layer),
      stop: () => inst.stopLayer(layer),
      isPlaying: () => inst.isLayerPlaying(layer),
      getFrame: () => inst.layerFrame(layer),
      setFrame: (frame) => inst.setLayerFrame(layer, Number(frame) || 0),
      addEndedCallback: (cb) => { if (typeof cb !== 'function') throw new Error('addEndedCallback needs a function'); layer.endCallbacks.push(cb); },
      getAnimation: (name) => ctx.findAnimation(name, image),
    };
    for (const key of ['rate', 'blend', 'visible']) {
      const source = layer.source;
      if (isDynamic(source[key])) dynamicProperty(h, key, source[key]);
      else plainProperty(h, key, () => source[key], (v) => { source[key] = key === 'visible' ? !!v : Number(v); });
    }
    return h;
  }

  // ---- ILayer --------------------------------------------------------------------------------
  function layerHandle(ctx, object) {
    const cached = layerHandles.get(object);
    if (cached) return cached;
    const scene = ctx.scene;
    const kind = kindOf(object);
    const what = kind + ' layer ' + (object.name || object.id);
    const h = {};
    plainProperty(h, 'id', () => object.id, () => { throw new Error(what + ': the id cannot change'); });
    plainProperty(h, 'name', () => object.name, (v) => { object.name = String(v); });
    dynamicProperty(h, 'origin', object.origin);
    dynamicProperty(h, 'angles', object.angles, { degrees: true });
    dynamicProperty(h, 'scale', object.scale);
    dynamicProperty(h, 'visible', object.visible);
    if (isDynamic(object.parallaxDepth)) dynamicProperty(h, 'parallaxDepth', object.parallaxDepth);
    h.getTransformMatrix = () => {
      const t = object.resolveTransform();
      let m = M.translation(t.origin[0], t.origin[1], t.origin[2]);
      m = M.rotate(m, t.angles[2], 0, 0, 1);
      m = M.rotate(m, t.angles[1], 0, 1, 0);
      m = M.rotate(m, t.angles[0], 1, 0, 0);
      m = M.scale(m, t.scale[0], t.scale[1], t.scale[2]);
      return new Mat4(m);
    };
    h.rotateObjectSpace = (angles) => {
      const delta = radians(angles), current = object.angles.getVec(3);
      object.angles.set([current[0] + delta[0], current[1] + delta[1], current[2] + delta[2]], 'script');
    };
    const lookAt = (center, yawOnly) => {
      const c = arr3(center), o = object.resolveTransform().origin;
      const dx = c[0] - o[0], dy = c[1] - o[1], dz = c[2] - o[2];
      const yaw = Math.atan2(dy, dx);
      const current = object.angles.getVec(3);
      const pitch = yawOnly ? current[0] : -Math.atan2(dz, Math.hypot(dx, dy));
      object.angles.set([pitch, current[1], yaw], 'script');
    };
    h.lookAt = (center) => lookAt(center, false);
    h.lookAtYaw = (center) => lookAt(center, true);
    h.setParent = (parent, a, b) => {
      const attachment = (typeof a === 'string' || (typeof a === 'number' && b !== undefined)) ? a : null;
      const adjust = attachment === null ? !!a : !!b;
      const before = adjust ? object.resolveTransform() : null;
      const parentObject = parent === null || parent === undefined ? null : ctx.resolveLayer(parent);
      if (parentObject === object) throw new Error(what + ' cannot be its own parent');
      object.parent = parentObject ? parentObject.id : null;
      object.parentAttachment = attachment;
      if (attachment !== null && parentObject && typeof parentObject.attachmentTransform !== 'function') throw new Error('layer ' + parentObject.name + ' has no puppet attachments to hang ' + what + ' on');
      if (adjust && before) {
        // Keep the world transform: solve the local one against the new parent.
        object.origin.set([0, 0, 0], 'script'); object.angles.set([0, 0, 0], 'script'); object.scale.set([1, 1, 1], 'script');
        const base = object.resolveTransform();
        const angle = before.angle - base.angle;
        const dx = before.origin[0] - base.origin[0], dy = before.origin[1] - base.origin[1];
        const c = Math.cos(-base.angle), s = Math.sin(-base.angle);
        const local = [(dx * c - dy * s) / (base.scale[0] || 1), (dx * s + dy * c) / (base.scale[1] || 1), (before.origin[2] - base.origin[2]) / (base.scale[2] || 1)];
        object.origin.set(local, 'script');
        object.angles.set([before.angles[0] - base.angles[0], before.angles[1] - base.angles[1], angle], 'script');
        object.scale.set([before.scale[0] / (base.scale[0] || 1), before.scale[1] / (base.scale[1] || 1), before.scale[2] / (base.scale[2] || 1)], 'script');
      }
    };
    h.getParent = () => (object.parent === null ? null : layerHandle(ctx, ctx.requireObject(object.parent)));
    h.getChildren = () => Array.from(scene.objects.values()).filter((o) => o.parent === object.id).map((o) => layerHandle(ctx, o));
    h.getAnimation = (name) => ctx.findAnimation(name, object);

    if (kind === 'image' || kind === 'text') installImage(ctx, h, object, what);
    if (kind === 'text') installText(h, object);
    if (kind === 'sound') installSound(h, object);
    if (kind === 'particle') installParticle(h, object);
    if (kind === 'light') {
      dynamicProperty(h, 'color', object.color);
      dynamicProperty(h, 'intensity', object.intensity);
      dynamicProperty(h, 'radius', object.radius);
    }
    absent(h, LAYER_MEMBERS, what);
    layerHandles.set(object, h);
    handleObjects.set(h, object);
    return h;
  }

  function installImage(ctx, h, image, what) {
    dynamicProperty(h, 'alpha', image.alpha);
    dynamicProperty(h, 'color', image.color);
    plainProperty(h, 'alignment', () => image.alignment, (v) => { image.alignment = String(v); });
    plainProperty(h, 'size', () => new Vec2(image.size[0], image.size[1]), () => { throw new Error(what + ': size is read-only'); });
    plainProperty(h, 'perspective', () => image.perspective, (v) => { image.perspective = !!v; });
    plainProperty(h, 'solid', () => !!(image.model && image.model.solidlayer), () => { throw new Error(what + ': solid is decided by the model file'); });
    h.getEffectCount = () => image.effects.length;
    h.getEffect = (ref) => {
      const entry = typeof ref === 'number' ? image.effects[ref] : image.effects.find((e) => e.name === ref);
      if (!entry) throw new Error(what + ' has no effect ' + JSON.stringify(ref));
      return effectHandle(ctx, image, entry);
    };
    h.transformAttachmentToTexture = (attachmentLayer, attachmentName) => {
      const other = ctx.resolveLayer(attachmentLayer);
      if (typeof other.attachmentTransform !== 'function') throw new Error('layer ' + other.name + ' has no puppet attachments');
      const att = other.attachmentTransform(attachmentName);
      const ot = other.resolveTransform(), it = image.resolveTransform();
      const c = Math.cos(ot.angle), s = Math.sin(ot.angle);
      const ax = att.origin[0] * ot.scale[0], ay = -att.origin[1] * ot.scale[1];
      const world = [ot.origin[0] + ax * c - ay * s, ot.origin[1] + ax * s + ay * c];
      const dx = world[0] - it.origin[0], dy = world[1] - it.origin[1];
      const ic = Math.cos(-it.angle), is = Math.sin(-it.angle);
      const lx = (dx * ic - dy * is) / (it.scale[0] || 1), ly = (dx * is + dy * ic) / (it.scale[1] || 1);
      const u = lx / (image.size[0] || 1) + 0.5, v = 0.5 - ly / (image.size[1] || 1);
      return Mat3.compose(new Vec2(u, v), (ot.angle - att.angle - it.angle) * RAD, new Vec2(1, 1));
    };
    h.getTextureAnimation = () => textureAnimationHandle(ctx, image);
    h.getVideoTexture = () => videoTextureHandle(image);
    const puppet = () => {
      if (!image.puppet) throw new Error(what + ' has no puppet warp');
      return image.puppet;
    };
    const bone = (ref) => puppet().boneIndex(ref);
    h.getAnimationLayerCount = () => puppet().instance.layers.length;
    h.getAnimationLayer = (ref) => animationLayerHandle(ctx, image, puppet().findLayer(ref));
    h.createAnimationLayer = (animation, config) => animationLayerHandle(ctx, image, puppet().createLayer(animation, config, false));
    h.playSingleAnimation = (animation, config) => animationLayerHandle(ctx, image, puppet().createLayer(animation, config, true));
    h.destroyAnimationLayer = (ref) => {
      const p = puppet();
      const layer = ref && typeof ref === 'object' && typeof ref.getFrame === 'function' ? p.instance.layers.find((l) => animationLayerHandle(ctx, image, l).name === ref.name) : p.findLayer(ref);
      if (!layer) return false;
      p.instance.removeLayer(layer);
      return true;
    };
    h.getBoneCount = () => puppet().puppet.bones.length;
    h.getBoneIndex = (name) => puppet().boneIndex(name);
    h.getBoneParentIndex = (ref) => { const p = puppet().puppet.bones[bone(ref)].parent; return p === G.WEPuppet.NO_PARENT ? -1 : p; };
    h.getBoneTransform = (ref) => new Mat4(puppet().instance.boneWorld(bone(ref)));
    h.setBoneTransform = (ref, transform) => puppet().instance.setBoneTransform(bone(ref), transform.m);
    h.getLocalBoneTransform = (ref) => new Mat4(puppet().instance.localBoneTransform(bone(ref)));
    h.setLocalBoneTransform = (ref, transform) => puppet().instance.setLocalBoneTransform(bone(ref), transform.m);
    h.getLocalBoneAngles = (ref) => degrees(puppet().instance.localBoneAngles(bone(ref)));
    h.setLocalBoneAngles = (ref, angles) => puppet().instance.setLocalBoneAngles(bone(ref), radians(angles));
    h.getLocalBoneOrigin = (ref) => vec3(puppet().instance.localBoneOrigin(bone(ref)));
    h.setLocalBoneOrigin = (ref, origin) => puppet().instance.setLocalBoneOrigin(bone(ref), arr3(origin));
    h.applyBonePhysicsImpulse = (ref, directional, angular) => {
      const p = puppet();
      const targets = ref === undefined || ref === null ? p.puppet.bones.map((b, i) => i).filter((i) => p.instance.physics[i]) : [bone(ref)];
      for (const i of targets) p.instance.applyPhysicsImpulse(i, arr3(directional || new Vec3()), arr3(angular || new Vec3()));
    };
    h.resetBonePhysicsSimulation = (ref) => puppet().instance.resetPhysics(ref === undefined || ref === null ? null : bone(ref));
    const noShapes = () => { throw new Error(what + ': blend shapes are not rendered by this renderer'); };
    h.getBlendShapeIndex = noShapes;
    h.getBlendShapeWeight = noShapes;
    h.setBlendShapeWeight = noShapes;
    h.getAttachmentIndex = (name) => puppet().attachmentIndex(name);
    h.getAttachmentMatrix = (ref) => new Mat4(puppet().attachmentTransform(ref).matrix);
    h.getAttachmentOrigin = (ref) => vec3(puppet().attachmentTransform(ref).origin);
    h.getAttachmentAngles = (ref) => new Vec3(0, 0, puppet().attachmentTransform(ref).angle * RAD);
  }

  function installText(h, text) {
    for (const key of ['text', 'opaquebackground', 'backgroundcolor', 'pointsize', 'font', 'padding', 'horizontalalign', 'verticalalign', 'anchor', 'limitrows', 'maxrows', 'limitwidth', 'maxwidth']) dynamicProperty(h, key, text[key]);
  }

  function installSound(h, sound) {
    dynamicProperty(h, 'volume', sound.volume);
    h.isPlaying = () => sound.elements.some((el) => !el.paused && !el.ended);
    h.play = () => { for (const el of sound.elements) sound.play(el); };
    h.pause = () => { for (const el of sound.elements) el.pause(); };
    h.stop = () => { for (const el of sound.elements) { el.pause(); el.currentTime = 0; } };
  }

  function installParticle(h, particle) {
    const system = () => {
      if (!particle.system) throw new Error('particle layer ' + particle.name + ' is still loading');
      return particle.system;
    };
    h.play = () => system().play();
    h.pause = () => system().pause();
    h.stop = () => system().stop();
    h.isPlaying = () => system().isPlaying();
    h.emitParticles = (count) => system().emitParticles(count);
    const instance = {};
    plainProperty(h, 'instance', () => {
      const def = particle.def;
      if (!def || !particle.system) throw new Error('particle layer ' + particle.name + ' is still loading');
      for (const key of ['alpha', 'size', 'count', 'speed', 'lifetime', 'rate', 'colorn', 'color', 'enabled']) if (!Object.prototype.hasOwnProperty.call(instance, key)) dynamicProperty(instance, key, def.instance[key]);
      for (let i = 0; i < 8; i++) {
        const key = 'controlpoint' + i;
        if (Object.prototype.hasOwnProperty.call(instance, key)) continue;
        if (!def.instance[key]) def.instance[key] = G.WEProps.setting(particle.system.controlPointPosition(i), { kind: 'vec3', where: 'object ' + particle.id + ' ' + key, properties: particle.scene.properties });
        dynamicProperty(instance, key, def.instance[key]);
      }
      return instance;
    }, () => { throw new Error('particle instance overrides are set through their members'); });
  }

  // ---- IScene --------------------------------------------------------------------------------
  function sceneHandle(ctx) {
    const scene = ctx.scene;
    const data = scene.data;
    const h = {};
    const layers = () => scene.renderOrder.filter((o) => o.id !== G.WEScene.BLOOM_ID);
    h.getLayer = (ref) => layerHandle(ctx, ctx.resolveLayer(ref));
    h.getLayerByID = (id) => {
      const n = typeof id === 'number' ? id : parseInt(String(id), 10);
      if (!Number.isFinite(n)) throw new Error('getLayerByID needs a numeric id, not ' + JSON.stringify(id));
      return layerHandle(ctx, ctx.requireObject(n));
    };
    h.getLayerCount = () => layers().length;
    h.enumerateLayers = () => layers().map((o) => layerHandle(ctx, o));
    h.destroyLayer = (ref) => ctx.destroyLayer(ctx.resolveLayer(ref));
    h.createLayer = (configuration) => ctx.createLayer(configuration);
    h.sortLayer = (ref, index) => {
      const object = ctx.resolveLayer(ref);
      const order = scene.renderOrder;
      const from = order.indexOf(object);
      if (from < 0) return false;
      order.splice(from, 1);
      order.splice(Math.max(0, Math.min(order.length, Math.trunc(Number(index)))), 0, object);
      return true;
    };
    h.getLayerIndex = (ref) => layers().indexOf(ctx.resolveLayer(ref));
    h.getInitialLayerConfig = (ref) => JSON.parse(JSON.stringify(ctx.resolveLayer(ref).json));
    h.getCameraTransforms = () => new S.CameraTransforms(vec3(scene.camera.eye), vec3(scene.camera.center), vec3(scene.camera.up), ctx.cameraZoom);
    h.setCameraTransforms = (t) => {
      if (!t || typeof t !== 'object') throw new Error('setCameraTransforms needs a CameraTransforms');
      if (t.eye !== undefined) scene.camera.eye = arr3(t.eye);
      if (t.center !== undefined) scene.camera.center = arr3(t.center);
      if (t.up !== undefined) scene.camera.up = arr3(t.up);
      if (t.zoom !== undefined) { const z = Number(t.zoom); if (!(z > 0)) throw new Error('camera zoom must be positive, not ' + t.zoom); ctx.cameraZoom = z; }
      scene.camera.lookAt = M.lookAt(scene.camera.eye, scene.camera.center, scene.camera.up);
      scene.camera.setOrthogonalProjection(scene.width / ctx.cameraZoom, scene.height / ctx.cameraZoom);
    };
    h.getAnimation = (name) => ctx.findAnimation(name, null);
    h.createModelData = () => { throw new Error('createModelData: procedural model layers need Wallpaper Engine\'s model-data material, which this renderer does not have'); };
    h.destroyModelData = () => { throw new Error('destroyModelData: procedural model layers need Wallpaper Engine\'s model-data material, which this renderer does not have'); };
    dynamicProperty(h, 'bloom', data.camera.bloom.enabled);
    dynamicProperty(h, 'bloomstrength', data.camera.bloom.strength);
    dynamicProperty(h, 'bloomthreshold', data.camera.bloom.threshold);
    plainProperty(h, 'clearenabled', () => scene.clearEnabled, (v) => { scene.clearEnabled = !!v; });
    dynamicProperty(h, 'clearcolor', data.colors.clear);
    dynamicProperty(h, 'ambientcolor', data.colors.ambient);
    dynamicProperty(h, 'skylightcolor', data.colors.skylight);
    dynamicProperty(h, 'fov', data.camera.configuration.fov);
    dynamicProperty(h, 'nearz', data.camera.configuration.nearz);
    dynamicProperty(h, 'farz', data.camera.configuration.farz);
    dynamicProperty(h, 'camerafade', data.camera.fade);
    dynamicProperty(h, 'camerashake', data.camera.shake.enabled);
    dynamicProperty(h, 'camerashakespeed', data.camera.shake.speed);
    dynamicProperty(h, 'camerashakeamplitude', data.camera.shake.amplitude);
    dynamicProperty(h, 'camerashakeroughness', data.camera.shake.roughness);
    dynamicProperty(h, 'cameraparallax', data.camera.parallax.enabled);
    dynamicProperty(h, 'cameraparallaxamount', data.camera.parallax.amount);
    dynamicProperty(h, 'cameraparallaxdelay', data.camera.parallax.delay);
    dynamicProperty(h, 'cameraparallaxmouseinfluence', data.camera.parallax.mouseInfluence);
    return h;
  }

  // The scene object behind a layer handle (or a thisObject derived from one).
  function objectOf(handle) {
    for (let h = handle; h && typeof h === 'object'; h = Object.getPrototypeOf(h)) { const o = handleObjects.get(h); if (o) return o; }
    return null;
  }

  const api = { objectOf, layerHandle, effectHandle, animationHandle, animationLayerHandle, textureAnimationHandle, videoTextureHandle, sceneHandle, kindOf, dynamicProperty, LAYER_MEMBERS };
  G.WEScriptLayers = api;
  if (typeof module !== 'undefined' && module.exports) module.exports = api;
})();
