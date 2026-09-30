// Scene objects: the base object every layer type shares (id, parent chain, origin/scale/angles/
// visible bindings), sound layers, light layers and the type registry other modules extend.
// Ports Data/Parsers/ObjectParser.cpp (base and sound data), Render/CObject.cpp,
// CImage::localTransform/resolveTransform and Render/Objects/CSound.cpp.
(() => {
  'use strict';
  const G = typeof window !== 'undefined' ? window : globalThis;

  const MAX_PARENT_DEPTH = 32;

  // ObjectParser::parseDependencies
  function parseDependencies(json) {
    if (!Array.isArray(json.dependencies)) return [];
    return json.dependencies.map((d) => {
      if (typeof d !== 'number') throw new Error('object ' + json.id + ': dependency ' + JSON.stringify(d) + ' is not an object id');
      return d;
    });
  }

  function rotateVec2(x, y, angle) {
    const c = Math.cos(angle), s = Math.sin(angle);
    return [x * c - y * s, x * s + y * c];
  }

  /** ObjectData + CObject: what every scene.json object carries. */
  class SceneObject {
    constructor(scene, json) {
      if (!json || typeof json !== 'object') throw new Error('scene object is not a JSON object');
      if (typeof json.id !== 'number') throw new Error('object must have an id (' + JSON.stringify(json).slice(0, 80) + ')');
      this.scene = scene;
      this.json = json;
      this.id = json.id;
      this.name = typeof json.name === 'string' ? json.name : (typeof json.name === 'number' ? String(json.name) : '');
      this.dependencies = parseDependencies(json);
      this.parent = typeof json.parent === 'number' ? json.parent : null;
      // Name or index of the parent's puppet attachment this object hangs on (SceneScript setParent).
      this.parentAttachment = null;
      this.origin = this.setting('origin', 'vec3', [0, 0, 0]);
      this.scale = this.setting('scale', 'vec3', [1, 1, 1]);
      this.angles = this.setting('angles', 'vec3', [0, 0, 0]);
      this.visible = this.setting('visible', 'bool', true);
      this.initialized = false;
    }

    // A scene.json value of this object as a Dynamic (literal, {"user":...}, script, animation).
    setting(key, kind, dflt, expectColor) {
      return G.WEProps.setting(this.json[key], {
        kind, default: dflt, expectColor: !!expectColor,
        where: 'object ' + this.id + '.' + key,
        properties: this.scene.properties,
      });
    }

    async setup() { this.initialized = true; }
    update() {}
    render() {}
    setPaused() {}
    dispose() {}

    // CImage::localTransform
    localTransform() {
      const angles = this.angles.getVec(3);
      return { origin: this.origin.getVec(3), scale: this.scale.getVec(3), angles, angle: angles[2] };
    }

    // CImage::resolveTransform: fold this object's transform onto its parents', root first.
    resolveTransform() {
      const chain = [this];
      let current = this;
      while (current.parent !== null) {
        if (chain.length > MAX_PARENT_DEPTH) throw new Error('parent chain of object ' + current.id + ' is deeper than ' + MAX_PARENT_DEPTH + ' levels; it loops');
        const parentObject = this.scene.getObject(current.parent);
        if (!parentObject) break;
        current = parentObject;
        chain.push(current);
      }
      let resolved = chain[chain.length - 1].localTransform();
      for (let i = chain.length - 2; i >= 0; i--) {
        const local = chain[i].localTransform();
        if (chain[i].parentAttachment !== null) {
          const parentObject = chain[i + 1];
          if (typeof parentObject.attachmentTransform !== 'function') throw new Error('object ' + chain[i].id + ' hangs on attachment ' + JSON.stringify(chain[i].parentAttachment) + ' of object ' + parentObject.id + ', which has no puppet attachments');
          // Attachment offsets are in the parent's layer space (y up); scene space runs y down.
          const att = parentObject.attachmentTransform(chain[i].parentAttachment);
          local.origin = [local.origin[0] + att.origin[0], local.origin[1] - att.origin[1], local.origin[2] + att.origin[2]];
          local.angles = [local.angles[0], local.angles[1], local.angles[2] - att.angle];
          local.angle = local.angles[2];
        }
        const offset = rotateVec2(local.origin[0] * resolved.scale[0], local.origin[1] * resolved.scale[1], resolved.angle);
        const origin = [resolved.origin[0] + offset[0], resolved.origin[1] + offset[1], resolved.origin[2] + local.origin[2] * resolved.scale[2]];
        const angles = [local.angles[0] + resolved.angles[0], local.angles[1] + resolved.angles[1], local.angles[2] + resolved.angles[2]];
        resolved = {
          origin,
          scale: [local.scale[0] * resolved.scale[0], local.scale[1] * resolved.scale[1], local.scale[2] * resolved.scale[2]],
          angles,
          angle: angles[2],
        };
      }
      return resolved;
    }
  }

  /**
   * Sound layer (SoundData + CSound): "sound" lists the files, "playbackmode" is "loop",
   * "random" or absent (play once), "volume" scales the host's volume, "mintime"/"maxtime"
   * bound the pause between random plays in seconds.
   */
  class Sound extends SceneObject {
    constructor(scene, json) {
      super(scene, json);
      if (!Array.isArray(json.sound) || !json.sound.length) throw new Error('object ' + this.id + ': sound object must list its sound files');
      this.sounds = json.sound.map((s) => {
        if (typeof s !== 'string' || !s.length) throw new Error('object ' + this.id + ': sound entry ' + JSON.stringify(s) + ' is not a file name');
        return s;
      });
      this.playbackmode = typeof json.playbackmode === 'string' ? json.playbackmode : null;
      if (this.playbackmode !== null && this.playbackmode !== 'loop' && this.playbackmode !== 'random' && this.playbackmode !== 'once') {
        throw new Error('object ' + this.id + ': playbackmode "' + this.playbackmode + '" is not loop, random or once');
      }
      this.volume = this.setting('volume', 'float', 1);
      this.mintime = this.setting('mintime', 'float', 0);
      this.maxtime = this.setting('maxtime', 'float', 0);
      this.elements = [];
      this.timer = 0;
      this.paused = false;
    }

    // The bridge sets element.volume/muted for the host; the layer volume multiplies it here.
    attachVolume(el) {
      const proto = Object.getOwnPropertyDescriptor(HTMLMediaElement.prototype, 'volume');
      let host = 1;
      const apply = () => { proto.set.call(el, Math.max(0, Math.min(1, host * this.volume.getNumber()))); };
      Object.defineProperty(el, 'volume', {
        configurable: true,
        get: () => host,
        set: (v) => { host = Number(v); apply(); },
      });
      this.volume.listen(apply);
      apply();
    }

    async setup() {
      const container = this.scene.mediaContainer();
      for (const file of this.sounds) {
        const el = document.createElement('audio');
        el.preload = 'auto';
        el.src = this.scene.loader.url(file);
        el.loop = this.playbackmode === 'loop';
        this.attachVolume(el);
        container.appendChild(el);
        this.elements.push(el);
      }
      if (this.playbackmode === 'random') {
        for (const el of this.elements) el.addEventListener('ended', () => this.scheduleRandom());
        this.playRandom();
      } else {
        for (const el of this.elements) this.play(el);
      }
      this.initialized = true;
    }

    play(el) {
      const p = el.play();
      if (p && typeof p.catch === 'function') p.catch((e) => console.error('sound object ' + this.id + ' could not start ' + el.src, e));
    }

    playRandom() {
      if (this.paused || !this.elements.length) return;
      const el = this.elements[Math.floor(Math.random() * this.elements.length)];
      el.currentTime = 0;
      this.play(el);
    }

    scheduleRandom() {
      const min = this.mintime.getNumber(), max = Math.max(min, this.maxtime.getNumber());
      const delay = min + Math.random() * (max - min);
      clearTimeout(this.timer);
      this.timer = setTimeout(() => { this.timer = 0; this.playRandom(); }, delay * 1000);
    }

    setPaused(paused) {
      this.paused = paused;
      for (const el of this.elements) {
        if (paused) el.pause();
        else if (!el.ended || this.playbackmode === 'loop') this.play(el);
      }
      if (paused) { clearTimeout(this.timer); this.timer = 0; }
      else if (this.playbackmode === 'random' && this.elements.every((el) => el.paused)) this.scheduleRandom();
    }

    dispose() {
      clearTimeout(this.timer);
      for (const el of this.elements) { el.pause(); el.removeAttribute('src'); el.load(); el.remove(); }
      this.elements = [];
    }
  }

  /**
   * Light layer: "light" names the kind (point), "color", "intensity" and "radius" feed the
   * g_LightsPosition / g_LightsColorPremultiplied / g_LightsRadius uniforms of lit shaders.
   */
  class Light extends SceneObject {
    constructor(scene, json) {
      super(scene, json);
      this.kind = typeof json.light === 'string' ? json.light : 'point';
      if (this.kind !== 'point') throw new Error('object ' + this.id + ': light kind "' + this.kind + '" is not supported (point lights are)');
      this.color = this.setting('color', 'color', [1, 1, 1], true);
      this.intensity = this.setting('intensity', 'float', 1);
      this.radius = this.setting('radius', 'float', 100);
    }

    // Position in the scene's centred, y-up space (the space image quads are placed in).
    worldPosition() {
      const t = this.resolveTransform();
      return [t.origin[0] - this.scene.width / 2, this.scene.height / 2 - t.origin[1], t.origin[2]];
    }

    premultipliedColor() {
      const c = this.color.getVec(3), i = this.intensity.getNumber();
      return [c[0] * i, c[1] * i, c[2] * i];
    }
  }

  // ---- registry ------------------------------------------------------------------------------
  const registry = [];

  /**
   * Register a constructor for objects carrying `key` in their scene.json entry; higher
   * priority wins when several keys are present. `ctor` is `new (scene, json)`, a SceneObject.
   */
  function register(key, ctor, priority) {
    registry.push({ key, ctor, priority: priority || 0 });
    registry.sort((a, b) => b.priority - a.priority);
  }

  // CScene::dispatchObjectType: the first registered key present picks the type; objects with no
  // known key (groups, "solid" placeholders) become plain SceneObjects.
  function create(scene, json) {
    for (const entry of registry) {
      if (json[entry.key] !== undefined && json[entry.key] !== null) return new entry.ctor(scene, json);
    }
    return new SceneObject(scene, json);
  }

  register('sound', Sound);
  register('light', Light);

  const api = { SceneObject, Sound, Light, register, create, registry, parseDependencies, rotateVec2 };
  G.WEObjects = api;
  if (typeof module !== 'undefined' && module.exports) module.exports = api;
})();
