/*
    SPDX-License-Identifier: MIT

    Renderer for Kind picture. Still images take the same mpv property set as
    video (Lively drives pictures through the mpv player); speed and mute are
    accepted with no effect because a still image has no time axis or audio.
*/

import QtQuick
import QtQuick.Window

Item {
    id: renderer
    clip: true

    property string source: ""
    property string scaler: "uniformFill"

    signal loaded(success: bool)
    signal consoleMessage(message: string, category: int)

    LivelyUtil {
        id: util
    }

    property string scalerOverride: ""
    readonly property string activeScaler: scalerOverride !== "" ? scalerOverride : scaler
    property bool loadReported: false

    function reload() {
        loadReported = false
        image.source = ""
        image.source = Qt.binding(function () { return util.localFileUrl(renderer.source) })
    }

    function resetToDefaults() {
        scalerOverride = ""
        adjust.reset()
    }

    function handleMessage(message) {
        switch (message.Type) {
        case 12: { // lp_slider
            var name = String(message.Name)
            var value = Number(message.Value)
            if (isNaN(value))
                consoleMessage("Slider '" + name + "' has a non-numeric value", 1)
            else if (name !== "speed" && !adjust.apply(name, value))
                consoleMessage("Unsupported mpv property '" + name + "': the Plasma plugin honours saturation, hue, brightness, contrast, gamma, speed, mute and scaler", 1)
            break
        }
        case 18: // lp_chekbox
            if (message.Name !== "mute")
                consoleMessage("Unsupported mpv property '" + message.Name + "': the only checkbox honoured is mute", 1)
            break
        case 19: { // lp_dropdown_scaler
            var scalerName = util.scalerFromIndex(Number(message.Value))
            if (scalerName === "")
                consoleMessage("Scaler index " + message.Value + " is out of range (0 none, 1 fill, 2 uniform, 3 uniformFill)", 1)
            else
                scalerOverride = scalerName
            break
        }
        case 16: // lp_button
            if (message.IsDefault === true)
                resetToDefaults()
            break
        case 100: // host_mpv_command: a still image has no time axis and no audio.
            consoleMessage("mpv command " + JSON.stringify(message.Command) + " has no effect on a still image", 0)
            break
        default:
            break
        }
    }

    Image {
        id: image
        anchors.centerIn: parent
        source: util.localFileUrl(renderer.source)
        asynchronous: true
        cache: false
        smooth: true
        // none: native resolution (one image pixel per device pixel), centered, cropped by clip.
        width: renderer.activeScaler === "none" && status === Image.Ready
               ? sourceSize.width / Screen.devicePixelRatio : renderer.width
        height: renderer.activeScaler === "none" && status === Image.Ready
                ? sourceSize.height / Screen.devicePixelRatio : renderer.height
        fillMode: renderer.activeScaler === "uniform" ? Image.PreserveAspectFit
                : renderer.activeScaler === "uniformFill" ? Image.PreserveAspectCrop
                : Image.Stretch
        visible: !adjust.active

        onStatusChanged: {
            if (renderer.loadReported)
                return
            if (status === Image.Ready) {
                renderer.loadReported = true
                renderer.loaded(true)
            } else if (status === Image.Error) {
                renderer.loadReported = true
                renderer.consoleMessage("Image could not be loaded: " + source, 1)
                renderer.loaded(false)
            }
        }
    }

    AdjustLayer {
        id: adjust
        anchors.fill: image
        sourceItem: image
    }
}
