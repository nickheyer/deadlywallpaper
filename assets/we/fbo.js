// Render targets: Wallpaper Engine's named framebuffers ("_rt_*"), their aliases and the
// per-effect providers that resolve them. Port of Render/CFBO.cpp and Render/FBOProvider.cpp.
(() => {
  'use strict';
  const G = typeof window !== 'undefined' ? window : globalThis;

  // Data/Assets/Texture.h TextureFormat / TextureFlags
  const FORMAT = { RGBA8888: 0, RGB888: 1, RGB565: 2, DXT5: 4, DXT3: 6, DXT1: 7, RG88: 8, R8: 9, RG1616F: 10, R16F: 11, BC7: 12, RGBA1010102: 13, RGBA16F: 14, RGB16F: 15 };
  const FLAG = { NO_FLAGS: 0, NO_INTERPOLATION: 1, CLAMP_UVS: 2, IS_GIF: 4, CLAMP_UVS_BORDER: 8, VIDEO: 32 };

  // effect.json "fbos[].format" -> renderable WebGL2 storage.
  function storageFor(gl, format) {
    const name = String(format || 'rgba8888').toLowerCase();
    switch (name) {
      case 'rgba8888': case 'rgba8': case 'argb8888': return { internal: gl.RGBA8, format: gl.RGBA, type: gl.UNSIGNED_BYTE, id: FORMAT.RGBA8888 };
      case 'r8': return { internal: gl.R8, format: gl.RED, type: gl.UNSIGNED_BYTE, id: FORMAT.R8 };
      case 'rg88': case 'rg8': return { internal: gl.RG8, format: gl.RG, type: gl.UNSIGNED_BYTE, id: FORMAT.RG88 };
      case 'rgba16f': case 'rgba16161616f': return { internal: gl.RGBA16F, format: gl.RGBA, type: gl.HALF_FLOAT, id: FORMAT.RGBA16F, float: true };
      case 'rg1616f': case 'rg16f': return { internal: gl.RG16F, format: gl.RG, type: gl.HALF_FLOAT, id: FORMAT.RG1616F, float: true };
      case 'r16f': return { internal: gl.R16F, format: gl.RED, type: gl.HALF_FLOAT, id: FORMAT.R16F, float: true };
      default: throw new Error('framebuffer format "' + format + '" is not one this renderer can draw into');
    }
  }

  /** CFBO: a colour texture with a framebuffer, exposing the TextureProvider interface. */
  class FBO {
    constructor(gl, name, format, flags, realWidth, realHeight, textureWidth, textureHeight) {
      this.gl = gl;
      this.name = name;
      this.flags = flags;
      const storage = typeof format === 'string' ? storageFor(gl, format) : storageFor(gl, 'rgba8888');
      if (storage.float && !gl.getExtension('EXT_color_buffer_float')) throw new Error('framebuffer ' + name + ' needs floating point colour attachments, which this WebGL2 context does not offer');
      this.format = storage.id;
      const tw = Math.max(1, Math.round(textureWidth)), th = Math.max(1, Math.round(textureHeight));
      const rw = Math.max(1, Math.round(realWidth)), rh = Math.max(1, Math.round(realHeight));
      this.framebuffer = gl.createFramebuffer();
      gl.bindFramebuffer(gl.FRAMEBUFFER, this.framebuffer);
      this.texture = gl.createTexture();
      gl.bindTexture(gl.TEXTURE_2D, this.texture);
      gl.texImage2D(gl.TEXTURE_2D, 0, storage.internal, tw, th, 0, storage.format, storage.type, null);
      G.WETex.applySampling(gl, flags, 1);
      gl.framebufferTexture2D(gl.FRAMEBUFFER, gl.COLOR_ATTACHMENT0, gl.TEXTURE_2D, this.texture, 0);
      gl.drawBuffers([gl.COLOR_ATTACHMENT0]);
      const status = gl.checkFramebufferStatus(gl.FRAMEBUFFER);
      if (status !== gl.FRAMEBUFFER_COMPLETE) throw new Error('framebuffer ' + name + ' is incomplete (status ' + status + ')');
      // Layer framebuffers start transparent (CFBO constructor).
      gl.clearColor(0, 0, 0, 0);
      gl.clear(gl.COLOR_BUFFER_BIT);
      gl.bindFramebuffer(gl.FRAMEBUFFER, null);
      this.width = tw;
      this.height = th;
      this.realWidth = rw;
      this.realHeight = rh;
      this.resolution = new Float32Array([tw, th, rw, rh]);
      this.frames = [{ imageId: 0, frametime: 0, x: 0, y: 0, width1: tw, width2: rw, height2: rh, height1: th }];
      this.animated = false;
      this.spritesheet = null;
      this.animationTime = 0;
    }
    textureId() { return this.texture; }
    textureWidth() { return this.width; }
    textureHeight() { return this.height; }
    isReady() { return true; }
    update() {}
    setPaused() {}
    // Clear to transparent black, keeping the caller's framebuffer binding.
    clear() {
      const gl = this.gl;
      const previous = gl.getParameter(gl.FRAMEBUFFER_BINDING);
      gl.bindFramebuffer(gl.FRAMEBUFFER, this.framebuffer);
      gl.disable(gl.SCISSOR_TEST);
      gl.clearColor(0, 0, 0, 0);
      gl.clear(gl.COLOR_BUFFER_BIT);
      gl.bindFramebuffer(gl.FRAMEBUFFER, previous);
    }
    dispose() {
      this.gl.deleteTexture(this.texture);
      this.gl.deleteFramebuffer(this.framebuffer);
      this.texture = null;
      this.framebuffer = null;
    }
  }

  /** FBOProvider: a named FBO registry with a parent chain (effect -> image -> scene). */
  class Provider {
    constructor(gl, parent) {
      this.gl = gl;
      this.parent = parent || null;
      this.fbos = new Map();
    }
    // FBOProvider::create(name, format, flags, realSize, textureSize)
    create(name, format, flags, realSize, textureSize) {
      const fbo = new FBO(this.gl, name, format, flags, realSize[0], realSize[1], textureSize[0], textureSize[1]);
      this.fbos.set(name, fbo);
      return fbo;
    }
    // FBOProvider::create(const FBO& base, flags, size): effect-declared fbos scaled down by "scale".
    createFromDefinition(definition, flags, size) {
      const scale = definition.scale > 0 ? definition.scale : 1;
      const w = size[0] / scale, h = size[1] / scale;
      const fbo = new FBO(this.gl, definition.name, definition.format, flags, w, h, w, h);
      this.fbos.set(definition.name, fbo);
      return fbo;
    }
    alias(newName, original) {
      const fbo = this.find(original);
      if (!fbo) throw new Error('cannot alias ' + newName + ': framebuffer ' + original + ' does not exist');
      this.fbos.set(newName, fbo);
      return fbo;
    }
    find(name) {
      if (this.fbos.has(name)) return this.fbos.get(name);
      return this.parent ? this.parent.find(name) : null;
    }
    dispose() {
      const seen = new Set();
      for (const fbo of this.fbos.values()) {
        if (seen.has(fbo)) continue;
        seen.add(fbo);
        fbo.dispose();
      }
      this.fbos.clear();
    }
  }

  const api = { FBO, Provider, FORMAT, FLAG, storageFor };
  G.WEFBO = api;
  if (typeof module !== 'undefined' && module.exports) module.exports = api;
})();
