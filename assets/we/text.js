// Wallpaper Engine text layers, rasterised with the 2D canvas into a texture the image
// pipeline draws like any other layer: font loading, line layout (wrapping, alignment,
// padding, row and width limits, the bounding box) and painting.
(() => {
  'use strict';
  const G = typeof window !== 'undefined' ? window : globalThis;

  // Point sizes are at 300 DPI, as Wallpaper Engine defines them.
  const POINT_SCALE = 300 / 72;
  const LINE_HEIGHT = 1.2;
  const loadedFonts = new Map();

  // Register a font file shipped with the scene under a family name of its own.
  async function loadFont(loader, path) {
    if (loadedFonts.has(path)) return loadedFonts.get(path);
    const family = 'we-font-' + path.replace(/[^A-Za-z0-9]/g, '_');
    const promise = (async () => {
      const bytes = await loader.bytes(path);
      const face = new FontFace(family, bytes.buffer.slice(bytes.byteOffset, bytes.byteOffset + bytes.byteLength));
      await face.load();
      document.fonts.add(face);
      return family;
    })();
    loadedFonts.set(path, promise);
    return promise;
  }

  // CSS family for a scene "font" value: "systemfont_<name>" names an installed family, a file
  // path the family loadFont registered for it, nothing the default sans-serif.
  function familyFor(font, loaded) {
    if (!font) return 'sans-serif';
    if (font.startsWith('systemfont_')) {
      const name = font.slice('systemfont_'.length).replace(/_/g, ' ');
      return '"' + name + '", sans-serif';
    }
    if (typeof loaded !== 'string' || !loaded.length) throw new Error('font ' + font + ' was not loaded before its family was requested');
    return '"' + loaded + '", sans-serif';
  }

  function pixelSize(pointSize) { return Math.max(1, pointSize * POINT_SCALE); }

  // Word wrap every paragraph to `maxWidth` pixels (0: no wrapping); `measure(text)` gives a width.
  function wrapLines(measure, text, maxWidth) {
    const out = [];
    for (const paragraph of String(text).split(/\r?\n/)) {
      if (!(maxWidth > 0)) { out.push(paragraph); continue; }
      const words = paragraph.split(' ');
      let line = '';
      for (const word of words) {
        const candidate = line ? line + ' ' + word : word;
        if (measure(candidate) <= maxWidth || !line) line = candidate;
        else { out.push(line); line = word; }
      }
      out.push(line);
    }
    return out;
  }

  /**
   * Lay `text` out. `opts`: pointSize, padding, horizontal ('left'|'center'|'right'),
   * vertical ('top'|'center'|'bottom'), size [w,h] of the layer box (0 = fit the text),
   * limitWidth/maxWidth (wrap at that many pixels), limitRows/maxRows. `measure(text)` returns
   * the pixel width of a line at the layout's pixel size. Returns the lines, the canvas size,
   * the line height and the pen position of the first line.
   */
  function layout(text, opts, measure) {
    const px = pixelSize(opts.pointSize);
    const padding = Math.max(0, opts.padding || 0);
    const maxWidth = opts.limitWidth && opts.maxWidth > 0 ? Math.max(1, opts.maxWidth - padding * 2) : 0;
    let lines = wrapLines(measure, text, maxWidth);
    if (opts.limitRows && opts.maxRows > 0) lines = lines.slice(0, Math.trunc(opts.maxRows));
    const lineHeight = px * LINE_HEIGHT;
    let width = 0;
    for (const l of lines) width = Math.max(width, measure(l));
    width = Math.ceil(width + padding * 2);
    let height = Math.ceil(lines.length * lineHeight + padding * 2);
    if (opts.size && opts.size[0] > 0 && opts.size[1] > 0) { width = Math.ceil(opts.size[0]); height = Math.ceil(opts.size[1]); }
    width = Math.max(1, width);
    height = Math.max(1, height);
    const block = lines.length * lineHeight;
    const vertical = opts.vertical === 'top' || opts.vertical === 'bottom' ? opts.vertical : 'center';
    const horizontal = opts.horizontal === 'left' || opts.horizontal === 'right' ? opts.horizontal : 'center';
    let y = padding;
    if (vertical === 'center') y = (height - block) / 2;
    else if (vertical === 'bottom') y = height - block - padding;
    const x = horizontal === 'left' ? padding : horizontal === 'right' ? width - padding : width / 2;
    return { px, lines, lineHeight, width, height, x, y, horizontal, vertical };
  }

  function cssColor(c, alpha) {
    return 'rgba(' + Math.round(c[0] * 255) + ',' + Math.round(c[1] * 255) + ',' + Math.round(c[2] * 255) + ',' + alpha + ')';
  }

  // Paint a layout into a canvas: `opts.family`, `opts.color` [r,g,b] (0..1), `opts.alpha`,
  // `opts.background` [r,g,b] or null.
  function paint(laid, opts) {
    const canvas = document.createElement('canvas');
    canvas.width = laid.width;
    canvas.height = laid.height;
    const ctx = canvas.getContext('2d');
    ctx.clearRect(0, 0, canvas.width, canvas.height);
    if (opts.background) {
      ctx.fillStyle = cssColor(opts.background, 1);
      ctx.fillRect(0, 0, canvas.width, canvas.height);
    }
    ctx.font = laid.px + 'px ' + opts.family;
    ctx.textBaseline = 'top';
    ctx.textAlign = laid.horizontal;
    ctx.fillStyle = cssColor(opts.color || [1, 1, 1], opts.alpha === undefined ? 1 : opts.alpha);
    let y = laid.y;
    for (const line of laid.lines) { ctx.fillText(line, laid.x, y); y += laid.lineHeight; }
    return { canvas, width: canvas.width, height: canvas.height, layout: laid };
  }

  // Measure with a canvas context at the layout's pixel size and family.
  function measurer(pointSize, family) {
    const ctx = document.createElement('canvas').getContext('2d');
    ctx.font = pixelSize(pointSize) + 'px ' + family;
    return (text) => ctx.measureText(text).width;
  }

  /** Draw `text` into a canvas with the layout and paint options above. */
  function rasterize(text, opts) {
    return paint(layout(text, opts, measurer(opts.pointSize, opts.family)), opts);
  }

  const api = { loadFont, familyFor, layout, paint, rasterize, wrapLines, pixelSize, measurer, POINT_SCALE, LINE_HEIGHT };
  G.WEText = api;
  if (typeof module !== 'undefined' && module.exports) module.exports = api;
})();
