//! Page assets built into the binary and served under `/__deadlywp/`.

/// The mime type and bytes of a built-in page asset.
pub fn get(name: &str) -> Option<(&'static str, &'static [u8])> {
    const ASSETS: &[(&str, &str, &[u8])] = &[
        (
            "scene.html",
            "text/html; charset=utf-8",
            include_bytes!("../../assets/we/scene.html"),
        ),
        (
            "lz4.js",
            "text/javascript; charset=utf-8",
            include_bytes!("../../assets/we/lz4.js"),
        ),
        (
            "wemath.js",
            "text/javascript; charset=utf-8",
            include_bytes!("../../assets/we/wemath.js"),
        ),
        (
            "tex.js",
            "text/javascript; charset=utf-8",
            include_bytes!("../../assets/we/tex.js"),
        ),
        (
            "props.js",
            "text/javascript; charset=utf-8",
            include_bytes!("../../assets/we/props.js"),
        ),
        (
            "shader.js",
            "text/javascript; charset=utf-8",
            include_bytes!("../../assets/we/shader.js"),
        ),
        (
            "loader.js",
            "text/javascript; charset=utf-8",
            include_bytes!("../../assets/we/loader.js"),
        ),
        (
            "fbo.js",
            "text/javascript; charset=utf-8",
            include_bytes!("../../assets/we/fbo.js"),
        ),
        (
            "material.js",
            "text/javascript; charset=utf-8",
            include_bytes!("../../assets/we/material.js"),
        ),
        (
            "textures.js",
            "text/javascript; charset=utf-8",
            include_bytes!("../../assets/we/textures.js"),
        ),
        (
            "pass.js",
            "text/javascript; charset=utf-8",
            include_bytes!("../../assets/we/pass.js"),
        ),
        (
            "camera.js",
            "text/javascript; charset=utf-8",
            include_bytes!("../../assets/we/camera.js"),
        ),
        (
            "objects.js",
            "text/javascript; charset=utf-8",
            include_bytes!("../../assets/we/objects.js"),
        ),
        (
            "image.js",
            "text/javascript; charset=utf-8",
            include_bytes!("../../assets/we/image.js"),
        ),
        (
            "noise.js",
            "text/javascript; charset=utf-8",
            include_bytes!("../../assets/we/noise.js"),
        ),
        (
            "particles.js",
            "text/javascript; charset=utf-8",
            include_bytes!("../../assets/we/particles.js"),
        ),
        (
            "particle.js",
            "text/javascript; charset=utf-8",
            include_bytes!("../../assets/we/particle.js"),
        ),
        (
            "text.js",
            "text/javascript; charset=utf-8",
            include_bytes!("../../assets/we/text.js"),
        ),
        (
            "textlayer.js",
            "text/javascript; charset=utf-8",
            include_bytes!("../../assets/we/textlayer.js"),
        ),
        (
            "puppet.js",
            "text/javascript; charset=utf-8",
            include_bytes!("../../assets/we/puppet.js"),
        ),
        (
            "puppetwarp.js",
            "text/javascript; charset=utf-8",
            include_bytes!("../../assets/we/puppetwarp.js"),
        ),
        (
            "script.js",
            "text/javascript; charset=utf-8",
            include_bytes!("../../assets/we/script.js"),
        ),
        (
            "scriptlayers.js",
            "text/javascript; charset=utf-8",
            include_bytes!("../../assets/we/scriptlayers.js"),
        ),
        (
            "scene.js",
            "text/javascript; charset=utf-8",
            include_bytes!("../../assets/we/scene.js"),
        ),
        (
            "scripting.js",
            "text/javascript; charset=utf-8",
            include_bytes!("../../assets/we/scripting.js"),
        ),
    ];
    ASSETS
        .iter()
        .find(|(n, _, _)| *n == name)
        .map(|(_, mime, body)| (*mime, *body))
}
