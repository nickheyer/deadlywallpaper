// Wallpaper Engine `.tex` containers: header, mipmaps (raw, LZ4, embedded image files or
// embedded video), texture variants, sprite-sheet frames, and their upload to WebGL2.
(() => {
  'use strict';
  const G = typeof window !== 'undefined' ? window : globalThis;

  const FORMAT = { RGBA8888: 0, RGB888: 1, RGB565: 2, DXT5: 4, DXT3: 6, DXT1: 7, RG88: 8, R8: 9, RG1616F: 10, R16F: 11, BC7: 12, RGBA1010102: 13, RGBA16F: 14, RGB16F: 15 };
  const FLAG = { NO_INTERPOLATION: 1, CLAMP_UVS: 2, IS_GIF: 4, CLAMP_UVS_BORDER: 8, VIDEO: 32, ALPHA_PRIORITY: 0x80000 };
  const FORMAT_NAMES = { 0: 'RGBA8888', 1: 'RGB888', 2: 'RGB565', 4: 'DXT5', 6: 'DXT3', 7: 'DXT1', 8: 'RG88', 9: 'R8', 10: 'RG1616f', 11: 'R16f', 12: 'BC7', 13: 'RGBA1010102', 14: 'RGBA16161616f', 15: 'RGB161616f' };

  class Reader {
    constructor(bytes, name) {
      this.bytes = bytes;
      this.view = new DataView(bytes.buffer, bytes.byteOffset, bytes.byteLength);
      this.pos = 0;
      this.name = name;
    }
    need(n, what) {
      if (this.pos + n > this.bytes.length) throw new Error(this.name + ': truncated ' + what + ' at byte ' + this.pos);
    }
    i32(what) { this.need(4, what || 'int'); const v = this.view.getInt32(this.pos, true); this.pos += 4; return v; }
    u32(what) { this.need(4, what || 'int'); const v = this.view.getUint32(this.pos, true); this.pos += 4; return v; }
    f32(what) { this.need(4, what || 'float'); const v = this.view.getFloat32(this.pos, true); this.pos += 4; return v; }
    u8(what) { this.need(1, what || 'byte'); return this.bytes[this.pos++]; }
    bytesOf(n, what) { this.need(n, what || 'data'); const v = this.bytes.subarray(this.pos, this.pos + n); this.pos += n; return v; }
    // A 9-byte "TEXV0005\0" style stamp.
    stamp(prefix) {
      this.need(9, 'version stamp');
      let s = '';
      for (let i = 0; i < 8; i++) s += String.fromCharCode(this.bytes[this.pos + i]);
      if (!s.startsWith(prefix)) throw new Error(this.name + ': expected a ' + prefix + ' stamp at byte ' + this.pos + ', found ' + JSON.stringify(s));
      this.pos += 9;
      const n = parseInt(s.slice(4), 10);
      if (!Number.isFinite(n)) throw new Error(this.name + ': unreadable ' + prefix + ' version ' + JSON.stringify(s));
      return n;
    }
    peekStamp(prefix) {
      if (this.pos + 9 > this.bytes.length) return false;
      for (let i = 0; i < prefix.length; i++) if (this.bytes[this.pos + i] !== prefix.charCodeAt(i)) return false;
      return true;
    }
    cstring(what) {
      let end = this.pos;
      while (end < this.bytes.length && this.bytes[end] !== 0) end++;
      if (end >= this.bytes.length) throw new Error(this.name + ': unterminated ' + (what || 'string'));
      const s = new TextDecoder().decode(this.bytes.subarray(this.pos, end));
      this.pos = end + 1;
      return s;
    }
    get remaining() { return this.bytes.length - this.pos; }
  }

  // The file type an embedded payload carries, from its first bytes.
  function sniff(data) {
    const n = data.length;
    if (n >= 8 && data[0] === 0x89 && data[1] === 0x50 && data[2] === 0x4e && data[3] === 0x47) return 'image/png';
    if (n >= 3 && data[0] === 0xff && data[1] === 0xd8 && data[2] === 0xff) return 'image/jpeg';
    if (n >= 6 && data[0] === 0x47 && data[1] === 0x49 && data[2] === 0x46 && data[3] === 0x38) return 'image/gif';
    if (n >= 2 && data[0] === 0x42 && data[1] === 0x4d) return 'image/bmp';
    if (n >= 12 && data[8] === 0x57 && data[9] === 0x45 && data[10] === 0x42 && data[11] === 0x50) return 'image/webp';
    if (n >= 4 && ((data[0] === 0x49 && data[1] === 0x49 && data[2] === 0x2a && data[3] === 0) || (data[0] === 0x4d && data[1] === 0x4d && data[2] === 0 && data[3] === 0x2a))) return 'image/tiff';
    if (n >= 12 && data[4] === 0x66 && data[5] === 0x74 && data[6] === 0x79 && data[7] === 0x70) {
      const box = ((data[0] << 24) | (data[1] << 16) | (data[2] << 8) | data[3]) >>> 0;
      if (box >= 12 && box <= n) return 'video/mp4';
    }
    if (n >= 4 && data[0] === 0x1a && data[1] === 0x45 && data[2] === 0xdf && data[3] === 0xa3) return 'video/webm';
    return null;
  }

  // FreeImage format numbers Wallpaper Engine writes for image containers.
  const FIF_MIME = { 0: 'image/bmp', 2: 'image/jpeg', 13: 'image/png', 17: 'image/x-tga', 18: 'image/tiff', 25: 'image/gif', 35: 'image/webp' };

  /**
   * Parse a .tex file. `selectVariant(conditionJson)` decides whether a texture variant's
   * condition holds (it receives the JSON text stored in the file) and may be omitted when
   * there are no user properties to bind to.
   */
  function parse(bytes, name, selectVariant) {
    const r = new Reader(bytes, name || 'texture');
    const texv = r.stamp('TEXV');
    const early = texv === 4;
    const texi = early ? 1 : r.stamp('TEXI');
    if (texv < 4 || texv > 5 || texi !== 1) throw new Error(r.name + ': unsupported texture version TEXV' + texv + '/TEXI' + texi);
    const format = r.i32('format');
    if (FORMAT_NAMES[format] === undefined) throw new Error(r.name + ': unknown texture format ' + format);
    const flags = r.u32('flags');
    const textureWidth = r.i32('texture width');
    const textureHeight = r.i32('texture height');
    const width = r.i32('width');
    const height = r.i32('height');
    if (!early) r.u32('reserved');
    const texb = early ? 1 : r.stamp('TEXB');
    if (texb < 1 || texb > 4) throw new Error(r.name + ': unsupported mipmap container TEXB' + texb);
    const imageCount = early ? 1 : r.i32('image count');
    if (imageCount <= 0 || imageCount > 4096) throw new Error(r.name + ': invalid image count ' + imageCount);
    let imageType = -1;
    if (texb >= 3) imageType = r.i32('image type');
    let variantCount = 0;
    if (texb >= 4) variantCount = r.u32('variant count');
    const conditions = [];
    for (let i = 0; i < variantCount; i++) {
      conditions.push({ group: r.u32('variant group'), id: r.u32('variant id'), flags: r.u32('variant flags'), json: r.cstring('variant condition') });
    }
    // Variants: the first condition per group that holds wins; none holding keeps the base.
    const selected = new Map();
    if (conditions.length) {
      const taken = new Set();
      for (const c of conditions) {
        if (taken.has(c.group)) continue;
        const hit = selectVariant ? selectVariant(c.json) : false;
        if (hit) { taken.add(c.group); selected.set(c.id, c.flags); }
      }
    }

    const images = [];
    let payloadMime = imageType >= 0 ? (FIF_MIME[imageType] || 'image/unknown-' + imageType) : null;
    let isVideo = false;
    for (let i = 0; i < imageCount; i++) {
      const mipCount = r.i32('mipmap count');
      if (mipCount <= 0 || mipCount > 32) throw new Error(r.name + ': invalid mipmap count ' + mipCount);
      const mipmaps = [];
      for (let k = 0; k < mipCount; k++) {
        const w = r.i32('mipmap width');
        const h = r.i32('mipmap height');
        let lz4 = false, decompressed = 0;
        if (texb >= 2) { lz4 = r.i32('compression') === 1; decompressed = r.i32('decompressed size'); }
        const stored = r.i32('mipmap size');
        if (w <= 0 || h <= 0 || stored <= 0 || stored > r.remaining) throw new Error(r.name + ': invalid mipmap ' + k + ' (' + w + 'x' + h + ', ' + stored + ' bytes)');
        let data = r.bytesOf(stored, 'mipmap data');
        if (lz4) {
          if (decompressed <= 0 || decompressed > stored * 255) throw new Error(r.name + ': invalid LZ4 size ' + decompressed);
          data = G.WELZ4.decodeBlock(data, decompressed);
        }
        if (texb >= 3 && imageType < 0 && i === 0 && k === 0) {
          const kind = sniff(data);
          if (kind && kind.startsWith('video/')) { isVideo = true; payloadMime = kind; }
          else if (kind) payloadMime = kind;
        }
        const mip = { width: w, height: h, data };
        if (conditions.length) {
          const groups = r.u32('variant patch groups');
          const patches = [];
          for (let g = 0; g < groups; g++) {
            const count = r.u32('variant patch count');
            for (let p = 0; p < count; p++) {
              r.u32('variant patch reserved');
              const patch = { id: r.u32('patch id'), x: r.u32('patch x'), y: r.u32('patch y'), w: r.u32('patch w'), h: r.u32('patch h'), fif: r.u32('patch fif'), size: r.u32('patch size') };
              patch.data = r.bytesOf(patch.size, 'patch data');
              patches.push(patch);
            }
          }
          for (const patch of patches) {
            if (!selected.has(patch.id)) continue;
            const vflags = selected.get(patch.id);
            if (vflags !== 0 || payloadMime) {
              throw new Error(r.name + ': texture variant ' + patch.id + ' uses flags ' + vflags + (payloadMime ? ' on an embedded ' + payloadMime + ' payload' : '') + ', which has no reference layout to follow');
            }
            applyPatch(r.name, patch, lz4, format, mip);
          }
        }
        mipmaps.push(mip);
      }
      images.push({ mipmaps });
    }

    const animated = (flags & FLAG.IS_GIF) !== 0;
    const frames = [];
    let animatedVersion = 0, gifWidth = 0, gifHeight = 0;
    if (animated) {
      animatedVersion = r.stamp('TEXS');
      if (animatedVersion < 1 || animatedVersion > 3) throw new Error(r.name + ': unsupported frame table TEXS' + animatedVersion);
      const frameCount = r.i32('frame count');
      if (frameCount < 0 || frameCount > 65536) throw new Error(r.name + ': invalid frame count ' + frameCount);
      if (animatedVersion === 3) { gifWidth = r.i32('gif width'); gifHeight = r.i32('gif height'); }
      for (let i = 0; i < frameCount; i++) {
        const imageId = r.i32('frame image');
        const frametime = r.f32('frame time');
        const num = animatedVersion === 1 ? () => r.i32('frame coordinate') : () => r.f32('frame coordinate');
        const x = num(), y = num(), width1 = num(), width2 = num(), height2 = num(), height1 = num();
        if (imageId < 0 || imageId >= images.length) throw new Error(r.name + ': frame ' + i + ' refers to image ' + imageId + ' of ' + images.length);
        frames.push({ imageId, frametime, x, y, width1, width2, height2, height1 });
      }
      if (animatedVersion < 3 && frames.length) { gifWidth = frames[0].width1; gifHeight = frames[0].height1; }
    }

    // Sprite-sheet grid: frames laid out in a grid over one image.
    let spritesheet = null;
    if (frames.length && width > 0 && height > 0) {
      const fw = frames[0].width1, fh = frames[0].height1;
      if (fw > 0 && fh > 0) {
        const cols = Math.round(width / fw), rows = Math.round(height / fh);
        if (cols > 0 && rows > 0 && cols * rows >= frames.length) {
          let duration = 0;
          for (const f of frames) duration += f.frametime;
          spritesheet = { cols, rows, frames: frames.length, duration };
        }
      }
    }

    return {
      name: r.name, format, formatName: FORMAT_NAMES[format], flags, textureWidth, textureHeight, width, height,
      containerVersion: texb, imageType, payloadMime, isVideo, images, animated, animatedVersion, frames,
      gifWidth, gifHeight, spritesheet, variants: conditions.length,
    };
  }

  function blockLayout(format) {
    switch (format) {
      case FORMAT.DXT1: return { w: 4, h: 4, bytes: 8 };
      case FORMAT.DXT3: case FORMAT.DXT5: return { w: 4, h: 4, bytes: 16 };
      case FORMAT.RG88: return { w: 1, h: 1, bytes: 2 };
      case FORMAT.R8: return { w: 1, h: 1, bytes: 1 };
      default: return { w: 1, h: 1, bytes: 4 };
    }
  }

  // Copy a variant patch into a mipmap in place (row by row, block rows for DXT).
  function applyPatch(name, patch, lz4, format, mip) {
    const layout = blockLayout(format);
    const W = mip.width, H = mip.height;
    const { x, y, w, h } = patch;
    if (w === 0 || h === 0 || patch.size === 0 || x > W || w > W - x || y > H || h > H - y) {
      throw new Error(name + ': texture variant ' + patch.id + ' lies outside its mipmap');
    }
    let bytes = patch.data;
    if (lz4) bytes = G.WELZ4.decodeBlock(bytes, 4 * w * h);
    const row = (w * layout.bytes) / layout.w;
    const rows = Math.ceil(h / layout.h);
    if (bytes.length < row * rows) throw new Error(name + ': texture variant ' + patch.id + ' is shorter than its rectangle');
    for (let r = 0; r < rows; r++) {
      const dst = ((y + r * layout.h) * W * layout.bytes) / (layout.w * layout.h) + (x * layout.bytes) / layout.w;
      mip.data.set(bytes.subarray(r * row, r * row + row), dst);
    }
  }

  // ---- DXT decoding ------------------------------------------------------------------------
  function rgb565(v) {
    const r = (v >> 11) & 31, g = (v >> 5) & 63, b = v & 31;
    return [(r << 3) | (r >> 2), (g << 2) | (g >> 4), (b << 3) | (b >> 2)];
  }

  function decodeColorBlocks(src, out, width, height, stride, colorOffset, dxt1) {
    const view = new DataView(src.buffer, src.byteOffset, src.byteLength);
    const bw = Math.ceil(width / 4), bh = Math.ceil(height / 4);
    const pal = new Uint8Array(16);
    for (let by = 0; by < bh; by++) {
      for (let bx = 0; bx < bw; bx++) {
        const base = (by * bw + bx) * stride + colorOffset;
        const c0 = view.getUint16(base, true), c1 = view.getUint16(base + 2, true);
        const [r0, g0, b0] = rgb565(c0), [r1, g1, b1] = rgb565(c1);
        pal[0] = r0; pal[1] = g0; pal[2] = b0; pal[3] = 255;
        pal[4] = r1; pal[5] = g1; pal[6] = b1; pal[7] = 255;
        if (!dxt1 || c0 > c1) {
          pal[8] = ((2 * r0 + r1) / 3) | 0; pal[9] = ((2 * g0 + g1) / 3) | 0; pal[10] = ((2 * b0 + b1) / 3) | 0; pal[11] = 255;
          pal[12] = ((r0 + 2 * r1) / 3) | 0; pal[13] = ((g0 + 2 * g1) / 3) | 0; pal[14] = ((b0 + 2 * b1) / 3) | 0; pal[15] = 255;
        } else {
          pal[8] = ((r0 + r1) / 2) | 0; pal[9] = ((g0 + g1) / 2) | 0; pal[10] = ((b0 + b1) / 2) | 0; pal[11] = 255;
          pal[12] = 0; pal[13] = 0; pal[14] = 0; pal[15] = 0;
        }
        const bits = view.getUint32(base + 4, true);
        for (let py = 0; py < 4; py++) {
          const y = by * 4 + py;
          if (y >= height) break;
          for (let px = 0; px < 4; px++) {
            const x = bx * 4 + px;
            if (x >= width) break;
            const sel = ((bits >>> (2 * (py * 4 + px))) & 3) * 4;
            const d = (y * width + x) * 4;
            out[d] = pal[sel]; out[d + 1] = pal[sel + 1]; out[d + 2] = pal[sel + 2]; out[d + 3] = pal[sel + 3];
          }
        }
      }
    }
  }

  function decodeDXT1(src, width, height) {
    const out = new Uint8Array(width * height * 4);
    decodeColorBlocks(src, out, width, height, 8, 0, true);
    return out;
  }

  function decodeDXT3(src, width, height) {
    const out = new Uint8Array(width * height * 4);
    decodeColorBlocks(src, out, width, height, 16, 8, false);
    const bw = Math.ceil(width / 4), bh = Math.ceil(height / 4);
    for (let by = 0; by < bh; by++) {
      for (let bx = 0; bx < bw; bx++) {
        const base = (by * bw + bx) * 16;
        for (let i = 0; i < 16; i++) {
          const x = bx * 4 + (i & 3), y = by * 4 + (i >> 2);
          if (x >= width || y >= height) continue;
          const byte = src[base + (i >> 1)];
          const nib = (i & 1) ? (byte >> 4) : (byte & 15);
          out[(y * width + x) * 4 + 3] = nib * 17;
        }
      }
    }
    return out;
  }

  function decodeDXT5(src, width, height) {
    const out = new Uint8Array(width * height * 4);
    decodeColorBlocks(src, out, width, height, 16, 8, false);
    const bw = Math.ceil(width / 4), bh = Math.ceil(height / 4);
    const alphas = new Uint8Array(8);
    for (let by = 0; by < bh; by++) {
      for (let bx = 0; bx < bw; bx++) {
        const base = (by * bw + bx) * 16;
        const a0 = src[base], a1 = src[base + 1];
        alphas[0] = a0; alphas[1] = a1;
        if (a0 > a1) {
          for (let k = 2; k < 8; k++) alphas[k] = (((8 - k) * a0 + (k - 1) * a1) / 7) | 0;
        } else {
          for (let k = 2; k < 6; k++) alphas[k] = (((6 - k) * a0 + (k - 2) * a1) / 5) | 0;
          alphas[6] = 0; alphas[7] = 255;
        }
        // 48 bits of 3-bit indices, little endian, read as two 24-bit halves.
        const lo = src[base + 2] | (src[base + 3] << 8) | (src[base + 4] << 16);
        const hi = src[base + 5] | (src[base + 6] << 8) | (src[base + 7] << 16);
        for (let i = 0; i < 16; i++) {
          const x = bx * 4 + (i & 3), y = by * 4 + (i >> 2);
          const idx = i < 8 ? (lo >>> (3 * i)) & 7 : (hi >>> (3 * (i - 8))) & 7;
          if (x >= width || y >= height) continue;
          out[(y * width + x) * 4 + 3] = alphas[idx];
        }
      }
    }
    return out;
  }

  function decodeToRGBA(format, mip) {
    const { width, height, data } = mip;
    const need = (n) => { if (data.length < n) throw new Error('mipmap ' + width + 'x' + height + ' holds ' + data.length + ' bytes, ' + FORMAT_NAMES[format] + ' needs ' + n); };
    switch (format) {
      case FORMAT.RGBA8888: need(width * height * 4); return data.subarray(0, width * height * 4);
      case FORMAT.R8: {
        need(width * height);
        const out = new Uint8Array(width * height * 4);
        for (let i = 0; i < width * height; i++) { const v = data[i]; out[i * 4] = v; out[i * 4 + 1] = v; out[i * 4 + 2] = v; out[i * 4 + 3] = 255; }
        return out;
      }
      case FORMAT.RG88: {
        need(width * height * 2);
        const out = new Uint8Array(width * height * 4);
        for (let i = 0; i < width * height; i++) { out[i * 4] = data[i * 2]; out[i * 4 + 1] = data[i * 2 + 1]; out[i * 4 + 2] = 0; out[i * 4 + 3] = 255; }
        return out;
      }
      case FORMAT.DXT1: need(Math.ceil(width / 4) * Math.ceil(height / 4) * 8); return decodeDXT1(data, width, height);
      case FORMAT.DXT3: need(Math.ceil(width / 4) * Math.ceil(height / 4) * 16); return decodeDXT3(data, width, height);
      case FORMAT.DXT5: need(Math.ceil(width / 4) * Math.ceil(height / 4) * 16); return decodeDXT5(data, width, height);
      default: throw new Error('texture format ' + FORMAT_NAMES[format] + ' has no decoder');
    }
  }

  // ---- WebGL upload ------------------------------------------------------------------------
  function applySampling(gl, flags, levels) {
    const wrap = (flags & FLAG.CLAMP_UVS) || (flags & FLAG.CLAMP_UVS_BORDER) ? gl.CLAMP_TO_EDGE : gl.REPEAT;
    gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_WRAP_S, wrap);
    gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_WRAP_T, wrap);
    const nearest = (flags & FLAG.NO_INTERPOLATION) !== 0;
    gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MAG_FILTER, nearest ? gl.NEAREST : gl.LINEAR);
    if (levels > 1) {
      gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MIN_FILTER, nearest ? gl.NEAREST_MIPMAP_NEAREST : gl.LINEAR_MIPMAP_LINEAR);
    } else {
      gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MIN_FILTER, nearest ? gl.NEAREST : gl.LINEAR);
    }
    gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_BASE_LEVEL, 0);
    gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MAX_LEVEL, Math.max(0, levels - 1));
    const aniso = gl.getExtension('EXT_texture_filter_anisotropic');
    if (aniso) gl.texParameterf(gl.TEXTURE_2D, aniso.TEXTURE_MAX_ANISOTROPY_EXT, Math.min(8, gl.getParameter(aniso.MAX_TEXTURE_MAX_ANISOTROPY_EXT)));
  }

  function decodeImageBlob(data, mime) {
    const blob = new Blob([data], { type: mime });
    return createImageBitmap(blob, { premultiplyAlpha: 'none', colorSpaceConversion: 'none' });
  }

  /** A texture ready for rendering: one WebGL texture per image of the file. */
  class Texture {
    constructor(gl, parsed) {
      this.gl = gl;
      this.name = parsed.name;
      this.format = parsed.format;
      this.flags = parsed.flags;
      this.parsed = parsed;
      this.ids = [];
      this.mipDims = [];
      this.frames = parsed.frames;
      this.animated = parsed.animated;
      this.spritesheet = parsed.spritesheet;
      this.animationTime = parsed.frames.reduce((t, f) => t + f.frametime, 0);
      this.isVideo = parsed.isVideo;
      this.video = null;
      this.videoReady = false;
      this.resolution = new Float32Array(4);
      this.ready = true;
    }
    get realWidth() { return this.animated ? this.parsed.gifWidth : this.parsed.width; }
    get realHeight() { return this.animated ? this.parsed.gifHeight : this.parsed.height; }
    textureWidth(i) { const d = this.mipDims[i] || this.mipDims[0]; return d ? d[0] : this.parsed.textureWidth; }
    textureHeight(i) { const d = this.mipDims[i] || this.mipDims[0]; return d ? d[1] : this.parsed.textureHeight; }
    textureId(i) { return this.ids[i] === undefined ? this.ids[0] : this.ids[i]; }
    isReady() { return this.isVideo ? this.videoReady : this.ready; }
    setResolution() {
      const p = this.parsed;
      if (this.animated) this.resolution.set([p.textureWidth, p.textureHeight, p.gifWidth, p.gifHeight]);
      else if (p.payloadMime) this.resolution.set([this.textureWidth(0), this.textureHeight(0), p.width, p.height]);
      else this.resolution.set([p.textureWidth, p.textureHeight, p.width, p.height]);
    }
    // Upload the current video frame; other textures need no per-frame work.
    update() {
      if (!this.video || this.video.readyState < 2) return;
      const gl = this.gl;
      gl.bindTexture(gl.TEXTURE_2D, this.ids[0]);
      // Video frames are top-down; the texture keeps the bottom-up layout of raw payloads.
      gl.pixelStorei(gl.UNPACK_FLIP_Y_WEBGL, true);
      gl.texImage2D(gl.TEXTURE_2D, 0, gl.RGBA8, gl.RGBA, gl.UNSIGNED_BYTE, this.video);
      gl.pixelStorei(gl.UNPACK_FLIP_Y_WEBGL, false);
      this.videoReady = true;
    }
    setPaused(paused) {
      if (!this.video) return;
      if (paused) this.video.pause(); else this.video.play().catch(() => {});
    }
    dispose() {
      for (const id of this.ids) this.gl.deleteTexture(id);
      this.ids = [];
      if (this.video) { this.video.pause(); this.video.removeAttribute('src'); this.video.load(); this.video = null; }
    }
  }

  /** Build a Texture from a parsed file; embedded image payloads decode asynchronously. */
  async function createTexture(gl, parsed) {
    const tex = new Texture(gl, parsed);
    const s3tc = gl.getExtension('WEBGL_compressed_texture_s3tc') || gl.getExtension('WEBGL_compressed_texture_s3tc_srgb');
    const isDxt = parsed.format === FORMAT.DXT1 || parsed.format === FORMAT.DXT3 || parsed.format === FORMAT.DXT5;

    if (parsed.isVideo) {
      const mip = parsed.images[0].mipmaps[0];
      const video = document.createElement('video');
      video.muted = true;
      video.loop = true;
      video.playsInline = true;
      video.preload = 'auto';
      video.src = URL.createObjectURL(new Blob([mip.data], { type: parsed.payloadMime }));
      tex.video = video;
      const id = gl.createTexture();
      gl.bindTexture(gl.TEXTURE_2D, id);
      gl.texImage2D(gl.TEXTURE_2D, 0, gl.RGBA8, parsed.textureWidth, parsed.textureHeight, 0, gl.RGBA, gl.UNSIGNED_BYTE, null);
      applySampling(gl, parsed.flags | FLAG.CLAMP_UVS, 1);
      tex.ids.push(id);
      tex.mipDims.push([parsed.textureWidth, parsed.textureHeight]);
      tex.setResolution();
      tex.ready = true;
      await new Promise((resolve, reject) => {
        video.addEventListener('loadeddata', () => resolve(), { once: true });
        video.addEventListener('error', () => reject(new Error(parsed.name + ': the embedded ' + parsed.payloadMime + ' video could not be decoded')), { once: true });
        video.play().catch(() => {});
      });
      tex.update();
      return tex;
    }

    for (let i = 0; i < parsed.images.length; i++) {
      const mips = parsed.images[i].mipmaps;
      const id = gl.createTexture();
      gl.bindTexture(gl.TEXTURE_2D, id);
      gl.pixelStorei(gl.UNPACK_FLIP_Y_WEBGL, false);
      gl.pixelStorei(gl.UNPACK_PREMULTIPLY_ALPHA_WEBGL, false);
      gl.pixelStorei(gl.UNPACK_COLORSPACE_CONVERSION_WEBGL, gl.NONE);
      if (parsed.payloadMime) {
        // An image file per mipmap; only the first level is meaningful for these.
        const bitmap = await decodeImageBlob(mips[0].data, parsed.payloadMime);
        gl.bindTexture(gl.TEXTURE_2D, id);
        gl.pixelStorei(gl.UNPACK_ALIGNMENT, 4);
        // Raw .tex payloads are stored bottom-up (first row at v=0, as linux-wallpaperengine
        // uploads them); decoded image files arrive top-down, so they are flipped to match.
        gl.pixelStorei(gl.UNPACK_FLIP_Y_WEBGL, true);
        gl.texImage2D(gl.TEXTURE_2D, 0, gl.RGBA8, gl.RGBA, gl.UNSIGNED_BYTE, bitmap);
        gl.pixelStorei(gl.UNPACK_FLIP_Y_WEBGL, false);
        tex.mipDims.push([bitmap.width, bitmap.height]);
        bitmap.close();
        applySampling(gl, parsed.flags, 1);
      } else {
        const compressed = isDxt && s3tc && mips.every((m) => m.width % 4 === 0 && m.height % 4 === 0);
        for (let level = 0; level < mips.length; level++) {
          const mip = mips[level];
          if (level === 0) tex.mipDims.push([mip.width, mip.height]);
          if (compressed) {
            const fmt = parsed.format === FORMAT.DXT1 ? s3tc.COMPRESSED_RGBA_S3TC_DXT1_EXT : parsed.format === FORMAT.DXT3 ? s3tc.COMPRESSED_RGBA_S3TC_DXT3_EXT : s3tc.COMPRESSED_RGBA_S3TC_DXT5_EXT;
            const size = Math.ceil(mip.width / 4) * Math.ceil(mip.height / 4) * (parsed.format === FORMAT.DXT1 ? 8 : 16);
            if (mip.data.length < size) throw new Error(parsed.name + ': mipmap ' + level + ' is shorter than its DXT block grid');
            gl.compressedTexImage2D(gl.TEXTURE_2D, level, fmt, mip.width, mip.height, 0, mip.data.subarray(0, size));
          } else if (parsed.format === FORMAT.R8) {
            if (mip.data.length < mip.width * mip.height) throw new Error(parsed.name + ': mipmap ' + level + ' is shorter than R8 needs');
            gl.pixelStorei(gl.UNPACK_ALIGNMENT, 1);
            gl.texImage2D(gl.TEXTURE_2D, level, gl.R8, mip.width, mip.height, 0, gl.RED, gl.UNSIGNED_BYTE, mip.data.subarray(0, mip.width * mip.height));
          } else if (parsed.format === FORMAT.RG88) {
            if (mip.data.length < mip.width * mip.height * 2) throw new Error(parsed.name + ': mipmap ' + level + ' is shorter than RG88 needs');
            gl.pixelStorei(gl.UNPACK_ALIGNMENT, 2);
            gl.texImage2D(gl.TEXTURE_2D, level, gl.RG8, mip.width, mip.height, 0, gl.RG, gl.UNSIGNED_BYTE, mip.data.subarray(0, mip.width * mip.height * 2));
          } else {
            const rgba = decodeToRGBA(parsed.format, mip);
            gl.pixelStorei(gl.UNPACK_ALIGNMENT, 4);
            gl.texImage2D(gl.TEXTURE_2D, level, gl.RGBA8, mip.width, mip.height, 0, gl.RGBA, gl.UNSIGNED_BYTE, rgba);
          }
        }
        applySampling(gl, parsed.flags, mips.length);
      }
      tex.ids.push(id);
    }
    tex.setResolution();
    return tex;
  }

  const api = { FORMAT, FLAG, FORMAT_NAMES, parse, decodeToRGBA, decodeDXT1, decodeDXT3, decodeDXT5, sniff, createTexture, Texture, applySampling };
  G.WETex = api;
  if (typeof module !== 'undefined' && module.exports) module.exports = api;
})();
