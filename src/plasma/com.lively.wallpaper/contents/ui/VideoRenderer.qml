/*
    SPDX-License-Identifier: MIT

    Renderer for Kind video, gif and videostream: MediaPlayer + VideoOutput +
    AudioOutput, looping forever, with the mpv property set of
    Assets/Plugins/Mpv/LivelyProperties.json applied through lp_* messages and
    the host_mpv_command subset (seek, set_property) the core uses for playback
    control.
*/

import QtQuick
import QtQuick.Window
import QtMultimedia

Item {
    id: renderer
    clip: true

    property string source: ""
    property string kind: "video"
    property string scaler: "uniformFill"
    property int volume: 0
    property bool suspended: false

    signal loaded(success: bool)
    signal consoleMessage(message: string, category: int)
    // set_property volume: the volume is owned by LivelyContent (cmd_volume sets the same value).
    signal volumeRequested(volume: int)

    LivelyUtil {
        id: util
    }

    // mpv properties driven by lp_slider / lp_chekbox / lp_dropdown_scaler / set_property.
    property real speed: 1
    property bool mute: false
    property string scalerOverride: ""
    // mpv "pause" (set_property), independent of cmd_suspend; "aid" "no" disables the audio track.
    property bool userPaused: false
    property bool audioTrackEnabled: true
    readonly property bool playing: !suspended && !userPaused
    readonly property string activeScaler: scalerOverride !== "" ? scalerOverride : scaler
    property bool loadReported: false

    // videostream plays the URL directly; files are opened through file://.
    readonly property string mediaUrl: kind === "videostream" ? source : util.localFileUrl(source)

    Component.onCompleted: {
        if (playing)
            player.play()
    }
    onPlayingChanged: {
        if (playing)
            player.play()
        else
            player.pause()
    }

    // Tearing the old media down emits LoadedMedia and NoMedia synchronously before
    // sourceChanged(""); reporting stays suppressed until onSourceChanged re-arms it.
    function reload() {
        loadReported = true
        player.source = ""
        player.source = Qt.binding(function () { return renderer.mediaUrl })
    }

    function resetToDefaults() {
        speed = 1
        mute = false
        scalerOverride = ""
        adjust.reset()
    }

    function applySlider(name, value) {
        if (isNaN(value)) {
            consoleMessage("Slider '" + name + "' has a non-numeric value", 1)
            return
        }
        if (name === "speed") {
            if (value <= 0) {
                consoleMessage("Slider 'speed' must be positive, got " + value, 1)
                return
            }
            speed = value
            return
        }
        if (!adjust.apply(name, value))
            consoleMessage("Unsupported mpv property '" + name + "': the Plasma plugin honours saturation, hue, brightness, contrast, gamma, speed, mute and scaler", 1)
    }

    function handleMessage(message) {
        switch (message.Type) {
        case 12: // lp_slider
            applySlider(String(message.Name), Number(message.Value))
            break
        case 18: // lp_chekbox
            if (message.Name === "mute")
                mute = message.Value === true
            else
                consoleMessage("Unsupported mpv property '" + message.Name + "': the only checkbox honoured is mute", 1)
            break
        case 19: { // lp_dropdown_scaler
            var name = util.scalerFromIndex(Number(message.Value))
            if (name === "")
                consoleMessage("Scaler index " + message.Value + " is out of range (0 none, 1 fill, 2 uniform, 3 uniformFill)", 1)
            else
                scalerOverride = name
            break
        }
        case 16: // lp_button: IsDefault resets; the core then re-sends the file's values.
            if (message.IsDefault === true)
                resetToDefaults()
            break
        case 100: // host_mpv_command
            runMpvCommand(message.Command)
            break
        default:
            // lp_textbox, lp_dropdown, lp_fdropdown, lp_cpicker, lsp_perfcntr, lsp_nowplaying and
            // lsp_audio have no meaning for an mpv-style wallpaper; lively-mpv-host ignores them too.
            break
        }
    }

    // ---- host_mpv_command ----------------------------------------------------

    function runMpvCommand(command) {
        if (!Array.isArray(command) || command.length === 0 || typeof command[0] !== "string") {
            consoleMessage("host_mpv_command needs a Command array starting with the command name, got " + JSON.stringify(command), 1)
            return
        }
        switch (command[0]) {
        case "seek":
            seek(Number(command[1]), command.length > 2 ? String(command[2]) : "relative")
            break
        case "set_property":
            setMpvProperty(String(command[1]), command[2])
            break
        default:
            consoleMessage("Unsupported mpv command " + JSON.stringify(command)
                           + " (the Plasma plugin runs seek and set_property pause/volume/mute/speed/aid)", 0)
            break
        }
    }

    // mpv seek: amount in seconds (absolute, relative) or percent (absolute-percent, relative-percent).
    function seek(amount, flags) {
        if (isNaN(amount)) {
            consoleMessage("seek needs a numeric amount", 1)
            return
        }
        if (!player.seekable) {
            consoleMessage("seek ignored: the media is not seekable (" + player.source + ")", 1)
            return
        }
        var percent = flags === "absolute-percent" || flags === "relative-percent"
        if (percent && player.duration <= 0) {
            consoleMessage("seek " + flags + " ignored: the media has no known duration", 1)
            return
        }
        var target
        switch (flags) {
        case "absolute-percent": target = amount / 100 * player.duration; break
        case "relative-percent": target = player.position + amount / 100 * player.duration; break
        case "absolute": target = amount * 1000; break
        case "relative": target = player.position + amount * 1000; break
        default:
            consoleMessage("Unsupported seek flag '" + flags + "' (absolute, relative, absolute-percent, relative-percent)", 0)
            return
        }
        var clamped = Math.max(0, Math.round(target))
        if (player.duration > 0)
            clamped = Math.min(clamped, player.duration)
        player.position = clamped
    }

    // mpv flag values arrive as JSON booleans or the strings yes/no; null for anything else.
    function mpvFlag(value) {
        if (value === true || value === "yes")
            return true
        if (value === false || value === "no")
            return false
        return null
    }

    function setMpvProperty(name, value) {
        switch (name) {
        case "pause": {
            var pause = mpvFlag(value)
            if (pause === null)
                consoleMessage("set_property pause needs a boolean, got " + JSON.stringify(value), 1)
            else
                userPaused = pause
            break
        }
        case "volume": {
            var level = Number(value)
            if (isNaN(level))
                consoleMessage("set_property volume needs a number 0-100, got " + JSON.stringify(value), 1)
            else
                volumeRequested(Math.max(0, Math.min(100, Math.round(level))))
            break
        }
        case "mute": {
            var muted = mpvFlag(value)
            if (muted === null)
                consoleMessage("set_property mute needs a boolean, got " + JSON.stringify(value), 1)
            else
                mute = muted
            break
        }
        case "speed":
            applySlider("speed", Number(value))
            break
        case "aid":
            if (value === "no" || value === false)
                audioTrackEnabled = false
            else if (value === "auto" || value === true || (!isNaN(Number(value)) && Number(value) >= 1))
                audioTrackEnabled = true
            else
                consoleMessage("set_property aid needs \"no\", \"auto\" or a track number, got " + JSON.stringify(value), 1)
            break
        default:
            consoleMessage("Unsupported mpv property '" + name + "' in set_property (pause, volume, mute, speed, aid)", 0)
            break
        }
    }

    MediaPlayer {
        id: player
        source: renderer.mediaUrl
        loops: MediaPlayer.Infinite
        playbackRate: renderer.speed
        audioOutput: AudioOutput {
            volume: renderer.volume / 100
            muted: renderer.mute || !renderer.audioTrackEnabled
        }
        videoOutput: videoOutput

        onSourceChanged: {
            renderer.loadReported = false
            if (String(source) !== "" && renderer.playing)
                play()
        }
        onMediaStatusChanged: {
            if (renderer.loadReported)
                return
            if (mediaStatus === MediaPlayer.LoadedMedia || mediaStatus === MediaPlayer.BufferedMedia) {
                renderer.loadReported = true
                renderer.loaded(true)
            } else if (mediaStatus === MediaPlayer.InvalidMedia) {
                renderer.loadReported = true
                renderer.consoleMessage("Media could not be loaded: " + source, 1)
                renderer.loaded(false)
            }
        }
        onErrorOccurred: (error, errorString) => {
            renderer.consoleMessage("MediaPlayer error " + error + ": " + errorString + " (" + source + ")", 1)
            if (!renderer.loadReported) {
                renderer.loadReported = true
                renderer.loaded(false)
            }
        }
    }

    VideoOutput {
        id: videoOutput
        anchors.centerIn: parent
        // none: native resolution (one video pixel per device pixel), centered, cropped by clip.
        width: renderer.activeScaler === "none" && sourceRect.width > 0
               ? sourceRect.width / Screen.devicePixelRatio : renderer.width
        height: renderer.activeScaler === "none" && sourceRect.height > 0
                ? sourceRect.height / Screen.devicePixelRatio : renderer.height
        fillMode: renderer.activeScaler === "uniform" ? VideoOutput.PreserveAspectFit
                : renderer.activeScaler === "uniformFill" ? VideoOutput.PreserveAspectCrop
                : VideoOutput.Stretch
        visible: !adjust.active
    }

    AdjustLayer {
        id: adjust
        anchors.fill: videoOutput
        sourceItem: videoOutput
    }
}
