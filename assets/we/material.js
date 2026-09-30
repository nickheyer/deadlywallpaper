// Wallpaper Engine materials, effects and models as scene files declare them. Port of
// Data/Parsers/MaterialParser.cpp, EffectParser.cpp, ModelParser.cpp, ShaderConstantParser.cpp
// and TextureParser::parseTextureMap.
(() => {
  'use strict';
  const G = typeof window !== 'undefined' ? window : globalThis;

  // Data/Model/Material.h enumerations
  const BLENDING = { NORMAL: 1, TRANSLUCENT: 2, ADDITIVE: 3 };
  const CULL = { NORMAL: 1, DISABLE: 2 };
  const DEPTHTEST = { DISABLED: 1, ENABLED: 2 };
  const DEPTHWRITE = { DISABLED: 1, ENABLED: 2 };
  // Data/Model/Effect.h PassCommandType
  const COMMAND = { COPY: 0, SWAP: 1 };

  // MaterialParser::parseBlendMode
  function parseBlendMode(mode, where) {
    switch (mode) {
      case 'normal': return BLENDING.NORMAL;
      case 'additive': return BLENDING.ADDITIVE;
      case 'translucent': return BLENDING.TRANSLUCENT;
      default: throw new Error(where + ': unknown blending mode "' + mode + '"');
    }
  }
  // MaterialParser::parseCullMode
  function parseCullMode(mode, where) {
    switch (mode) {
      case 'nocull': return CULL.DISABLE;
      case 'normal': return CULL.NORMAL;
      default: throw new Error(where + ': unknown culling mode "' + mode + '"');
    }
  }
  // MaterialParser::parseDepthtestMode
  function parseDepthtestMode(mode, where) {
    switch (mode) {
      case 'disabled': return DEPTHTEST.DISABLED;
      case 'enabled': return DEPTHTEST.ENABLED;
      default: throw new Error(where + ': unknown depthtest mode "' + mode + '"');
    }
  }
  // MaterialParser::parseDepthwriteMode
  function parseDepthwriteMode(mode, where) {
    switch (mode) {
      case 'disabled': return DEPTHWRITE.DISABLED;
      case 'enabled': return DEPTHWRITE.ENABLED;
      default: throw new Error(where + ': unknown depthwrite mode "' + mode + '"');
    }
  }

  // TextureParser::parseTextureMap: an array indexed by texture slot; null keeps the slot free,
  // strings name a texture, objects carry "name" plus an optional "user" property binding whose
  // value replaces the texture. Returns { names: {slot: name}, users: {slot: propertyName} }.
  function parseTextureMap(json, where) {
    const names = {}, users = {};
    if (!Array.isArray(json)) return { names, users };
    json.forEach((cur, index) => {
      if (cur === null || cur === undefined) return;
      if (typeof cur === 'string') {
        if (cur.length) names[index] = cur;
        return;
      }
      if (typeof cur === 'object') {
        const name = typeof cur.name === 'string' ? cur.name : (typeof cur.value === 'string' ? cur.value : null);
        if (name && name.length) names[index] = name;
        const user = cur.user;
        if (typeof user === 'string') users[index] = user;
        else if (user && typeof user === 'object' && typeof user.name === 'string') users[index] = user.name;
        else if (user !== undefined && user !== null) throw new Error(where + ': texture slot ' + index + ' has an unreadable user binding ' + JSON.stringify(user));
        return;
      }
      throw new Error(where + ': texture slot ' + index + ' is neither a name nor an object (' + JSON.stringify(cur) + ')');
    });
    return { names, users };
  }

  // MaterialParser::parseCombos / ObjectParser::parseComboMap: {"NAME": int}
  function parseCombos(json, where) {
    const result = {};
    if (!json || typeof json !== 'object' || Array.isArray(json)) return result;
    for (const [key, value] of Object.entries(json)) {
      if (typeof value === 'number' && Number.isFinite(value)) result[key] = Math.trunc(value);
      else if (typeof value === 'boolean') result[key] = value ? 1 : 0;
      else if (typeof value === 'string' && /^-?\d+$/.test(value.trim())) result[key] = parseInt(value, 10);
      else throw new Error(where + ': combo ' + key + ' has a non-integer value ' + JSON.stringify(value));
    }
    return result;
  }

  // ShaderConstantParser::parse: every constant is a user setting (literal, {"user":...} binding
  // or script/animation source) so property changes reach the shader live.
  function parseConstants(json, ctx, where) {
    const result = {};
    if (!json || typeof json !== 'object' || Array.isArray(json)) return result;
    for (const [key, value] of Object.entries(json)) {
      result[key] = G.WEProps.setting(value, { where: where + '.' + key, properties: ctx.properties });
    }
    return result;
  }

  // MaterialParser::parsePass
  function parsePass(json, ctx, where) {
    if (!json || typeof json !== 'object') throw new Error(where + ': material pass is not an object');
    if (typeof json.shader !== 'string' || !json.shader.length) throw new Error(where + ': material pass must have a shader');
    const textures = parseTextureMap(json.textures, where + '.textures');
    const usertextures = parseTextureMap(json.usertextures, where + '.usertextures');
    return {
      blending: parseBlendMode(json.blending === undefined ? 'normal' : json.blending, where),
      cullmode: parseCullMode(json.cullmode === undefined ? 'nocull' : json.cullmode, where),
      depthtest: parseDepthtestMode(json.depthtest === undefined ? 'disabled' : json.depthtest, where),
      depthwrite: parseDepthwriteMode(json.depthwrite === undefined ? 'disabled' : json.depthwrite, where),
      shader: json.shader,
      textures: textures.names,
      usertextures: usertextures.names,
      textureUsers: Object.assign({}, textures.users, usertextures.users),
      combos: parseCombos(json.combos, where),
      constants: parseConstants(json.constantshadervalues, ctx, where + '.constantshadervalues'),
    };
  }

  // MaterialParser::parse
  function parseMaterial(json, filename, ctx) {
    if (!json || typeof json !== 'object') throw new Error(filename + ': material is not an object');
    if (!Array.isArray(json.passes)) throw new Error(filename + ': material must have passes to render');
    return {
      filename,
      passes: json.passes.map((p, i) => parsePass(p, ctx, filename + ' pass ' + i)),
    };
  }

  // MaterialParser::load
  async function loadMaterial(ctx, filename) {
    return parseMaterial(await ctx.loader.json(filename), filename, ctx);
  }

  // EffectParser::parseBinds: [{"index": n, "name": fbo}]
  function parseBinds(json, where) {
    const result = {};
    if (!Array.isArray(json)) return result;
    for (const cur of json) {
      if (!cur || typeof cur !== 'object') throw new Error(where + ': texture bind is not an object');
      if (typeof cur.index !== 'number') throw new Error(where + ': texture binds must have an index');
      if (typeof cur.name !== 'string') throw new Error(where + ': texture bind must name the FBO that should be used');
      result[cur.index] = cur.name;
    }
    return result;
  }

  // EffectParser::parseFBOs
  function parseFBOs(json, where) {
    if (!Array.isArray(json)) return [];
    return json.map((cur, i) => {
      if (!cur || typeof cur !== 'object' || typeof cur.name !== 'string') throw new Error(where + ': FBO ' + i + ' must have a name');
      return {
        name: cur.name,
        format: typeof cur.format === 'string' ? cur.format : 'rgba8888',
        scale: typeof cur.scale === 'number' ? cur.scale : 1,
      };
    });
  }

  // EffectParser::parseEffectPasses (materials are loaded here so an effect is complete once parsed)
  async function parseEffectPasses(json, ctx, where) {
    if (!Array.isArray(json)) return [];
    const result = [];
    for (let i = 0; i < json.length; i++) {
      const cur = json[i];
      const passWhere = where + ' pass ' + i;
      if (!cur || typeof cur !== 'object') throw new Error(passWhere + ': effect pass is not an object');
      const hasCommand = cur.command !== undefined;
      let command = null;
      if (hasCommand) {
        if (cur.command === 'copy') command = COMMAND.COPY;
        else if (cur.command === 'swap') command = COMMAND.SWAP;
        else throw new Error(passWhere + ': unknown effect command "' + cur.command + '"');
        if (typeof cur.source !== 'string') throw new Error(passWhere + ': effect command must have a source');
        if (typeof cur.target !== 'string') throw new Error(passWhere + ': effect command must have a target');
      }
      result.push({
        material: typeof cur.material === 'string' ? await loadMaterial(ctx, cur.material) : null,
        binds: parseBinds(cur.bind, passWhere),
        command,
        source: typeof cur.source === 'string' ? cur.source : null,
        target: typeof cur.target === 'string' ? cur.target : null,
      });
    }
    return result;
  }

  // EffectParser::parse
  async function parseEffect(json, filename, ctx) {
    if (!json || typeof json !== 'object') throw new Error(filename + ': effect is not an object');
    if (!('passes' in json)) throw new Error(filename + ': effect file must have passes');
    return {
      filename,
      name: typeof json.name === 'string' ? json.name : '',
      passes: await parseEffectPasses(json.passes, ctx, filename),
      fbos: parseFBOs(json.fbos, filename),
    };
  }

  // EffectParser::load
  async function loadEffect(ctx, filename) {
    return parseEffect(await ctx.loader.json(filename), filename, ctx);
  }

  // ModelParser::parse. `material` may be a file name or an inline material object.
  async function parseModel(json, filename, ctx) {
    if (!json || typeof json !== 'object') throw new Error(filename + ': model is not an object');
    let material;
    if (typeof json.material === 'string') material = await loadMaterial(ctx, json.material);
    else if (json.material && typeof json.material === 'object') material = parseMaterial(json.material, filename + ' (inline material)', ctx);
    else throw new Error(filename + ': model must have a material');
    const optionalInt = (key) => (typeof json[key] === 'number' ? Math.trunc(json[key]) : null);
    return {
      filename,
      material,
      solidlayer: !!json.solidlayer,
      fullscreen: !!json.fullscreen,
      passthrough: !!json.passthrough,
      autosize: !!json.autosize,
      nopadding: !!json.nopadding,
      width: optionalInt('width'),
      height: optionalInt('height'),
      puppet: typeof json.puppet === 'string' ? json.puppet : null,
      animations: Array.isArray(json.animations) ? json.animations : [],
    };
  }

  // ModelParser::load
  async function loadModel(ctx, filename) {
    return parseModel(await ctx.loader.json(filename), filename, ctx);
  }

  const api = {
    BLENDING, CULL, DEPTHTEST, DEPTHWRITE, COMMAND,
    parseBlendMode, parseCullMode, parseDepthtestMode, parseDepthwriteMode,
    parseTextureMap, parseCombos, parseConstants, parsePass, parseMaterial, loadMaterial,
    parseBinds, parseFBOs, parseEffect, loadEffect, parseModel, loadModel,
  };
  G.WEMaterial = api;
  if (typeof module !== 'undefined' && module.exports) module.exports = api;
})();
