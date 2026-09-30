// Scene asset access: the page's base URL, Wallpaper Engine's file layout (shaders/, materials/,
// models/, effects/) and the virtual files linux-wallpaperengine adds for its bloom pass.
// Port of Assets/AssetLocator.cpp and the virtual container in Application/WallpaperApplication.cpp.
(() => {
  'use strict';
  const G = typeof window !== 'undefined' ? window : globalThis;

  const PAGE = '__deadlywp/scene.html';

  // Files linux-wallpaperengine registers in its virtual container (WallpaperApplication.cpp:
  // setupContainer). They implement the camera bloom on top of Wallpaper Engine's own
  // materials/util/* materials and give effect "copy" commands a shader.
  const VIRTUAL = {
    'effects/wpenginelinux/bloomeffect.json': JSON.stringify({
      name: 'camerabloom_wpengine_linux',
      group: 'wpengine_linux_camera',
      dependencies: [],
      passes: [
        { material: 'materials/util/downsample_quarter_bloom.json', target: '_rt_4FrameBuffer', bind: [{ name: '_rt_FullFrameBuffer', index: 0 }] },
        { material: 'materials/util/downsample_eighth_blur_v.json', target: '_rt_8FrameBuffer', bind: [{ name: '_rt_4FrameBuffer', index: 0 }] },
        { material: 'materials/util/blur_h_bloom.json', target: '_rt_Bloom', bind: [{ name: '_rt_8FrameBuffer', index: 0 }] },
        { material: 'materials/util/combine.json', target: '_rt_FullFrameBuffer', bind: [{ name: '_rt_imageLayerComposite_-1_a', index: 0 }, { name: '_rt_Bloom', index: 1 }] },
      ],
    }),
    'models/wpenginelinux.json': JSON.stringify({ material: 'materials/wpenginelinux.json' }),
    'materials/wpenginelinux.json': JSON.stringify({
      passes: [{ blending: 'normal', cullmode: 'nocull', depthtest: 'disabled', depthwrite: 'disabled', shader: 'genericimage2', textures: ['_rt_FullFrameBuffer'] }],
    }),
    'shaders/commands/copy.frag': 'uniform sampler2D g_Texture0;\nin vec2 v_TexCoord;\nvoid main () {\nout_FragColor = texture (g_Texture0, v_TexCoord);\n}',
    'shaders/commands/copy.vert': 'in vec3 a_Position;\nin vec2 a_TexCoord;\nout vec2 v_TexCoord;\nvoid main () {\ngl_Position = vec4 (a_Position, 1.0);\nv_TexCoord = a_TexCoord;\n}',
  };

  // The server root the scene is served from: "/" under the custom protocol, "/c/<id>/" on Plasma.
  function pageBase() {
    const path = G.location.pathname;
    if (!path.endsWith(PAGE)) throw new Error('scene page served from an unexpected path: ' + path);
    return path.slice(0, path.length - PAGE.length);
  }

  // std::filesystem::path::replace_extension: swap the last extension of the final segment, or append one.
  function replaceExtension(name, ext) {
    const slash = name.lastIndexOf('/');
    const dot = name.lastIndexOf('.');
    const stem = dot > slash ? name.slice(0, dot) : name;
    return stem + '.' + ext;
  }

  class Loader {
    constructor(base) {
      this.base = base;
      this.cache = new Map();
    }

    url(rel) {
      return this.base + rel.split('/').map(encodeURIComponent).join('/');
    }

    fetchRaw(rel, kind) {
      const key = kind + ':' + rel;
      if (this.cache.has(key)) return this.cache.get(key);
      const promise = (async () => {
        const response = await fetch(this.url(rel));
        if (!response.ok) throw new Error(rel + ': HTTP ' + response.status + ' ' + response.statusText);
        return kind === 'text' ? response.text() : new Uint8Array(await response.arrayBuffer());
      })();
      this.cache.set(key, promise);
      promise.catch(() => this.cache.delete(key));
      return promise;
    }

    async text(rel) {
      if (Object.prototype.hasOwnProperty.call(VIRTUAL, rel)) return VIRTUAL[rel];
      return this.fetchRaw(rel, 'text');
    }

    async bytes(rel) {
      if (Object.prototype.hasOwnProperty.call(VIRTUAL, rel)) return new TextEncoder().encode(VIRTUAL[rel]);
      return this.fetchRaw(rel, 'bytes');
    }

    async json(rel) {
      const text = await this.text(rel);
      try {
        return JSON.parse(text);
      } catch (e) {
        throw new Error(rel + ': invalid JSON (' + e.message + ')');
      }
    }

    // AssetLocator::shader: workshop shaders may have a compatibility replacement under
    // zcompat/scene/shaders/<workshop id>/<file>; otherwise shaders live under shaders/.
    async shader(name) {
      const parts = name.split('/');
      if (parts[0] === 'workshop' && parts.length >= 3) {
        const compat = 'zcompat/scene/shaders/' + parts[1] + '/' + parts.slice(2).join('/');
        try {
          return await this.text(compat);
        } catch (e) {
          // The replacement is optional; the workshop shader itself is loaded below.
        }
      }
      return this.text('shaders/' + name);
    }

    // AssetLocator::vertexShader / fragmentShader / includeShader
    vertexShader(name) { return this.shader(replaceExtension(name, 'vert')); }
    fragmentShader(name) { return this.shader(replaceExtension(name, 'frag')); }
    include(name) { return this.shader(replaceExtension(name, 'h')); }

    // AssetLocator::texture: "<name>" -> materials/<name>.tex (names that already carry the
    // materials/ prefix are used as they are).
    texture(name) {
      const rel = (name.startsWith('materials/') ? name : 'materials/' + name) + '.tex';
      return this.bytes(rel);
    }
  }

  const api = { Loader, pageBase, replaceExtension, VIRTUAL, PAGE };
  G.WELoader = api;
  if (typeof module !== 'undefined' && module.exports) module.exports = api;
})();
