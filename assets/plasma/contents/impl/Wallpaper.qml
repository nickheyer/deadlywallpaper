import QtQuick

// The live wallpaper implementation, loaded by the stable main.qml trampoline. The daemon
// writes this plugin's configuration through plasmashell's scripting interface; the
// wallpaper reports back through `State` and `ScreenshotResult`, each prefixed with the
// generation the daemon wrote.
Item {
    id: wallpaper
    required property var wallpaperItem

    readonly property var cfg: wallpaper.wallpaperItem.configuration
    readonly property string kind: wallpaper.cfg.Kind
    readonly property int generation: wallpaper.cfg.Generation

    function isMedia(kind) {
        return kind === "video" || kind === "videostream" || kind === "gif" || kind === "picture"
    }

    function isWeb(kind) {
        return kind === "web" || kind === "webaudio" || kind === "url" || kind === "scene"
    }

    // Everything the wallpaper reports carries the generation it applies to.
    function report(state) {
        wallpaper.cfg.State = wallpaper.generation + "|" + state
        wallpaper.cfg.writeConfig()
        wallpaper.wallpaperItem.loading = state === "loading"
    }

    function screenshotResult(serial, result) {
        wallpaper.cfg.ScreenshotResult = serial + "|" + result
        wallpaper.cfg.writeConfig()
    }

    Loader {
        id: loader
        anchors.fill: parent
        asynchronous: false
        onStatusChanged: {
            if (status === Loader.Error) {
                wallpaper.report("error|" + (wallpaper.isWeb(wallpaper.kind) ? "QtWebEngine is not available inside plasmashell" : "wallpaper component failed to load"))
            }
        }
    }

    function reload() {
        loader.active = false
        loader.source = ""
        if (wallpaper.kind === "") {
            wallpaper.wallpaperItem.loading = false
            return
        }
        var file = wallpaper.isMedia(wallpaper.kind) ? "Media.qml" : (wallpaper.isWeb(wallpaper.kind) ? "Web.qml" : "")
        if (file === "") {
            wallpaper.report("error|unsupported wallpaper kind '" + wallpaper.kind + "'")
            return
        }
        wallpaper.report("loading")
        loader.setSource(file, { "cfg": wallpaper.cfg, "wallpaperItem": wallpaper })
        loader.active = true
    }

    onGenerationChanged: reload()
    Component.onCompleted: reload()
}
