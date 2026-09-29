import QtQuick
import org.kde.plasma.plasmoid

// Keep this entry point stable; content-hashed Impl URLs bypass Plasma's QML cache.
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
