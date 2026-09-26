import QtQuick
import org.kde.plasma.plasmoid

// Stable entry point. plasmashell caches QML components by URL for as long as it runs, so
// this file never changes: the implementation lives in a directory named after its own
// content hash, chosen through the `Impl` configuration key the daemon writes, which gives
// every build a fresh URL.
WallpaperItem {
    id: root

    readonly property string impl: root.configuration.Impl

    Rectangle {
        anchors.fill: parent
        color: "black"
    }

    Loader {
        id: loader
        anchors.fill: parent
        asynchronous: false
        onStatusChanged: {
            if (status === Loader.Error) {
                root.configuration.State = root.configuration.Generation + "|error|wallpaper implementation " + root.impl + " failed to load"
                root.configuration.writeConfig()
                root.loading = false
            }
        }
    }

    function load() {
        loader.active = false
        loader.source = ""
        if (root.impl === "") {
            return
        }
        loader.setSource(Qt.resolvedUrl("../" + root.impl + "/Wallpaper.qml"), { "wallpaperItem": root })
        loader.active = true
    }

    onImplChanged: load()
    Component.onCompleted: load()
}
