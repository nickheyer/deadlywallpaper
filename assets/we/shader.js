// Wallpaper Engine shader units: the include/require/combo/annotation preprocessing that
// linux-wallpaperengine performs, translated to GLSL ES 3.00 for WebGL2.
(() => {
  'use strict';
  const G = typeof window !== 'undefined' ? window : globalThis;

  const VERTEX = 0, FRAGMENT = 1;

  // Overloads that accept HLSL's argument orders; declared before the macros that route to them.
  const OVERLOADS = (() => {
    const out = [];
    const vec = ['vec2', 'vec3', 'vec4'];
    const ivec = ['ivec2', 'ivec3', 'ivec4'];
    for (const fn of ['max', 'min']) {
      const w = '_we' + fn;
      out.push(`float ${w}(float a, float b) { return ${fn}(a, b); }`);
      out.push(`int ${w}(int a, int b) { return ${fn}(a, b); }`);
      for (const t of vec) {
        out.push(`${t} ${w}(${t} a, ${t} b) { return ${fn}(a, b); }`);
        out.push(`${t} ${w}(${t} a, float b) { return ${fn}(a, b); }`);
        out.push(`${t} ${w}(float a, ${t} b) { return ${fn}(b, a); }`);
      }
      for (const t of ivec) {
        out.push(`${t} ${w}(${t} a, ${t} b) { return ${fn}(a, b); }`);
        out.push(`${t} ${w}(${t} a, int b) { return ${fn}(a, b); }`);
        out.push(`${t} ${w}(int a, ${t} b) { return ${fn}(b, a); }`);
      }
    }
    out.push('float _wepow(float a, float b) { return pow(a, b); }');
    for (const t of vec) {
      out.push(`${t} _wepow(${t} a, ${t} b) { return pow(a, b); }`);
      out.push(`${t} _wepow(${t} a, float b) { return pow(a, ${t}(b)); }`);
      out.push(`${t} _wepow(float a, ${t} b) { return pow(${t}(a), b); }`);
    }
    // Texture slots flagged ClampUVsBorder sample transparent black outside [0, 1] (GL_CLAMP_TO_BORDER
    // with the default border colour); WebGL2 has no border mode, so flagged slots are routed here.
    out.push('uniform int g_WEBorderMask;');
    out.push('bool _weOutside(int slot, vec2 uv) { return (g_WEBorderMask & (1 << slot)) != 0 && (uv.x < 0.0 || uv.x > 1.0 || uv.y < 0.0 || uv.y > 1.0); }');
    out.push('vec4 _weSampleBorder(sampler2D s, int slot, vec2 uv) { if (_weOutside(slot, uv)) return vec4(0.0); return texture(s, uv); }');
    out.push('vec4 _weSampleBorderLod(sampler2D s, int slot, vec2 uv, float lod) { if (_weOutside(slot, uv)) return vec4(0.0); return textureLod(s, uv, lod); }');
    return out.join('\n') + '\n';
  })();

  // Route material texture samples through the border-aware samplers (see OVERLOADS).
  function routeBorderSamples(body) {
    return body
      .replace(/\b(?:texSample2DLod|texture2DLod|textureLod)\s*\(\s*(g_Texture([0-7]))\s*,/g, '_weSampleBorderLod($1, $2,')
      .replace(/\b(?:texSample2D|texture2D|texture)\s*\(\s*(g_Texture([0-7]))\s*,/g, '_weSampleBorder($1, $2,');
  }

  const HEADER_DEFINES = [
    '#define mul(x, y) ((y) * (x))',
    '#define lerp mix',
    '#define frac fract',
    '#define CAST2(x) (vec2(x))',
    '#define CAST3(x) (vec3(x))',
    '#define CAST4(x) (vec4(x))',
    '#define CAST3X3(x) (mat3(x))',
    '#define CAST4X4(x) (mat4(x))',
    '#define float2 vec2',
    '#define float3 vec3',
    '#define float4 vec4',
    '#define int2 ivec2',
    '#define int3 ivec3',
    '#define int4 ivec4',
    '#define float2x2 mat2',
    '#define float3x3 mat3',
    '#define float4x4 mat4',
    '#define saturate(x) (clamp(x, 0.0, 1.0))',
    '#define texSample2D texture',
    '#define texSample2DLod textureLod',
    '#define texture2D texture',
    '#define texture2DLod textureLod',
    '#define log10(x) (log2(x) * 0.301029995663981)',
    '#define atan2 atan',
    '#define fmod(x, y) ((x)-(y)*trunc((x)/(y)))',
    '#define ddx dFdx',
    '#define ddy(x) dFdy(-(x))',
    '#define max _wemax',
    '#define min _wemin',
    '#define pow _wepow',
    '#define GLSL 1',
    '#define HLSL 0',
    '',
  ].join('\n');

  // The biased sample exists only in fragment shaders (GLSL ES 3.00 restricts texture bias there).
  const FRAGMENT_DEFINES = 'out vec4 out_FragColor;\n#define varying in\n'
    + 'vec4 _weSampleBorder(sampler2D s, int slot, vec2 uv, float bias) { if (_weOutside(slot, uv)) return vec4(0.0); return texture(s, uv, bias); }\n';
  const VERTEX_DEFINES = '#define attribute in\n#define varying out\n';

  // "1 2 3" and numbers into the vector type a parameter declares.
  function parseVector(value, size) {
    let parts;
    if (typeof value === 'number') parts = [value];
    else if (typeof value === 'string') parts = value.trim().split(/\s+/).filter((s) => s.length).map(Number);
    else if (Array.isArray(value)) parts = value.map(Number);
    else throw new Error('cannot read a vector from ' + JSON.stringify(value));
    if (parts.some((n) => !Number.isFinite(n))) throw new Error('cannot read a vector from ' + JSON.stringify(value));
    const out = new Array(size).fill(0);
    if (parts.length === 1) out.fill(parts[0]);
    else for (let i = 0; i < size; i++) out[i] = parts[i] === undefined ? (i === 3 ? 1 : 0) : parts[i];
    return out;
  }

  function parseScalar(value, integer) {
    let n;
    if (typeof value === 'number') n = value;
    else if (typeof value === 'string') n = Number(value.trim().split(/\s+/)[0]);
    else if (typeof value === 'boolean') n = value ? 1 : 0;
    else throw new Error('cannot read a number from ' + JSON.stringify(value));
    if (!Number.isFinite(n)) throw new Error('cannot read a number from ' + JSON.stringify(value));
    return integer ? Math.trunc(n) : n;
  }

  /**
   * One shader stage. `loader.include(name)` returns the text of `shaders/<name>` (a `.h`
   * file) or throws when it is missing.
   */
  class Unit {
    constructor(type, file, content, options) {
      this.type = type;
      this.file = file;
      this.content = content;
      this.combos = options.combos || {};
      this.overrideCombos = options.overrideCombos || {};
      this.constants = options.constants || {};
      this.passTextures = options.passTextures || {};
      this.overrideTextures = options.overrideTextures || {};
      this.parameters = [];
      this.discoveredCombos = {};
      this.usedCombos = {};
      this.defaultTextures = {};
      this.link = null;
      this.preprocessed = '';
      this.includes = '';
      this.final = '';
    }

    async preprocess(loader) {
      this.preprocessed = this.expandSamplerUniforms(this.content);
      this.includes = '';
      await this.preprocessIncludes(loader);
      this.preprocessRequires();
      this.preprocessVariables();
      this.preprocessed = this.preprocessed.split('gl_FragColor').join('out_FragColor');
    }

    // SAMPLER_UNIFORM(sampler2D, g_Texture0) and SAMPLER_UNIFORM(g_Texture0) declare a sampler
    // uniform; the declaration is spelled out so the annotation comment on the line is read.
    expandSamplerUniforms(source) {
      return source.replace(/\bSAMPLER_UNIFORM\s*\(([^)]*)\)\s*;?/g, (all, args) => {
        const parts = args.split(',').map((s) => s.trim()).filter((s) => s.length);
        if (parts.length === 1) return 'uniform sampler2D ' + parts[0] + ';';
        if (parts.length === 2) return 'uniform ' + parts[0] + ' ' + parts[1] + ';';
        throw new Error('shader ' + this.file + ': SAMPLER_UNIFORM(' + args + ') takes a sampler name, optionally preceded by its type');
      });
    }

    async includeText(loader, filename, seen) {
      if (seen.has(filename)) return '// include of ' + filename + ' skipped: it includes itself\n';
      let body;
      try {
        body = await loader.include(filename);
      } catch (e) {
        return '// tried including file ' + filename + ' but was not found\n';
      }
      const inner = new Set(seen);
      inner.add(filename);
      let text = '// begin of include from file ' + filename + '\n' + body + '\n// end of included from file ' + filename + '\n';
      // Nested includes are expanded in place.
      const re = /^[ \t]*#include[ \t]*"([^"\n]*)"[^\n]*$/m;
      let guard = 0;
      let m;
      while ((m = re.exec(text)) !== null && guard++ < 64) {
        const nested = await this.includeText(loader, m[1], inner);
        text = text.slice(0, m.index) + nested + text.slice(m.index + m[0].length);
      }
      return text;
    }

    async preprocessIncludes(loader) {
      const re = /#include[ \t]*"([^"\n]*)"/g;
      const found = [];
      let m;
      while ((m = re.exec(this.preprocessed)) !== null) found.push({ index: m.index, name: m[1] });
      for (const f of found) {
        this.includes += await this.includeText(loader, f.name, new Set([this.file]));
      }
      // Comment the directives out without moving anything else.
      this.preprocessed = this.preprocessed.replace(/#include([ \t]*"[^"\n]*")/g, '//nclude$1');
      if (!found.length) return;
      this.insertIncludes();
    }

    // Place gathered includes before main(), after the last declaration and outside any
    // preprocessor block that would hide them from other branches.
    insertIncludes() {
      const src = this.preprocessed;
      const mainRe = /(^|[\s;}])main\s*\(/g;
      let m;
      let mainAt = -1;
      while ((m = mainRe.exec(src)) !== null) { mainAt = m.index + m[1].length; break; }
      if (mainAt < 0) throw new Error('Could not find where to place includes for shader unit ' + this.file + ': it has no main()');
      const before = src.slice(0, mainAt);
      const declRe = /\b(attribute|varying|uniform|in|out)\b/g;
      let latest = -1;
      while ((m = declRe.exec(before)) !== null) latest = m.index;
      let insertAt;
      if (latest >= 0) {
        const eol = src.indexOf('\n', latest);
        insertAt = eol < 0 ? src.length : eol;
      } else {
        insertAt = src.lastIndexOf('\n', mainAt);
        if (insertAt < 0) insertAt = 0;
      }
      const lineStart = src.lastIndexOf('\n', mainAt);
      insertAt = Math.min(insertAt, lineStart < 0 ? 0 : lineStart);
      // Move above the outermost open #if block that encloses the insertion point.
      const stack = [];
      const ifRe = /#(if|endif)/g;
      while ((m = ifRe.exec(src)) !== null) {
        if (m.index > insertAt) break;
        if (m[1] === 'if') stack.push(m.index);
        else stack.pop();
      }
      if (stack.length) {
        const open = stack[0];
        const eol = src.lastIndexOf('\n', open);
        insertAt = eol < 0 ? 0 : eol;
      }
      this.preprocessed = src.slice(0, insertAt) + '\n' + this.includes + '\n' + src.slice(insertAt);
    }

    preprocessRequires() {
      const re = /^[ \t]*#require[ \t]+([^\s]+)[^\n]*$/gm;
      this.preprocessed = this.preprocessed.replace(re, (all, module) => {
        const code = this.resolveRequire(module.trim());
        return '//' + all.trim().slice(2) + '\n' + code;
      });
    }

    resolveRequire(name) {
      if (name === 'LightingV1') return LIGHTING_V1;
      throw new Error('shader ' + this.file + ' requires module ' + name + ', which is not one Wallpaper Engine provides');
    }

    preprocessVariables() {
      const lines = this.preprocessed.split('\n');
      for (const line of lines) {
        const combo = line.indexOf('// [COMBO] ');
        if (combo >= 0) {
          this.parseComboConfiguration(line.slice(combo + '// [COMBO] '.length), 0);
          continue;
        }
        const uniform = line.indexOf('uniform ');
        const comment = line.indexOf('// ');
        const semicolon = line.indexOf(';');
        if (uniform >= 0 && comment >= 0 && semicolon >= 0 && semicolon < comment) {
          const decl = line.slice(uniform + 'uniform '.length, semicolon).trim().replace(/\s+/g, ' ');
          const parts = decl.split(' ');
          if (parts.length >= 2) {
            const type = parts[parts.length - 2];
            const name = parts[parts.length - 1];
            this.parseParameterConfiguration(type, name, line.slice(comment + 2));
          }
        }
      }
    }

    parseComboConfiguration(content, defaultValue) {
      let data;
      try { data = JSON.parse(content); } catch (e) { throw new Error('Cannot parse combo metadata in shader ' + this.file + ': ' + content); }
      if (typeof data.combo !== 'string') throw new Error('Combo metadata without a combo name in shader ' + this.file + ': ' + content);
      const combo = data.combo;
      this.usedCombos[combo] = true;
      if (combo in this.combos || combo in this.overrideCombos) return;
      if (!('default' in data)) { this.discoveredCombos[combo] = defaultValue; return; }
      const d = data.default;
      if (typeof d === 'number' && Number.isInteger(d)) this.discoveredCombos[combo] = d;
      else if (typeof d === 'boolean') this.discoveredCombos[combo] = d ? 1 : 0;
      else if (typeof d === 'string' && /^-?\d+$/.test(d.trim())) this.discoveredCombos[combo] = parseInt(d, 10);
      else throw new Error('combo ' + combo + ' in shader ' + this.file + ' has a default of an unsupported kind: ' + JSON.stringify(d));
    }

    parseParameterConfiguration(type, name, content) {
      let data;
      try { data = JSON.parse(content); } catch (e) { return; }
      if (!data || typeof data !== 'object') return;
      const material = data.material;
      const hasDefault = 'default' in data;
      const constant = material !== undefined ? this.constants[material] : undefined;
      if (constant === undefined && !hasDefault && type !== 'sampler2D' && type !== 'sampler2DComparison') {
        throw new Error('Cannot parse parameter data for ' + name + ' in shader ' + this.file + ': no default and no material value');
      }
      let parameter = null;
      const arrayMatch = /^([A-Za-z_][A-Za-z0-9_]*)\[(\d+)\]$/.exec(name);
      const baseName = arrayMatch ? arrayMatch[1] : name;
      const dflt = hasDefault ? data.default : (constant !== undefined ? constant : null);
      switch (type) {
        case 'vec4': parameter = { type: 'vec4', value: parseVector(dflt, 4) }; break;
        case 'vec3': parameter = { type: 'vec3', value: parseVector(dflt, 3) }; break;
        case 'vec2': parameter = { type: 'vec2', value: parseVector(dflt, 2) }; break;
        case 'float': parameter = { type: 'float', value: parseScalar(dflt, false) }; break;
        case 'int': parameter = { type: 'int', value: parseScalar(dflt, true) }; break;
        case 'bool': parameter = { type: 'int', value: parseScalar(dflt, true) ? 1 : 0 }; break;
        case 'sampler2D':
        case 'sampler2DComparison': {
          const im = /g_Texture(\d+)/.exec(baseName);
          if (!im) return;
          const index = parseInt(im[1], 10);
          const combo = data.combo;
          if (typeof combo === 'string') {
            const slotUsed = (index in this.passTextures) || (index in this.overrideTextures);
            let required = false;
            let comboValue = 1;
            if (slotUsed) {
              required = true;
            } else if (data.require && typeof data.require === 'object') {
              if (data.requireany) {
                for (const [macro, value] of Object.entries(data.require)) {
                  const have = this.combos[macro];
                  if (have === undefined || (macro in this.overrideCombos) || have !== value) { required = true; break; }
                }
              } else {
                required = true;
                for (const [macro, value] of Object.entries(data.require)) {
                  const have = macro in this.overrideCombos ? this.overrideCombos[macro] : this.combos[macro];
                  if (have !== undefined && have === value) { required = false; break; }
                }
              }
            }
            if (required && !slotUsed) {
              if (!hasDefault) required = false;
              else if (combo in this.combos || combo in this.overrideCombos) required = false;
              else if (typeof data.default === 'string' && /^-?\d+$/.test(data.default.trim())) comboValue = parseInt(data.default, 10);
              else if (typeof data.default === 'number') comboValue = data.default;
              else throw new Error('Cannot determine default value for combo ' + combo + ' because it is not specified by the shader and is not given a default value: ' + this.file);
            }
            if (required) {
              this.discoveredCombos[combo] = comboValue;
              this.usedCombos[combo] = true;
            }
          }
          if (typeof data.default === 'string' && data.default.length) this.defaultTextures[index] = data.default;
          return;
        }
        default:
          return;
      }
      if (material !== undefined && parameter) {
        parameter.identifier = material;
        parameter.name = baseName;
        parameter.count = arrayMatch ? parseInt(arrayMatch[2], 10) : 1;
        this.parameters.push(parameter);
      }
    }

    applyLinkedVaryingCompatibility(source) {
      if (this.type !== VERTEX || !this.link) return source;
      const linked = this.link.preprocessed;
      const fragVec4 = /\bvarying\s+vec4\s+([A-Za-z_][A-Za-z0-9_]*)\s*;/g;
      let m;
      while ((m = fragVec4.exec(linked)) !== null) {
        const name = m[1];
        const decl = new RegExp('\\bvarying\\s+vec2\\s+' + name + '\\s*;');
        if (!decl.test(source)) continue;
        source = source.replace(decl, 'varying vec4 ' + name + ';');
        const assign = new RegExp('(^|\\n)([ \\t]*)' + name + '\\s*=\\s*([^;\\n]+);', 'g');
        source = source.replace(assign, (all, prefix, indent, expr) => prefix + indent + name + ' = vec4(' + expr + ', 0.0, 1.0);');
      }
      return source;
    }

    applyFragmentTexCoordCompatibility(source) {
      if (this.type !== FRAGMENT) return source;
      const before = /\bv_TexCoord\b(\s*[-+*/]\s*CAST2\s*\()/g;
      const after = /(CAST2\s*\([^)]+\)\s*[-+*/]\s*)\bv_TexCoord\b/g;
      if (!/\bvarying\s+vec[34]\s+v_TexCoord\s*;/.test(source) || (!before.test(source) && !after.test(source))) return source;
      return source.replace(before, 'v_TexCoord.xy$1').replace(after, '$1v_TexCoord.xy');
    }

    compile() {
      if (this.final) return this.final;
      let out = '#version 300 es\n';
      out += '// ======================================================\n';
      out += '// Processed shader ' + this.file + '\n';
      out += '// ======================================================\n';
      out += 'precision highp float;\nprecision highp int;\n';
      out += OVERLOADS;
      out += HEADER_DEFINES;
      out += this.type === FRAGMENT ? FRAGMENT_DEFINES : VERTEX_DEFINES;
      const added = new Set();
      const define = (name, value) => {
        const upper = name.toUpperCase();
        if (added.has(upper)) return;
        added.add(upper);
        out += '#define ' + upper + ' ' + value + '\n';
      };
      for (const [k, v] of Object.entries(this.overrideCombos)) define(k, v);
      for (const [k, v] of Object.entries(this.combos)) define(k, v);
      for (const [k, v] of Object.entries(this.discoveredCombos)) define(k, v);
      if (this.link) {
        for (const [k, v] of Object.entries(this.link.combos)) define(k, v);
        for (const [k, v] of Object.entries(this.link.discoveredCombos)) define(k, v);
      }
      const body = routeBorderSamples(this.applyFragmentTexCoordCompatibility(this.applyLinkedVaryingCompatibility(this.preprocessed)));
      out += undefinedConditionMacros(out, body).map((name) => '#define ' + name + ' 0\n').join('');
      out += body;
      this.final = out;
      return out;
    }
  }

  const PREDEFINED_MACROS = new Set(['__VERSION__', '__LINE__', '__FILE__', 'GL_ES', 'defined']);

  // GLSL ES rejects an undefined identifier inside #if / #elif where desktop GLSL reads 0, so
  // every such identifier that neither a combo nor a #define supplies is defined as 0 -- unless
  // the source also asks whether it is defined at all (#ifdef, #ifndef, defined()).
  function undefinedConditionMacros(header, body) {
    const defined = new Set(PREDEFINED_MACROS);
    const defineRe = /^[ \t]*#[ \t]*define[ \t]+([A-Za-z_][A-Za-z0-9_]*)/gm;
    let m;
    for (const text of [header, body]) while ((m = defineRe.exec(text)) !== null) defined.add(m[1]);
    const tested = new Set();
    const ifdefRe = /^[ \t]*#[ \t]*(?:ifdef|ifndef)[ \t]+([A-Za-z_][A-Za-z0-9_]*)/gm;
    while ((m = ifdefRe.exec(body)) !== null) tested.add(m[1]);
    const definedRe = /\bdefined[ \t]*\(?[ \t]*([A-Za-z_][A-Za-z0-9_]*)/g;
    while ((m = definedRe.exec(body)) !== null) tested.add(m[1]);
    const out = [];
    const seen = new Set();
    const condRe = /^[ \t]*#[ \t]*(?:if|elif)[ \t]+([^\n]*)$/gm;
    while ((m = condRe.exec(body)) !== null) {
      const expr = m[1].replace(/\/\/.*$/, '').replace(/\bdefined[ \t]*\(?[ \t]*[A-Za-z_][A-Za-z0-9_]*[ \t]*\)?/g, ' ');
      const idRe = /(?:^|[^A-Za-z0-9_.])([A-Za-z_][A-Za-z0-9_]*)/g;
      let id;
      while ((id = idRe.exec(expr)) !== null) {
        const name = id[1];
        if (defined.has(name) || tested.has(name) || seen.has(name)) continue;
        seen.add(name);
        out.push(name);
      }
    }
    return out;
  }

  const LIGHTING_V1 = `// begin of generated module LightingV1
#ifndef WE_LIGHT_COUNT
#define WE_LIGHT_COUNT 4
#endif
uniform vec3 g_LightsPosition[WE_LIGHT_COUNT];
uniform vec3 g_LightsColorPremultiplied[WE_LIGHT_COUNT];
uniform float g_LightsRadius[WE_LIGHT_COUNT];
uniform vec3 g_LightAmbientColor;
uniform vec3 g_LightSkylightColor;
float _weDistributionGGX(float NdotH, float roughness) {
    float a = roughness * roughness;
    float a2 = a * a;
    float d = NdotH * NdotH * (a2 - 1.0) + 1.0;
    return a2 / (3.14159265 * d * d + 0.0001);
}
float _weGeometrySchlick(float NdotV, float roughness) {
    float k = (roughness + 1.0) * (roughness + 1.0) / 8.0;
    return NdotV / (NdotV * (1.0 - k) + k + 0.0001);
}
vec3 PerformLighting_V1(vec3 worldPos, vec3 albedo, vec3 normal, vec3 viewDir,
    vec3 specularTint, vec3 baseReflectance, float roughness, float metallic)
{
    vec3 N = normalize(normal);
    vec3 V = normalize(viewDir);
    vec3 F0 = mix(baseReflectance, albedo, metallic);
    vec3 result = vec3(0.0);
    for (int i = 0; i < WE_LIGHT_COUNT; i++) {
        vec3 toLight = g_LightsPosition[i] - worldPos;
        float dist = length(toLight);
        if (g_LightsRadius[i] <= 0.0) continue;
        vec3 L = toLight / (dist + 0.0001);
        float attenuation = clamp(1.0 - (dist * dist) / (g_LightsRadius[i] * g_LightsRadius[i]), 0.0, 1.0);
        attenuation *= attenuation;
        vec3 radiance = g_LightsColorPremultiplied[i] * attenuation;
        vec3 H = normalize(L + V);
        float NdotL = clamp(dot(N, L), 0.0, 1.0);
        float NdotV = clamp(dot(N, V), 0.0, 1.0);
        float NdotH = clamp(dot(N, H), 0.0, 1.0);
        float HdotV = clamp(dot(H, V), 0.0, 1.0);
        vec3 F = F0 + (1.0 - F0) * pow(1.0 - HdotV, 5.0);
        float D = _weDistributionGGX(NdotH, roughness);
        float Gs = _weGeometrySchlick(NdotV, roughness) * _weGeometrySchlick(NdotL, roughness);
        vec3 specular = (D * Gs * F) / (4.0 * NdotV * NdotL + 0.0001) * specularTint;
        vec3 kD = (vec3(1.0) - F) * (1.0 - metallic);
        result += (kD * albedo / 3.14159265 + specular) * radiance * NdotL;
    }
    result += albedo * mix(g_LightAmbientColor, g_LightSkylightColor, clamp(N.y * 0.5 + 0.5, 0.0, 1.0));
    return result;
}
// end of generated module LightingV1
`;

  /** A vertex + fragment pair with linked combos. */
  class Shader {
    static async load(loader, filename, options) {
      const [vs, fs] = await Promise.all([loader.vertexShader(filename), loader.fragmentShader(filename)]);
      const shader = new Shader();
      shader.file = filename;
      shader.vertex = new Unit(VERTEX, filename, vs, options);
      shader.fragment = new Unit(FRAGMENT, filename, fs, options);
      shader.vertex.link = shader.fragment;
      shader.fragment.link = shader.vertex;
      await shader.vertex.preprocess(loader);
      await shader.fragment.preprocess(loader);
      shader.combos = options.combos || {};
      return shader;
    }

    vertexSource() { return this.vertex.compile(); }
    fragmentSource() { return this.fragment.compile(); }

    findParameter(identifier) {
      const v = this.vertex.parameters.find((p) => p.identifier === identifier) || null;
      const f = this.fragment.parameters.find((p) => p.identifier === identifier) || null;
      return { vertex: v, fragment: f };
    }
  }

  function numbered(src) {
    return src.split('\n').map((l, i) => String(i + 1).padStart(4, ' ') + ': ' + l).join('\n');
  }

  function compileStage(gl, type, source, label) {
    const sh = gl.createShader(type);
    gl.shaderSource(sh, source);
    gl.compileShader(sh);
    if (!gl.getShaderParameter(sh, gl.COMPILE_STATUS)) {
      const log = gl.getShaderInfoLog(sh) || '';
      gl.deleteShader(sh);
      throw new Error('shader ' + label + ' failed to compile:\n' + log + '\nTranslated source:\n' + numbered(source));
    }
    return sh;
  }

  /** Compile and link, returning the program with its uniform and attribute reflection. */
  function buildProgram(gl, shader) {
    const vsSrc = shader.vertexSource();
    const fsSrc = shader.fragmentSource();
    const vs = compileStage(gl, gl.VERTEX_SHADER, vsSrc, shader.file + '.vert');
    const fs = compileStage(gl, gl.FRAGMENT_SHADER, fsSrc, shader.file + '.frag');
    const program = gl.createProgram();
    gl.attachShader(program, vs);
    gl.attachShader(program, fs);
    gl.linkProgram(program);
    gl.deleteShader(vs);
    gl.deleteShader(fs);
    if (!gl.getProgramParameter(program, gl.LINK_STATUS)) {
      const log = gl.getProgramInfoLog(program) || '';
      gl.deleteProgram(program);
      throw new Error('shader ' + shader.file + ' failed to link:\n' + log + '\nVertex source:\n' + numbered(vsSrc) + '\nFragment source:\n' + numbered(fsSrc));
    }
    const uniforms = {};
    const count = gl.getProgramParameter(program, gl.ACTIVE_UNIFORMS);
    for (let i = 0; i < count; i++) {
      const info = gl.getActiveUniform(program, i);
      if (!info) continue;
      const name = info.name.replace(/\[0\]$/, '');
      uniforms[name] = { location: gl.getUniformLocation(program, info.name), type: info.type, size: info.size };
    }
    const attributes = {};
    const acount = gl.getProgramParameter(program, gl.ACTIVE_ATTRIBUTES);
    for (let i = 0; i < acount; i++) {
      const info = gl.getActiveAttrib(program, i);
      if (!info) continue;
      attributes[info.name] = { location: gl.getAttribLocation(program, info.name), type: info.type, size: info.size };
    }
    return { program, uniforms, attributes };
  }

  const api = { VERTEX, FRAGMENT, Unit, Shader, buildProgram, parseVector, parseScalar, OVERLOADS, HEADER_DEFINES, undefinedConditionMacros, routeBorderSamples };
  G.WEShader = api;
  if (typeof module !== 'undefined' && module.exports) module.exports = api;
})();
