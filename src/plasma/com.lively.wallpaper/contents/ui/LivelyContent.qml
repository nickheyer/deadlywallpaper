/*
    SPDX-License-Identifier: MIT

    Framework-agnostic wallpaper surface. Connects to the Lively core over a
    WebSocket (PROTOCOL.md section 7), dispatches the section 2 messages to the
    active renderer and answers with msg_wploaded / msg_screenshot / msg_console.
*/

pragma ComponentBehavior: Bound

import QtQuick
import QtQuick.Window
import QtWebSockets

Item {
    id: root

    // Plugin configuration (PROTOCOL.md section 7).
    property string source: ""
    property string kind: "none"
    property string coreSocket: ""
    property string instance: ""
    property string scaler: "uniformFill"
    property int volume: 0
    property bool interactive: false
    property color fillColor: "black"
    // Span mode (the render hosts' --span X,Y,W,H,VW,VH): active when both virtual sizes are
    // > 0. The content is rendered at the virtual screen size and this item shows the slice
    // (spanX, spanY, spanWidth, spanHeight) of it, stretched to its own size.
    property int spanX: 0
    property int spanY: 0
    property int spanWidth: 0
    property int spanHeight: 0
    property int spanVirtualWidth: 0
    property int spanVirtualHeight: 0

    readonly property bool spanRequested: spanVirtualWidth > 0 && spanVirtualHeight > 0
    readonly property bool spanActive: spanRequested && spanWidth > 0 && spanHeight > 0
    readonly property real spanScaleX: spanActive ? stage.width / spanWidth : 1
    readonly property real spanScaleY: spanActive ? stage.height / spanHeight : 1
    readonly property bool kindKnown: knownKinds.indexOf(kind) >= 0
    readonly property bool configurationValid: kindKnown && (!spanRequested || spanActive)

    // True once nothing is left to load: the renderer reported msg_wploaded (either way),
    // there is no renderer for this Kind, or the core closed the session.
    readonly property bool ready: closedByCore || rendererLoader.sourceComponent === null || loadedState !== null
    readonly property string socketUrl: (coreSocket !== "" && instance !== "") ? coreSocket + "/wallpaper/" + instance : ""
    readonly property int socketStatus: socket.status
    // The active renderer (VideoRenderer, ImageRenderer or WebRenderer), null for Kind none.
    readonly property var renderer: rendererLoader.item

    // cmd_close was received: the session ended (the host equivalent of exiting).
    signal closed()
    // Every JSON text frame exchanged with the core, for diagnostics and the test harness.
    signal messageSent(text: string)
    signal messageReceived(text: string)

    LivelyUtil {
        id: util
    }

    property bool closedByCore: false
    property bool suspended: false
    property int currentVolume: volume
    property var loadedState: null
    property var outbox: []
    property int backoffMs: 1000

    readonly property var knownKinds: ["video", "gif", "videostream", "picture", "web", "url", "none"]

    Component.onCompleted: {
        validateConfiguration()
        updateConnection()
    }
    onVolumeChanged: currentVolume = volume
    onSourceChanged: restartSession()
    onKindChanged: {
        restartSession()
        validateConfiguration()
    }
    onSpanRequestedChanged: validateConfiguration()
    onSpanActiveChanged: validateConfiguration()
    // A new core socket is a new session with the same wallpaper: keep loadedState so the
    // Open handler can announce it, but forget any earlier cmd_close.
    onSocketUrlChanged: {
        closedByCore = false
        updateConnection()
    }
    onClosedByCoreChanged: updateConnection()

    function restartSession() {
        loadedState = null
        closedByCore = false
    }

    function validateConfiguration() {
        if (!kindKnown)
            sendConsole("Unsupported Kind '" + kind + "' (expected video, gif, picture, web, url, videostream or none)", 1)
        if (spanRequested && !spanActive)
            sendConsole("Span mode needs a positive slice size, got SpanWidth " + spanWidth + " SpanHeight " + spanHeight
                        + " for virtual screen " + spanVirtualWidth + "x" + spanVirtualHeight, 1)
        if (!configurationValid)
            reportLoaded(false)
    }

    // ---- WebSocket transport -------------------------------------------------

    function updateConnection() {
        reconnectTimer.stop()
        var want = socketUrl !== "" && !closedByCore
        // Already connecting or connected to the right place: leave it alone (the initial
        // binding change and Component.onCompleted both call this).
        if (want && socket.active && String(socket.url) === socketUrl
                && (socket.status === WebSocket.Open || socket.status === WebSocket.Connecting))
            return
        if (socket.active)
            socket.active = false
        if (want) {
            socket.url = socketUrl
            socket.active = true
        }
    }

    Timer {
        id: reconnectTimer
        repeat: false
        onTriggered: {
            if (root.socketUrl === "" || root.closedByCore)
                return
            if (socket.status === WebSocket.Open || socket.status === WebSocket.Connecting)
                return
            socket.active = false
            socket.url = root.socketUrl
            socket.active = true
        }
    }

    WebSocket {
        id: socket
        onStatusChanged: (status) => {
            if (status === WebSocket.Open) {
                root.backoffMs = 1000
                root.flushOutbox()
                if (root.loadedState !== null)
                    root.send({ Type: 2, Success: root.loadedState })
            } else if (status === WebSocket.Closed || status === WebSocket.Error) {
                if (root.socketUrl !== "" && !root.closedByCore) {
                    reconnectTimer.interval = root.backoffMs
                    root.backoffMs = Math.min(root.backoffMs * 2, 10000)
                    reconnectTimer.restart()
                }
            }
        }
        onTextMessageReceived: (message) => root.handleFrame(message)
    }

    function send(message) {
        var text = JSON.stringify(message)
        if (socket.status === WebSocket.Open) {
            socket.sendTextMessage(text)
            messageSent(text)
        } else {
            if (outbox.length >= 256)
                outbox.shift()
            outbox.push(text)
        }
    }

    function flushOutbox() {
        var pending = outbox
        outbox = []
        for (var i = 0; i < pending.length; i++) {
            socket.sendTextMessage(pending[i])
            messageSent(pending[i])
        }
    }

    function sendConsole(message, category) {
        send({ Type: 1, Message: message, Category: category })
    }

    // msg_wploaded is not queued: the Open handler re-announces the current state to every
    // new connection, so the core always learns whether the wallpaper is loaded.
    function reportLoaded(success) {
        loadedState = success
        if (socket.status === WebSocket.Open)
            send({ Type: 2, Success: success })
    }

    // ---- Inbound messages ----------------------------------------------------

    function handleFrame(text) {
        messageReceived(text)
        var message = null
        try {
            message = JSON.parse(text)
        } catch (error) {
            sendConsole("Discarding malformed message from core: " + error.message, 1)
            return
        }
        if (message === null || typeof message !== "object" || typeof message.Type !== "number") {
            sendConsole("Discarding message without a numeric Type: " + text, 1)
            return
        }
        dispatch(message)
    }

    function dispatch(message) {
        switch (message.Type) {
        case 4: // cmd_reload
            if (renderer !== null)
                renderer.reload()
            break
        case 5: // cmd_close: the session ends, the renderer is unloaded and nothing is loaded
                //  any more until the core configures a new session.
            closedByCore = true
            loadedState = null
            closed()
            break
        case 6: // cmd_screenshot
            captureScreenshot(message)
            break
        case 7: // cmd_suspend
            suspended = true
            break
        case 8: // cmd_resume
            suspended = false
            break
        case 9: // cmd_volume
            currentVolume = Math.max(0, Math.min(100, Math.round(Number(message.Volume))))
            break
        case 10: // lsp_perfcntr
        case 11: // lsp_nowplaying
        case 12: // lp_slider
        case 13: // lp_textbox
        case 14: // lp_dropdown
        case 15: // lp_fdropdown
        case 16: // lp_button
        case 17: // lp_cpicker
        case 18: // lp_chekbox
        case 19: // lp_dropdown_scaler
        case 20: // lsp_audio
        case 100: // host_mpv_command
            if (renderer !== null)
                renderer.handleMessage(message)
            break
        default:
            // Unknown message types are ignored (PROTOCOL.md section 1).
            break
        }
    }

    // ---- Screenshots ---------------------------------------------------------

    function captureScreenshot(message) {
        var path = typeof message.FilePath === "string" ? message.FilePath : ""
        var format = typeof message.Format === "number" ? message.Format : -1
        var name = util.fileName(path)
        var dot = name.lastIndexOf(".")
        var suffix = dot < 0 ? "" : name.substring(dot + 1).toLowerCase()
        if (path === "" || util.screenshotSuffixes(format).indexOf(suffix) < 0) {
            sendConsole("Screenshot rejected: Format " + format + " does not match the file name '" + name + "'", 1)
            send({ Type: 3, FileName: name, Success: false })
            return
        }
        var pixelSize = Qt.size(Math.round(stage.width * Screen.devicePixelRatio),
                                Math.round(stage.height * Screen.devicePixelRatio))
        var scheduled = stage.grabToImage(function (result) {
            var saved = result.saveToFile(path)
            if (!saved)
                sendConsole("Screenshot could not be written to " + path, 1)
            send({ Type: 3, FileName: name, Success: saved })
        }, pixelSize)
        if (!scheduled) {
            sendConsole("Screenshot could not be scheduled: the wallpaper item is not being rendered", 1)
            send({ Type: 3, FileName: name, Success: false })
        }
    }

    // ---- Renderers -----------------------------------------------------------

    Item {
        id: stage
        anchors.fill: parent
        clip: true

        Rectangle {
            anchors.fill: parent
            color: root.fillColor
        }

        // In span mode the renderer covers the whole virtual screen; the slice this desktop
        // shows is moved to the origin and stretched so (spanWidth, spanHeight) fills the stage.
        Loader {
            id: rendererLoader
            x: root.spanActive ? -root.spanX * root.spanScaleX : 0
            y: root.spanActive ? -root.spanY * root.spanScaleY : 0
            width: root.spanActive ? root.spanVirtualWidth : stage.width
            height: root.spanActive ? root.spanVirtualHeight : stage.height
            transform: Scale {
                origin.x: 0
                origin.y: 0
                xScale: root.spanScaleX
                yScale: root.spanScaleY
            }
            sourceComponent: {
                if (root.closedByCore || !root.configurationValid)
                    return null
                switch (root.kind) {
                case "video":
                case "gif":
                case "videostream":
                    return videoComponent
                case "picture":
                    return imageComponent
                case "web":
                case "url":
                    return webComponent
                default:
                    return null
                }
            }
        }
    }

    Component {
        id: videoComponent
        VideoRenderer {
            source: root.source
            kind: root.kind
            scaler: root.scaler
            volume: root.currentVolume
            suspended: root.suspended
            onLoaded: (success) => root.reportLoaded(success)
            onConsoleMessage: (message, category) => root.sendConsole(message, category)
            onVolumeRequested: (value) => root.currentVolume = value
        }
    }

    Component {
        id: imageComponent
        ImageRenderer {
            source: root.source
            scaler: root.scaler
            onLoaded: (success) => root.reportLoaded(success)
            onConsoleMessage: (message, category) => root.sendConsole(message, category)
        }
    }

    Component {
        id: webComponent
        WebRenderer {
            source: root.source
            kind: root.kind
            volume: root.currentVolume
            suspended: root.suspended
            interactive: root.interactive
            onLoaded: (success) => root.reportLoaded(success)
            onConsoleMessage: (message, category) => root.sendConsole(message, category)
        }
    }
}
