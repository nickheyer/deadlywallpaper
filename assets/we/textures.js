// The scene's texture cache: .tex files resolved by name, texture variants re-selected when the
// user properties they depend on change, and stable references passes bind through. Port of
// Render/TextureCache.cpp with live variant selection.
(() => {
  'use strict';
  const G = typeof window !== 'undefined' ? window : globalThis;

  /**
   * A stable handle to a texture that may be replaced (variant re-selection, texture user
   * properties). It forwards the TextureProvider interface to the current texture.
   */
  class TextureRef {
    constructor(name, texture) {
      this.name = name;
      this.current = texture;
    }
    get realWidth() { return this.current.realWidth; }
    get realHeight() { return this.current.realHeight; }
    get resolution() { return this.current.resolution; }
    get frames() { return this.current.frames; }
    get animated() { return this.current.animated; }
    get spritesheet() { return this.current.spritesheet; }
    get animationTime() { return this.current.animationTime; }
    get flags() { return this.current.flags; }
    get format() { return this.current.format; }
    textureId(i) { return this.current.textureId(i); }
    textureWidth(i) { return this.current.textureWidth(i); }
    textureHeight(i) { return this.current.textureHeight(i); }
    isReady() { return this.current.isReady(); }
    update() { this.current.update(); }
    setPaused(p) { this.current.setPaused(p); }
  }

  class Cache {
    constructor(gl, loader, properties, onError) {
      this.gl = gl;
      this.loader = loader;
      this.properties = properties;
      this.onError = onError;
      this.entries = new Map();
      properties.listen(() => { this.refreshVariants(); });
    }

    // TextureCache::store: register a provider (an FBO or a texture built elsewhere) by name.
    store(name, provider) {
      const existing = this.entries.get(name);
      if (existing) {
        existing.ref.current = provider;
        existing.bytes = null;
        existing.evaluations = [];
        return existing.ref;
      }
      const ref = new TextureRef(name, provider);
      this.entries.set(name, { ref, bytes: null, evaluations: [], promise: Promise.resolve(ref) });
      return ref;
    }

    // Parse the file with the live property values, recording which variant conditions were
    // consulted so a later property change can be checked against them.
    buildTexture(name, bytes) {
      const evaluations = [];
      const parsed = G.WETex.parse(bytes, name, (json) => {
        const hit = this.properties.conditionHolds(json);
        evaluations.push({ json, hit });
        return hit;
      });
      return G.WETex.createTexture(this.gl, parsed).then((texture) => ({ texture, evaluations, hasVariants: parsed.variants > 0 }));
    }

    // TextureCache::resolve: "<name>" -> materials/<name>.tex, cached by name.
    resolve(name) {
      const existing = this.entries.get(name);
      if (existing) return existing.promise;
      const entry = { ref: null, bytes: null, evaluations: [], promise: null };
      entry.promise = (async () => {
        const bytes = await this.loader.texture(name);
        const built = await this.buildTexture(name, bytes);
        entry.ref = new TextureRef(name, built.texture);
        entry.evaluations = built.evaluations;
        entry.bytes = built.hasVariants ? bytes : null;
        return entry.ref;
      })();
      this.entries.set(name, entry);
      entry.promise.catch(() => this.entries.delete(name));
      return entry.promise;
    }

    // Re-select variants whose recorded conditions changed outcome; the reference keeps its
    // identity so every pass bound to it sees the new texture.
    refreshVariants() {
      for (const entry of this.entries.values()) {
        if (!entry.bytes || !entry.ref) continue;
        const changed = entry.evaluations.some((e) => this.properties.conditionHolds(e.json) !== e.hit);
        if (!changed) continue;
        const bytes = entry.bytes;
        const pending = this.buildTexture(entry.ref.name, bytes).then((built) => {
          if (entry.rebuild !== pending) { built.texture.dispose(); return; }
          const old = entry.ref.current;
          entry.ref.current = built.texture;
          entry.evaluations = built.evaluations;
          if (old && typeof old.dispose === 'function') old.dispose();
        });
        entry.rebuild = pending;
        pending.catch((e) => this.onError(new Error('texture variant re-selection for ' + entry.ref.name + ' failed: ' + (e && e.message ? e.message : e))));
      }
    }

    update() {
      for (const entry of this.entries.values()) if (entry.ref) entry.ref.update();
    }

    setPaused(paused) {
      for (const entry of this.entries.values()) if (entry.ref) entry.ref.setPaused(paused);
    }

    dispose() {
      for (const entry of this.entries.values()) {
        if (entry.ref && entry.ref.current && typeof entry.ref.current.dispose === 'function') entry.ref.current.dispose();
      }
      this.entries.clear();
    }
  }

  const api = { Cache, TextureRef };
  G.WETextures = api;
  if (typeof module !== 'undefined' && module.exports) module.exports = api;
})();
