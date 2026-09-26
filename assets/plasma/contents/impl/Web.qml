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

    WebEngineView {
        id: view
        x: web.spanning ? web.cfg.RegionX - web.cfg.ScreenX : 0
        y: web.spanning ? web.cfg.RegionY - web.cfg.ScreenY : 0
        width: web.spanning ? web.cfg.RegionW : web.width
        height: web.spanning ? web.cfg.RegionH : web.height
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
