/*
    SPDX-License-Identifier: MIT

    Colour adjustment chain shared by the video and image renderers, driven by
    the mpv property names of Assets/Plugins/Mpv/LivelyProperties.json
    (saturation, hue, brightness, contrast, gamma; all -100..100, default 0).

    brightness/contrast/saturation: MultiEffect (mpv -100..100 -> -1..1).
    hue/gamma: ShaderEffect (contents/shaders/adjust.frag, compiled with qsb):
    hue rotates the chroma by hue/100 * pi radians, gamma applies
    pow(c, 1 / 8^(gamma/100)), both like mpv's video equalizer.

    While every value is 0 the chain is inactive and the source item draws
    itself directly.
*/

import QtQuick
import QtQuick.Effects

Item {
    id: layer

    required property Item sourceItem

    property real brightness: 0
    property real contrast: 0
    property real saturation: 0
    property real hue: 0
    property real gamma: 0

    readonly property bool colorActive: brightness !== 0 || contrast !== 0 || saturation !== 0
    readonly property bool shaderActive: hue !== 0 || gamma !== 0
    readonly property bool active: colorActive || shaderActive

    function reset() {
        brightness = 0
        contrast = 0
        saturation = 0
        hue = 0
        gamma = 0
    }

    // Applies an lp_slider by mpv property name; false when the name is not an adjustment.
    function apply(name, value) {
        var clamped = Math.max(-100, Math.min(100, value))
        switch (name) {
        case "brightness": brightness = clamped; return true
        case "contrast": contrast = clamped; return true
        case "saturation": saturation = clamped; return true
        case "hue": hue = clamped; return true
        case "gamma": gamma = clamped; return true
        default: return false
        }
    }

    // Both effects keep their source assigned permanently (unassigning it makes the effect
    // complain about a missing texture provider); an invisible effect is not rendered, so
    // the inactive chain costs nothing per frame.
    MultiEffect {
        id: colorEffect
        anchors.fill: parent
        visible: layer.active && !layer.shaderActive
        source: layer.sourceItem
        brightness: layer.brightness / 100
        contrast: layer.contrast / 100
        saturation: layer.saturation / 100
    }

    ShaderEffect {
        id: hueGammaEffect
        anchors.fill: parent
        visible: layer.shaderActive
        property var source: ShaderEffectSource {
            sourceItem: colorEffect
            live: layer.shaderActive
        }
        property real hueAngle: layer.hue / 100 * Math.PI
        property real gammaPower: Math.pow(8, -layer.gamma / 100)
        fragmentShader: "../shaders/adjust.frag.qsb"
    }
}
