import QtQuick
import QtWebEngine

// Web page, web audio visualizer and website wallpapers. The daemon serves local pages over
// loopback HTTP and pushes properties, audio spectra and pointer events through the bridge
// script it injects at document creation.
Item {
    id: web
    required property var cfg
    required property var wallpaperItem
    clip: true

    readonly property bool spanning: web.cfg.RegionW > 0 && web.cfg.RegionH > 0

    // scalable background span website preview for ref
    WebEngineView {
        id: view
        width: web.spanning ? web.cfg.RegionW : web.width
        height: web.spanning ? web.cfg.RegionH : web.height
        x: (web.width - width) / 2 + (web.spanning ? web.cfg.ViewX : 0)
        y: (web.height - height) / 2 + (web.spanning ? web.cfg.ViewY : 0)
        transform: [
            Scale {
                origin.x: view.width / 2
                origin.y: view.height / 2
                xScale: web.spanning ? web.cfg.ViewScale : 1
                yScale: web.spanning ? web.cfg.ViewScale : 1
            },
            Rotation {
                origin.x: view.width / 2
                origin.y: view.height / 2
                angle: web.spanning ? web.cfg.ViewRotation : 0
            }
        ]
        url: web.cfg.Source
        backgroundColor: "black"
        audioMuted: web.cfg.Muted
        settings.playbackRequiresUserGesture: false
        settings.localContentCanAccessFileUrls: true
        settings.localContentCanAccessRemoteUrls: true
        settings.showScrollBars: false
        settings.webGLEnabled: true
        settings.accelerated2dCanvasEnabled: true
        settings.javascriptCanAccessClipboard: false
        settings.allowWindowActivationFromJavaScript: false
        userScripts.collection: [{
            "name": "deadlywp-bridge",
            "sourceCode": web.cfg.Bridge,
            "injectionPoint": WebEngineScript.DocumentCreation,
            "worldId": WebEngineScript.MainWorld,
            "runsOnSubFrames": false
        }]
        onLoadingChanged: function(info) {
            if (info.status === WebEngineView.LoadSucceededStatus) {
                web.wallpaperItem.report("playing")
            } else if (info.status === WebEngineView.LoadFailedStatus) {
                web.wallpaperItem.report("error|" + (info.errorString !== "" ? info.errorString : "the page failed to load"))
            }
        }
        onNewWindowRequested: function(request) {}
        onRenderProcessTerminated: function(status, code) {
            web.wallpaperItem.report("error|the web view stopped (status " + status + ", code " + code + ")")
        }
    }

    readonly property string screenshotRequest: web.cfg.Screenshot
    onScreenshotRequestChanged: {
        var sep = screenshotRequest.indexOf("|")
        if (sep <= 0) return
        var serial = screenshotRequest.substring(0, sep)
        var path = screenshotRequest.substring(sep + 1)
        var ok = web.grabToImage(function(result) {
            web.wallpaperItem.screenshotResult(serial, result.saveToFile(path) ? "ok" : "error|could not write " + path)
        })
        if (!ok) web.wallpaperItem.screenshotResult(serial, "error|the wallpaper is not on screen")
    }
}
