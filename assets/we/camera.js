// The scene camera (orthographic projection with Wallpaper Engine's eye/center/up), parallax,
// shake and fade, the viewport mapping that fits the scene to the window, and the pointer.
// Ports Render/Camera.cpp, CScene::renderFrame's parallax, CScene::updateMouse and
// WallpaperState::updateTextureUVs<ZoomFillUVs>.
(() => {
  'use strict';
  const G = typeof window !== 'undefined' ? window : globalThis;
  const M = G.WEM;

  const FADE_SECONDS = 1;

  class Camera {
    constructor(data) {
      this.center = data.center;
      this.eye = data.eye;
      this.up = data.up;
      this.fov = data.fov;
      this.nearz = data.nearz;
      this.farz = data.farz;
      this.width = 0;
      this.height = 0;
      this.projection = M.identity();
      this.lookAt = M.lookAt(this.eye, this.center, this.up);
      this.viewProjection = M.identity();
      this.perspectiveViewProjection = M.identity();
    }

    // Camera::setOrthogonalProjection
    setOrthogonalProjection(width, height) {
      this.width = width;
      this.height = height;
      const projection = M.ortho(-width / 2, width / 2, -height / 2, height / 2, this.nearz.getNumber(), this.farz.getNumber());
      this.projection = M.translate(projection, this.eye[0], this.eye[1], this.eye[2]);
      this.updateFrame([0, 0]);
    }

    // Per-frame view-projection matrices: the orthographic one with the camera shake offset,
    // and the perspective one layers with "perspective" use. The perspective eye sits at the
    // distance where the z=0 plane spans exactly the orthographic extents, so flat layers match.
    updateFrame(shake) {
      const shaken = M.multiply(M.multiply(this.projection, this.lookAt), M.translation(shake[0], shake[1], 0));
      this.viewProjection = shaken;
      const fov = this.fov.getNumber() * Math.PI / 180;
      const distance = (this.height / 2) / Math.tan(fov / 2);
      const near = Math.max(this.nearz.getNumber(), 1);
      const far = Math.max(this.farz.getNumber(), distance * 2 + 1);
      const projection = M.perspective(fov, this.width / this.height, near, far);
      const view = M.lookAt([0, 0, distance], [0, 0, 0], [0, 1, 0]);
      this.perspectiveViewProjection = M.multiply(M.multiply(projection, view), M.translation(shake[0], shake[1], 0));
    }
  }

  /** CScene::renderFrame's parallax displacement, driven by the pointer. */
  class Parallax {
    constructor(settings) {
      this.enabled = settings.enabled;
      this.amount = settings.amount;
      this.delay = settings.delay;
      this.mouseInfluence = settings.mouseInfluence;
      this.displacement = new Float32Array(2);
    }
    update(dt, mousePosition) {
      if (!this.enabled.getBool()) return;
      const influence = this.mouseInfluence.getNumber();
      const amount = this.amount.getNumber();
      const delay = Math.min(1, Math.max(0, this.delay.getNumber() * dt));
      const cx = mousePosition[0] - 0.5, cy = mousePosition[1] - 0.5;
      const tx = cx * amount * influence, ty = cy * amount * influence;
      this.displacement[0] += (tx - this.displacement[0]) * delay;
      this.displacement[1] += (ty - this.displacement[1]) * delay;
    }
  }

  // Smooth value noise in [-1, 1] for the camera shake.
  function hash(n) {
    const x = Math.sin(n * 127.1 + 311.7) * 43758.5453;
    return (x - Math.floor(x)) * 2 - 1;
  }
  function noise1(t) {
    const i = Math.floor(t), f = t - i;
    const u = f * f * (3 - 2 * f);
    return hash(i) * (1 - u) + hash(i + 1) * u;
  }

  /** Camera shake: "camerashake" with amplitude (scene units), roughness and speed. */
  class Shake {
    constructor(settings) {
      this.enabled = settings.enabled;
      this.amplitude = settings.amplitude;
      this.roughness = settings.roughness;
      this.speed = settings.speed;
      this.offset = new Float32Array(2);
    }
    update(time) {
      if (!this.enabled.getBool()) { this.offset[0] = 0; this.offset[1] = 0; return; }
      const amplitude = this.amplitude.getNumber();
      const roughness = Math.min(1, Math.max(0, this.roughness.getNumber()));
      const t = time * this.speed.getNumber();
      const smooth = 1 - roughness;
      this.offset[0] = amplitude * (noise1(t) * smooth + noise1(t * 4 + 17.3) * roughness);
      this.offset[1] = amplitude * (noise1(t + 101.7) * smooth + noise1(t * 4 + 59.1) * roughness);
    }
  }

  /** "camerafade": the wallpaper fades in from black when it starts. */
  class Fade {
    constructor(enabled) { this.enabled = enabled; }
    factor(time) {
      if (!this.enabled.getBool()) return 1;
      return Math.min(1, Math.max(0, time / FADE_SECONDS));
    }
  }

  /**
   * WallpaperState (ZoomFillUVs): the scene texture covers the viewport keeping its aspect
   * ratio, centred, cropping the overflow; plus CScene::updateMouse's pointer mapping.
   */
  class Viewport {
    constructor() {
      this.width = 0;
      this.height = 0;
      this.projectionWidth = 0;
      this.projectionHeight = 0;
      this.uvs = { ustart: 0, uend: 1, vstart: 1, vend: 0 };
      this.mouse = { position: new Float32Array(2), positionLast: new Float32Array(2), normalized: new Float32Array(2) };
      this.pointer = [0.5, 0.5];
    }

    // WallpaperState::resetUVs (no vertical flip)
    resetUVs() { this.uvs = { ustart: 0, uend: 1, vstart: 1, vend: 0 }; }

    // WallpaperState::updateUs
    updateUs(projectionWidth, projectionHeight) {
      const newWidth = Math.trunc(this.height / projectionHeight * projectionWidth);
      const newCenter = newWidth / 2;
      const viewportCenter = this.width / 2;
      this.uvs.ustart = (newCenter - viewportCenter) / newWidth;
      this.uvs.uend = (newCenter + viewportCenter) / newWidth;
    }

    // WallpaperState::updateVs
    updateVs(projectionWidth, projectionHeight) {
      const newHeight = Math.trunc(this.width / projectionWidth * projectionHeight);
      const newCenter = newHeight / 2;
      const viewportCenter = this.height / 2;
      const down = newCenter - viewportCenter;
      const up = newCenter + viewportCenter;
      this.uvs.vstart = up / newHeight;
      this.uvs.vend = down / newHeight;
    }

    // WallpaperState::updateState + updateTextureUVs<ZoomFillUVs>
    update(width, height, projectionWidth, projectionHeight) {
      if (this.width === width && this.height === height && this.projectionWidth === projectionWidth && this.projectionHeight === projectionHeight) return;
      this.width = width;
      this.height = height;
      this.projectionWidth = projectionWidth;
      this.projectionHeight = projectionHeight;
      this.resetUVs();
      const m = Math.max(width / projectionWidth, height / projectionHeight);
      const scaledWidth = Math.trunc(projectionWidth * m);
      const scaledHeight = Math.trunc(projectionHeight * m);
      if (scaledWidth !== width) this.updateUs(scaledWidth, scaledHeight);
      else if (scaledHeight !== height) this.updateVs(scaledWidth, scaledHeight);
    }

    // Pointer in CSS pixels of the window (mousemove clientX/clientY, y down from the top, as
    // linux-wallpaperengine's input drivers report it).
    setPointer(x, y, cssWidth, cssHeight) {
      this.pointer = [cssWidth > 0 ? x / cssWidth : 0.5, cssHeight > 0 ? y / cssHeight : 0.5];
    }

    // CScene::updateMouse: viewport space -> the visible part of the scene texture.
    updateMouse() {
      const m = this.mouse;
      m.positionLast.set(m.position);
      const mouseX = Math.min(1, Math.max(0, this.pointer[0]));
      const normalizedY = Math.min(1, Math.max(0, this.pointer[1]));
      const uvs = this.uvs;
      m.normalized[0] = uvs.ustart + mouseX * (uvs.uend - uvs.ustart);
      m.normalized[1] = uvs.vstart + normalizedY * (uvs.vend - uvs.vstart);
      const mouseY = 1 - normalizedY;
      m.position[0] = m.normalized[0];
      m.position[1] = uvs.vstart + mouseY * (uvs.vend - uvs.vstart);
    }
  }

  const api = { Camera, Parallax, Shake, Fade, Viewport, noise1, FADE_SECONDS };
  G.WECamera = api;
  if (typeof module !== 'undefined' && module.exports) module.exports = api;
})();
