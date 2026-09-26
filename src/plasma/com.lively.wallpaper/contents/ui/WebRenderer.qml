/*
    SPDX-License-Identifier: MIT

    Renderer for Kind web (local page) and url (online page). Mirrors
    Lively.Player.WebView2/Form1.cs and lively-web-host (PROTOCOL.md section 5):
    same JavaScript bridge, pause by snapshot + hidden frozen page, screenshots
    and console forwarding.
*/

import QtQuick
import QtQuick.Window
import QtWebEngine
import Qt.labs.folderlistmodel

Item {
    id: renderer

    property string source: ""
    property string kind: "web"
    property int volume: 0
    property bool suspended: false
    property bool interactive: false

    signal loaded(success: bool)
    signal consoleMessage(message: string, category: int)

    LivelyUtil {
        id: util
    }

    // Transparent until the first navigation finishes, then white (WebView2 does the same).
    property bool firstLoadDone: false
    // msg_wploaded is reported once a successful navigation has painted its first frame in the
    // Qt scene graph (a page's two requestAnimationFrame callbacks, then one Qt frame swap), so
    // a screenshot taken right after it shows the page. A page that never paints is reported
    // 3 s after the navigation finished.
    property bool painted: false
    property int framesToWait: 0
    property var lastNowPlaying: null
    property var freezeResult: null

    readonly property string pageUrl: kind === "web" ? util.localFileUrl(source) : util.onlineUrl(source)
    readonly property string pauseMediaScript: "document.querySelectorAll('video, audio').forEach(function (element) { element.pause(); });"
    readonly property string playMediaScript: "document.querySelectorAll('video, audio').forEach(function (element) { element.play(); });"

    onSuspendedChanged: {
        if (suspended)
            suspend()
        else
            resume()
    }

    function reload() {
        view.reload()
    }

    // Calls a page function with JSON-encoded arguments, guarded like every Lively player.
    function callPage(functionName, args) {
        var call = functionName + "(" + args.map(function (argument) { return util.jsLiteral(argument) }).join(", ") + ");"
        view.runJavaScript("if (typeof " + functionName + " === 'function') { " + call + " }")
    }

    function playbackChanged(paused) {
        callPage("livelyWallpaperPlaybackChanged", [JSON.stringify({ IsPaused: paused })])
    }

    function suspend() {
        unfreezeTimer.stop()
        view.runJavaScript(pauseMediaScript)
        playbackChanged(true)
        var scheduled = view.grabToImage(function (result) {
            if (!renderer.suspended)
                return
            renderer.freezeResult = result
            freezeFrame.source = result.url
        })
        if (!scheduled)
            consoleMessage("Suspend: the page could not be snapshotted, so it stays active to keep the picture", 1)
    }

    function freezePage() {
        view.visible = false
        view.lifecycleState = WebEngineView.LifecycleState.Frozen
        if (view.lifecycleState !== WebEngineView.LifecycleState.Frozen)
            consoleMessage("Suspend: Chromium refused to freeze the page; it stays hidden but active", 1)
    }

    function resume() {
        view.visible = true
        view.lifecycleState = WebEngineView.LifecycleState.Active
        view.runJavaScript(playMediaScript)
        playbackChanged(false)
        if (lastNowPlaying !== null)
            callPage("livelyCurrentTrack", [JSON.stringify(lastNowPlaying)])
        unfreezeTimer.restart()
    }

    function armPaintProbe() {
        painted = false
        swapWatch.enabled = false
        view.runJavaScript("window.__livelyPainted = false; requestAnimationFrame(function () { requestAnimationFrame(function () { window.__livelyPainted = true; }); });")
        paintPoll.restart()
        paintDeadline.restart()
    }

    function reportPainted() {
        paintPoll.stop()
        paintDeadline.stop()
        swapWatch.enabled = false
        if (painted)
            return
        painted = true
        loaded(true)
    }

    Timer {
        id: paintPoll
        interval: 16
        repeat: true
        onTriggered: view.runJavaScript("window.__livelyPainted === true", function (result) {
            if (result === true && paintPoll.running) {
                paintPoll.stop()
                renderer.framesToWait = 1
                swapWatch.enabled = true
                view.update()
            }
        })
    }

    Timer {
        id: paintDeadline
        interval: 3000
        repeat: false
        onTriggered: renderer.reportPainted()
    }

    Connections {
        id: swapWatch
        target: renderer.Window.window
        enabled: false
        function onFrameSwapped() {
            renderer.framesToWait--
            if (renderer.framesToWait <= 0)
                renderer.reportPainted()
        }
    }

    function handleMessage(message) {
        switch (message.Type) {
        case 10: // lsp_perfcntr
            callPage("livelySystemInformation", [JSON.stringify(message.Info === undefined ? null : message.Info)])
            break
        case 11: // lsp_nowplaying
            lastNowPlaying = message.Info === undefined ? null : message.Info
            callPage("livelyCurrentTrack", [JSON.stringify(lastNowPlaying)])
            break
        case 12: // lp_slider
        case 13: // lp_textbox
        case 14: // lp_dropdown
        case 17: // lp_cpicker
        case 18: // lp_chekbox
            callPage("livelyPropertyListener", [String(message.Name), message.Value === undefined ? null : message.Value])
            break
        case 15: // lp_fdropdown: relative path when the file exists next to the page, else null.
            applyFolderDropdown(String(message.Name), message.Value)
            break
        case 16: // lp_button: IsDefault is answered by the core re-sending every property.
            if (message.IsDefault !== true)
                callPage("livelyPropertyListener", [String(message.Name), true])
            break
        case 20: // lsp_audio
            callPage("livelyAudioListener", [Array.isArray(message.Data) ? message.Data : []])
            break
        case 100: // host_mpv_command: only the mpv-style renderers understand these.
            consoleMessage("mpv command " + JSON.stringify(message.Command) + " is ignored for web wallpapers", 0)
            break
        default:
            break
        }
    }

    function applyFolderDropdown(name, value) {
        if (typeof value !== "string" || kind !== "web") {
            callPage("livelyPropertyListener", [name, null])
            return
        }
        var absolute = util.directoryOf(source) + "/" + value
        fileExists(absolute, function (exists) {
            callPage("livelyPropertyListener", [name, exists ? value : null])
        })
    }

    Component {
        id: probeComponent
        FolderListModel {
            showDirs: false
            showHidden: true
            showDotAndDotDot: false
            nameFilters: ["*"]
        }
    }

    // Asynchronous existence check; QML has no synchronous file API.
    function fileExists(absolutePath, callback) {
        var probe = probeComponent.createObject(renderer, { folder: util.localFileUrl(util.directoryOf(absolutePath)) })
        var finish = function () {
            if (probe.status !== FolderListModel.Ready)
                return
            var exists = probe.indexOf(util.localFileUrl(absolutePath)) >= 0
            probe.destroy()
            callback(exists)
        }
        probe.statusChanged.connect(finish)
        finish()
    }

    WebEngineView {
        id: view
        anchors.fill: parent
        url: renderer.pageUrl
        enabled: renderer.interactive
        activeFocusOnPress: false
        backgroundColor: renderer.firstLoadDone ? "white" : "transparent"
        audioMuted: renderer.volume === 0

        settings.playbackRequiresUserGesture: false
        settings.webGLEnabled: true
        settings.localContentCanAccessFileUrls: true
        settings.localContentCanAccessRemoteUrls: true
        settings.javascriptCanOpenWindows: false

        onNewWindowRequested: (request) => {
            if (request.userInitiated)
                Qt.openUrlExternally(request.requestedUrl)
        }
        onJavaScriptConsoleMessage: (level, message, lineNumber, sourceID) => renderer.consoleMessage(message, 2)
        onLoadingChanged: (loadingInfo) => {
            if (loadingInfo.status === WebEngineView.LoadSucceededStatus) {
                renderer.firstLoadDone = true
                renderer.armPaintProbe()
            } else if (loadingInfo.status === WebEngineView.LoadFailedStatus) {
                renderer.firstLoadDone = true
                paintPoll.stop()
                paintDeadline.stop()
                swapWatch.enabled = false
                renderer.consoleMessage("Web page load failed: " + loadingInfo.errorString + " (" + loadingInfo.url + ")", 1)
                renderer.loaded(false)
            }
        }
        onRenderProcessTerminated: (terminationStatus, exitCode) =>
            renderer.consoleMessage("Web render process terminated (status " + terminationStatus + ", exit code " + exitCode + ")", 1)
    }

    // Last rendered frame shown while the page is hidden and frozen.
    Image {
        id: freezeFrame
        anchors.fill: parent
        cache: false
        visible: status === Image.Ready
        onStatusChanged: {
            if (status === Image.Ready && renderer.suspended)
                renderer.freezePage()
        }
    }

    Timer {
        id: unfreezeTimer
        interval: 150
        repeat: false
        onTriggered: {
            freezeFrame.source = ""
            renderer.freezeResult = null
        }
    }
}
