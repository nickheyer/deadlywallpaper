// Wallpaper Engine's web API for pages Deadly Wallpaper runs: property, general, pause,
// directory, audio and media-integration events, plus the `file:///` rewrite that lets pages
// load the user's files through the wallpaper server. Injected before any page script runs.
(() => {
  'use strict';
  if (window.__dwp && window.__dwp.engine === 'wallpaper-engine') return;

  // ---- where this page is served from: `/` on the custom protocol, `/c/<id>/` on Plasma
  const base = (() => {
    const m = /^\/c\/\d+\//.exec(location.pathname);
    return m ? m[0] : '/';
  })();

  // ---- file:/// rewriting -----------------------------------------------------------------
  const FILE_URL = /^\s*file:\/\/(?:localhost)?(\/[^?#]*)/i;
  const encodeSegment = (s) => encodeURIComponent(s).replace(/[!'()*]/g, (c) => '%' + c.charCodeAt(0).toString(16).toUpperCase());
  const fileUrl = (u) => {
    if (typeof u !== 'string') return u;
    const m = FILE_URL.exec(u);
    if (!m) return u;
    let path = m[1].replace(/\\/g, '/');
    try { path = decodeURIComponent(path); } catch (e) { /* the page did not encode it */ }
    if (/^\/[A-Za-z]:\//.test(path)) path = path.slice(1);
    const encoded = path.split('/').map(encodeSegment).join('/');
    const tail = u.slice(m[0].length).trim();
    return base + '__file' + (encoded.startsWith('/') ? '' : '/') + encoded + tail;
  };
  const CSS_URL = /url\(\s*(['"]?)(file:\/\/[^'")]*)\1\s*\)/gi;
  const cssText = (t) => (typeof t === 'string' && t.indexOf('file:') >= 0) ? t.replace(CSS_URL, (all, q, url) => 'url(' + q + fileUrl(url) + q + ')') : t;
  const srcset = (t) => (typeof t === 'string' && t.indexOf('file:') >= 0)
    ? t.split(',').map((c) => { const p = c.trim().split(/\s+/); p[0] = fileUrl(p[0]); return p.join(' '); }).join(', ')
    : t;
  const URL_ATTRS = { src: fileUrl, href: fileUrl, poster: fileUrl, data: fileUrl, srcset: srcset, style: cssText };

  const hookAccessor = (proto, name, fix) => {
    if (!proto) return;
    const d = Object.getOwnPropertyDescriptor(proto, name);
    if (!d || !d.set) return;
    Object.defineProperty(proto, name, {
      configurable: true, enumerable: d.enumerable,
      get: d.get,
      set(v) { d.set.call(this, fix(v)); },
    });
  };
  for (const proto of [HTMLImageElement, HTMLMediaElement, HTMLSourceElement, HTMLScriptElement, HTMLIFrameElement, HTMLEmbedElement, HTMLTrackElement, HTMLInputElement].map((c) => c && c.prototype)) hookAccessor(proto, 'src', fileUrl);
  for (const proto of [HTMLImageElement, HTMLSourceElement].map((c) => c && c.prototype)) hookAccessor(proto, 'srcset', srcset);
  hookAccessor(HTMLVideoElement.prototype, 'poster', fileUrl);
  hookAccessor(HTMLLinkElement.prototype, 'href', fileUrl);
  hookAccessor(HTMLAnchorElement.prototype, 'href', fileUrl);
  hookAccessor(HTMLObjectElement.prototype, 'data', fileUrl);
  const setAttribute = Element.prototype.setAttribute;
  Element.prototype.setAttribute = function (name, value) {
    const fix = URL_ATTRS[String(name).toLowerCase()];
    return setAttribute.call(this, name, fix ? fix(String(value)) : value);
  };
  const setAttributeNS = Element.prototype.setAttributeNS;
  Element.prototype.setAttributeNS = function (ns, name, value) {
    const local = String(name).split(':').pop().toLowerCase();
    const fix = URL_ATTRS[local];
    return setAttributeNS.call(this, ns, name, fix ? fix(String(value)) : value);
  };
  const cssProto = CSSStyleDeclaration.prototype;
  const setProperty = cssProto.setProperty;
  cssProto.setProperty = function (name, value, priority) { return setProperty.call(this, name, cssText(value), priority); };
  for (const name of ['background', 'backgroundImage', 'borderImage', 'borderImageSource', 'content', 'cursor', 'listStyle', 'listStyleImage', 'mask', 'maskImage', 'webkitMaskImage', 'cssText']) hookAccessor(cssProto, name, cssText);
  const fetchOrig = window.fetch;
  if (typeof fetchOrig === 'function') {
    window.fetch = function (input, init) {
      if (typeof input === 'string') input = fileUrl(input);
      else if (input instanceof URL) input = fileUrl(input.href);
      else if (input instanceof Request && FILE_URL.test(input.url)) input = new Request(fileUrl(input.url), input);
      return fetchOrig.call(this, input, init);
    };
  }
  const xhrOpen = XMLHttpRequest.prototype.open;
  XMLHttpRequest.prototype.open = function (method, url, ...rest) { return xhrOpen.call(this, method, fileUrl(String(url)), ...rest); };
  const AudioOrig = window.Audio;
  if (typeof AudioOrig === 'function') {
    const Audio = function (src) { return src === undefined ? new AudioOrig() : new AudioOrig(fileUrl(String(src))); };
    Audio.prototype = AudioOrig.prototype;
    window.Audio = Audio;
  }
  const fixElement = (el) => {
    if (!(el instanceof Element)) return;
    for (const attr of ['src', 'href', 'poster', 'data', 'srcset']) {
      const v = el.getAttribute(attr);
      if (v && /file:/i.test(v)) setAttribute.call(el, attr, URL_ATTRS[attr](v));
    }
    const style = el.getAttribute('style');
    if (style && /file:/i.test(style)) setAttribute.call(el, 'style', cssText(style));
    if (el instanceof HTMLStyleElement && el.textContent && /file:/i.test(el.textContent)) el.textContent = cssText(el.textContent);
  };
  const sweep = (root) => {
    fixElement(root);
    if (root.querySelectorAll) root.querySelectorAll('[src],[href],[poster],[data],[srcset],[style],style').forEach(fixElement);
  };
  const observe = () => {
    if (!document.documentElement) return;
    new MutationObserver((records) => {
      for (const r of records) {
        if (r.type === 'attributes') fixElement(r.target);
        else if (r.type === 'characterData') { const el = r.target.parentElement; if (el instanceof HTMLStyleElement) fixElement(el); }
        else r.addedNodes.forEach(sweep);
      }
    }).observe(document.documentElement, { childList: true, subtree: true, characterData: true, attributes: true, attributeFilter: ['src', 'href', 'poster', 'data', 'srcset', 'style'] });
    sweep(document.documentElement);
  };
  if (document.documentElement) observe(); else document.addEventListener('readystatechange', observe, { once: true });
  document.addEventListener('DOMContentLoaded', () => sweep(document.documentElement));

  // ---- frame pacing: Wallpaper Engine's FPS limit and pause ------------------------------
  const rafOrig = window.requestAnimationFrame.bind(window);
  let fpsLimit = 0, paused = false, queue = [], nextId = 1, driving = false, next = 0;
  const schedule = () => { if (!driving && !paused && queue.length) { driving = true; rafOrig(drive); } };
  function drive(ts) {
    driving = false;
    if (paused) return;
    const interval = fpsLimit > 0 ? 1000 / fpsLimit : 0;
    if (interval > 0 && ts + 0.5 < next) { schedule(); return; }
    next = interval > 0 ? Math.max(next + interval, ts + interval * 0.5) : ts;
    const cbs = queue;
    queue = [];
    for (const [, cb] of cbs) { try { cb(ts); } catch (e) { console.error(e); } }
    schedule();
  }
  window.requestAnimationFrame = (cb) => { const id = nextId++; queue.push([id, cb]); schedule(); return id; };
  window.cancelAnimationFrame = (id) => { queue = queue.filter(([i]) => i !== id); };

  // ---- the property listener and its event queue -----------------------------------------
  let listener = window.wallpaperPropertyListener;
  let pendingUser = {}, pendingGeneral = null, pendingPaused = null, pendingDirs = [];
  let flushTimer = 0;
  const call = (fn, ...args) => { try { fn(...args); } catch (e) { console.error('wallpaper listener', e); } };
  const flush = () => {
    flushTimer = 0;
    const l = listener;
    if (!l || typeof l !== 'object') return;
    if (pendingGeneral && typeof l.applyGeneralProperties === 'function') { const g = pendingGeneral; pendingGeneral = null; call(l.applyGeneralProperties.bind(l), g); }
    const names = Object.keys(pendingUser);
    if (names.length && typeof l.applyUserProperties === 'function') { const u = pendingUser; pendingUser = {}; call(l.applyUserProperties.bind(l), u); }
    if (pendingDirs.length) {
      const dirs = pendingDirs;
      pendingDirs = [];
      for (const [kind, name, files] of dirs) {
        const fn = kind === 'added' ? l.userDirectoryFilesAddedOrChanged : l.userDirectoryFilesRemoved;
        if (typeof fn === 'function') call(fn.bind(l), name, files); else pendingDirs.push([kind, name, files]);
      }
    }
    if (pendingPaused !== null && typeof l.setPaused === 'function') { const p = pendingPaused; pendingPaused = null; call(l.setPaused.bind(l), p); }
  };
  const flushSoon = () => { if (!flushTimer) flushTimer = setTimeout(flush, 16); };
  Object.defineProperty(window, 'wallpaperPropertyListener', {
    configurable: true, enumerable: true,
    get: () => listener,
    set: (v) => { listener = v; flushSoon(); },
  });
  document.addEventListener('DOMContentLoaded', flushSoon);
  window.addEventListener('load', flushSoon);
  // A listener created lazily after load still receives what was queued.
  const lateStart = Date.now();
  const late = setInterval(() => { if (listener) flush(); if (Date.now() - lateStart > 30000) clearInterval(late); }, 250);

  // ---- directories, audio and media integration ------------------------------------------
  const dirs = {};
  let audioListener = null;
  const mediaListeners = { status: null, properties: null, thumbnail: null, playback: null, timeline: null };
  const mediaLast = { status: null, properties: null, thumbnail: null, playback: null, timeline: null };
  const registerMedia = (kind) => (fn) => {
    mediaListeners[kind] = typeof fn === 'function' ? fn : null;
    if (mediaListeners[kind] && mediaLast[kind]) call(mediaListeners[kind], mediaLast[kind]);
  };
  window.wallpaperMediaIntegration = {
    PLAYBACK_STOPPED: 0, PLAYBACK_PLAYING: 1, PLAYBACK_PAUSED: 2,
    playback: { STOPPED: 0, PLAYING: 1, PAUSED: 2 },
  };
  window.wallpaperRegisterAudioListener = (fn) => { audioListener = typeof fn === 'function' ? fn : null; };
  window.wallpaperRegisterMediaStatusListener = registerMedia('status');
  window.wallpaperRegisterMediaPropertiesListener = registerMedia('properties');
  window.wallpaperRegisterMediaThumbnailListener = registerMedia('thumbnail');
  window.wallpaperRegisterMediaPlaybackListener = registerMedia('playback');
  window.wallpaperRegisterMediaTimelineListener = registerMedia('timeline');
  window.wallpaperRequestRandomFileForProperty = (name, cb) => {
    if (typeof cb !== 'function') return;
    const files = dirs[name] ? dirs[name].files : [];
    const pick = files.length ? files[Math.floor(Math.random() * files.length)] : '';
    setTimeout(() => call(cb, name, pick), 0);
  };

  // ---- media elements: volume, mute, pause -----------------------------------------------
  const media = () => Array.from(document.querySelectorAll('video,audio'));
  let volume = 1, muted = false;
  const applyVolume = () => media().forEach((m) => { m.volume = volume; m.muted = muted; });
  document.addEventListener('DOMContentLoaded', applyVolume);
  document.addEventListener('play', applyVolume, true);

  window.__dwp = {
    engine: 'wallpaper-engine',
    base,
    fileUrl,
    prop: (name, value) => { pendingUser[name] = { value }; flushSoon(); },
    props: (obj) => { for (const name of Object.keys(obj || {})) pendingUser[name] = { value: obj[name] }; flushSoon(); },
    general: (g) => {
      pendingGeneral = Object.assign(pendingGeneral || {}, g || {});
      if (g && typeof g.fps === 'number') { fpsLimit = g.fps; }
      flushSoon();
    },
    dir: (name, files, fetchall) => {
      const previous = dirs[name] ? dirs[name].files : [];
      const list = Array.isArray(files) ? files.map(String) : [];
      dirs[name] = { files: list, fetchall: !!fetchall };
      if (fetchall) {
        const had = new Set(previous);
        const have = new Set(list);
        const added = list.filter((f) => !had.has(f));
        const removed = previous.filter((f) => !have.has(f));
        if (removed.length) pendingDirs.push(['removed', name, removed]);
        if (added.length) pendingDirs.push(['added', name, added]);
        flushSoon();
      }
    },
    pause: (p) => {
      paused = !!p;
      media().forEach((m) => { if (paused) m.pause(); else m.play().catch(() => {}); });
      pendingPaused = paused;
      flushSoon();
      if (!paused) schedule();
    },
    volume: (v) => { volume = v; applyVolume(); },
    mute: (m) => { muted = m; applyVolume(); },
    audio: (bins) => { if (audioListener) call(audioListener, bins); },
    media: (event, data) => {
      mediaLast[event] = data;
      const fn = mediaListeners[event];
      if (fn) call(fn, data);
    },
    mouse: (kind, x, y) => {
      const target = document.elementFromPoint(x, y) || document.body || document.documentElement;
      if (!target) return;
      const init = { bubbles: true, cancelable: true, clientX: x, clientY: y, screenX: x, screenY: y, button: 0, buttons: kind === 'mousemove' ? 0 : 1, view: window };
      target.dispatchEvent(new MouseEvent(kind, init));
      if (kind === 'mouseup') target.dispatchEvent(new MouseEvent('click', init));
    },
    view: (w, h, k, r, x, y, sw, sh) => {
      const s = document.documentElement.style;
      s.width = w + 'px'; s.height = h + 'px'; s.overflow = 'hidden'; s.transformOrigin = '50% 50%';
      s.transform = 'translate(' + (sw / 2 - w / 2 + x) + 'px, ' + (sh / 2 - h / 2 + y) + 'px) rotate(' + r + 'deg) scale(' + k + ')';
    },
    unview: () => {
      const s = document.documentElement.style;
      s.width = ''; s.height = ''; s.overflow = ''; s.transformOrigin = ''; s.transform = '';
    },
  };
})();
