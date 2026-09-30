// Text layers: scene.json objects with a "text" value. The text is rasterised (text.js) into a
// canvas texture registered with the scene's texture cache, and the layer draws it through the
// image pipeline (effects, blending, alignment, parallax) with genericimage2. Every bound value
// (text, font, point size, colour, alignment, padding, limits, background) re-rasterises when it
// changes; the layer's "anchor" places the quad the way an image's alignment does.
(() => {
  'use strict';
  const G = typeof window !== 'undefined' ? window : globalThis;

  const TEXTURE_PREFIX = '_we_text_';
  const SHADER = 'genericimage2';
  const WHITE = [1, 1, 1];
  const WHITE4 = [1, 1, 1, 1];

  /** A 2D canvas uploaded as a texture, in the TextureProvider shape render passes bind. */
  class CanvasTexture {
    constructor(gl, name) {
      this.gl = gl;
      this.name = name;
      this.texture = gl.createTexture();
      this.width = 1;
      this.height = 1;
      this.resolution = new Float32Array([1, 1, 1, 1]);
      this.frames = [{ imageId: 0, frametime: 0, x: 0, y: 0, width1: 1, width2: 1, height2: 1, height1: 1 }];
      this.animated = false;
      this.spritesheet = null;
      this.animationTime = 0;
      this.flags = G.WETex.FLAG.CLAMP_UVS;
      this.format = G.WETex.FORMAT.RGBA8888;
      this.uploads = 0;
    }
    // Canvas rows run top-down; texture row 0 is the bottom, as .tex images are stored.
    upload(canvas) {
      const gl = this.gl;
      gl.bindTexture(gl.TEXTURE_2D, this.texture);
      gl.pixelStorei(gl.UNPACK_FLIP_Y_WEBGL, true);
      gl.texImage2D(gl.TEXTURE_2D, 0, gl.RGBA, gl.RGBA, gl.UNSIGNED_BYTE, canvas);
      gl.pixelStorei(gl.UNPACK_FLIP_Y_WEBGL, false);
      G.WETex.applySampling(gl, this.flags, 1);
      gl.bindTexture(gl.TEXTURE_2D, null);
      this.width = canvas.width;
      this.height = canvas.height;
      this.resolution.set([canvas.width, canvas.height, canvas.width, canvas.height]);
      this.frames[0].width1 = this.frames[0].width2 = canvas.width;
      this.frames[0].height1 = this.frames[0].height2 = canvas.height;
      this.uploads++;
    }
    get realWidth() { return this.width; }
    get realHeight() { return this.height; }
    textureId() { return this.texture; }
    textureWidth() { return this.width; }
    textureHeight() { return this.height; }
    isReady() { return this.uploads > 0; }
    update() {}
    setPaused() {}
    dispose() { this.gl.deleteTexture(this.texture); this.texture = null; }
  }

  class Text extends G.WEImage.Image {
    constructor(scene, json) {
      super(scene, json);
      // For text "horizontalalign" aligns the lines; "anchor" places the quad like an image's alignment.
      this.anchor = this.setting('anchor', 'string', 'center');
      this.alignment = this.anchor.getString() || 'center';
      this.text = this.setting('text', 'string', '');
      this.font = this.setting('font', 'string', '');
      this.pointsize = this.setting('pointsize', 'float', 32);
      this.padding = this.setting('padding', 'float', 0);
      this.horizontalalign = this.setting('horizontalalign', 'string', typeof json.alignment === 'string' ? json.alignment : 'center');
      this.verticalalign = this.setting('verticalalign', 'string', 'center');
      this.opaquebackground = this.setting('opaquebackground', 'bool', false);
      this.backgroundcolor = this.setting('backgroundcolor', 'color', [0, 0, 0], true);
      this.limitrows = this.setting('limitrows', 'bool', false);
      this.maxrows = this.setting('maxrows', 'int', 0);
      this.limitwidth = this.setting('limitwidth', 'bool', false);
      this.maxwidth = this.setting('maxwidth', 'float', 0);
      this.family = 'sans-serif';
      this.canvasTexture = null;
      this.rasterDirty = true;
      this.fontLoading = null;
    }

    textureName() { return TEXTURE_PREFIX + this.id; }

    async resolveFamily() {
      const font = this.font.getString();
      if (!font) return 'sans-serif';
      if (font.startsWith('systemfont_')) return G.WEText.familyFor(font, null);
      return G.WEText.familyFor(font, await G.WEText.loadFont(this.scene.loader, font));
    }

    // A text layer's model: the rasterised text through genericimage2, translucent.
    async loadModel() {
      this.family = await this.resolveFamily();
      const name = this.textureName();
      this.canvasTexture = new CanvasTexture(this.scene.gl, name);
      this.scene.textures.store(name, this.canvasTexture);
      this.rasterize();
      const material = G.WEMaterial.parseMaterial({
        passes: [{ blending: 'translucent', cullmode: 'nocull', depthtest: 'disabled', depthwrite: 'disabled', shader: SHADER, textures: [name] }],
      }, 'object ' + this.id + ' text material', this.ctx());
      return {
        filename: 'object ' + this.id + ' text', material,
        solidlayer: false, fullscreen: false, passthrough: false, autosize: false, nopadding: false,
        width: null, height: null, puppet: null, animations: [],
      };
    }

    rasterOptions() {
      const size = this.sizeSetting.getVec(2);
      return {
        pointSize: this.pointsize.getNumber(), family: this.family,
        color: this.color.getVec(3), alpha: 1,
        horizontal: this.horizontalalign.getString(), vertical: this.verticalalign.getString(),
        padding: this.padding.getNumber(), size: [size[0], size[1]],
        background: this.opaquebackground.getBool() ? this.backgroundcolor.getVec(3) : null,
        limitWidth: this.limitwidth.getBool(), maxWidth: this.maxwidth.getNumber(),
        limitRows: this.limitrows.getBool(), maxRows: this.maxrows.getNumber(),
      };
    }

    rasterize() {
      const out = G.WEText.rasterize(this.text.getString(), this.rasterOptions());
      this.canvasTexture.upload(out.canvas);
      this.rasterDirty = false;
    }

    async setup() {
      await super.setup();
      const dirty = () => { this.rasterDirty = true; };
      for (const dyn of [this.text, this.pointsize, this.padding, this.horizontalalign, this.verticalalign, this.opaquebackground, this.backgroundcolor, this.limitrows, this.maxrows, this.limitwidth, this.maxwidth, this.color, this.sizeSetting]) dyn.listen(dirty);
      this.anchor.listen(() => { this.alignment = this.anchor.getString() || 'center'; });
      this.font.listen(() => {
        const pending = this.resolveFamily().then((family) => {
          if (this.fontLoading !== pending) return;
          this.fontLoading = null;
          this.family = family;
          this.rasterDirty = true;
        });
        this.fontLoading = pending;
        pending.catch((e) => this.scene.fail(e));
      });
    }

    render() {
      if (this.rasterDirty && this.canvasTexture) this.rasterize();
      super.render();
    }

    // The raster carries the text colour; the pass tints nothing so the background keeps its own.
    getColor() { return WHITE; }
    getColor4() { return WHITE4; }
    getCompositeColor() { return WHITE; }

    dispose() {
      super.dispose();
      if (this.canvasTexture) this.canvasTexture.dispose();
    }
  }

  G.WEObjects.register('text', Text, 15);

  const api = { Text, CanvasTexture, TEXTURE_PREFIX, SHADER };
  G.WETextLayer = api;
  if (typeof module !== 'undefined' && module.exports) module.exports = api;
})();
