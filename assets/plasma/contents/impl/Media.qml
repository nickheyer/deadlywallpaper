import QtQuick
import QtMultimedia

// Video, stream, GIF and picture wallpapers. The content item covers the configured region
// (the whole screen, or a slice of a span across screens) and optional colour adjustments run
// through one fragment shader.
Item {
    id: media
    required property var cfg
    required property var wallpaperItem
    clip: true

    readonly property string kind: media.cfg.Kind
    readonly property bool spanning: media.cfg.RegionW > 0 && media.cfg.RegionH > 0
    readonly property bool adjusted: media.cfg.Saturation !== 0 || media.cfg.Hue !== 0 || media.cfg.Brightness !== 0 || media.cfg.Contrast !== 0 || media.cfg.Gamma !== 0

    function fillModeFor(fit) {
        if (fit === "none") return Image.Pad
        if (fit === "uniform") return Image.PreserveAspectFit
        if (fit === "uniformfill") return Image.PreserveAspectCrop
        return Image.Stretch
    }

    function videoFillModeFor(fit) {
        if (fit === "uniform" || fit === "none") return VideoOutput.PreserveAspectFit
        if (fit === "uniformfill") return VideoOutput.PreserveAspectCrop
        return VideoOutput.Stretch
    }

    // scalable background span image preview for ref
    Item {
        id: region
        width: media.spanning ? media.cfg.RegionW : media.width
        height: media.spanning ? media.cfg.RegionH : media.height
        x: (media.width - width) / 2 + (media.spanning ? media.cfg.ViewX : 0)
        y: (media.height - height) / 2 + (media.spanning ? media.cfg.ViewY : 0)
        transform: [
            Scale {
                origin.x: region.width / 2
                origin.y: region.height / 2
                xScale: media.spanning ? media.cfg.ViewScale : 1
                yScale: media.spanning ? media.cfg.ViewScale : 1
            },
            Rotation {
                origin.x: region.width / 2
                origin.y: region.height / 2
                angle: media.spanning ? media.cfg.ViewRotation : 0
            }
        ]

        Item {
            id: content
            anchors.fill: parent

            Loader {
                id: player
                anchors.fill: parent
                sourceComponent: media.kind === "video" || media.kind === "videostream" ? videoComponent
                               : media.kind === "gif" ? gifComponent : pictureComponent
            }
        }

        ShaderEffectSource {
            id: adjustedSource
            anchors.fill: content
            sourceItem: content
            hideSource: media.adjusted
            live: true
            visible: media.adjusted
        }

        ShaderEffect {
            anchors.fill: content
            visible: media.adjusted
            property variant source: adjustedSource
            property real saturation: media.cfg.Saturation / 100.0
            property real hue: media.cfg.Hue / 100.0
            property real brightness: media.cfg.Brightness / 100.0
            property real contrast: media.cfg.Contrast / 100.0
            property real gamma: media.cfg.Gamma / 100.0
            fragmentShader: Qt.resolvedUrl("adjust.frag.qsb")
        }
    }

    Component {
        id: videoComponent
        Item {
            id: video
            property bool reported: false
            MediaPlayer {
                id: mp
                source: media.cfg.Source
                loops: MediaPlayer.Infinite
                playbackRate: media.cfg.Speed
                videoOutput: vout
                audioOutput: AudioOutput {
                    muted: media.cfg.Muted
                    volume: media.cfg.Volume
                }
                onMediaStatusChanged: {
                    if (mediaStatus === MediaPlayer.LoadedMedia || mediaStatus === MediaPlayer.BufferedMedia) {
                        if (!video.reported) {
                            video.reported = true
                            media.wallpaperItem.report("playing")
                        }
                    } else if (mediaStatus === MediaPlayer.InvalidMedia) {
                        media.wallpaperItem.report("error|" + (errorString !== "" ? errorString : "the media cannot be played"))
                    }
                }
                onErrorOccurred: function(error, errorString) {
                    media.wallpaperItem.report("error|" + errorString)
                }
                Component.onCompleted: if (!media.cfg.Paused) play()
            }
            VideoOutput {
                id: vout
                anchors.fill: parent
                fillMode: media.videoFillModeFor(media.cfg.Fit)
            }
            readonly property bool paused: media.cfg.Paused
            onPausedChanged: if (paused) mp.pause(); else mp.play()
            readonly property string seekRequest: media.cfg.Seek
            onSeekRequestChanged: {
                var parts = seekRequest.split(":")
                if (parts.length < 3 || mp.duration <= 0) return
                var value = parseFloat(parts[2])
                var target = parts[1] === "relative" ? mp.position + mp.duration * value / 100.0 : mp.duration * value / 100.0
                mp.position = Math.max(0, Math.min(mp.duration, target))
            }
        }
    }

    Component {
        id: gifComponent
        AnimatedImage {
            anchors.fill: parent
            source: media.cfg.Source
            fillMode: media.fillModeFor(media.cfg.Fit)
            playing: !media.cfg.Paused
            speed: media.cfg.Speed
            smooth: false
            onStatusChanged: {
                if (status === AnimatedImage.Ready) media.wallpaperItem.report("playing")
                else if (status === AnimatedImage.Error) media.wallpaperItem.report("error|the image cannot be decoded")
            }
        }
    }

    Component {
        id: pictureComponent
        Image {
            anchors.fill: parent
            source: media.cfg.Source
            fillMode: media.fillModeFor(media.cfg.Fit)
            asynchronous: true
            sourceSize: Qt.size(width * Screen.devicePixelRatio, height * Screen.devicePixelRatio)
            onStatusChanged: {
                if (status === Image.Ready) media.wallpaperItem.report("playing")
                else if (status === Image.Error) media.wallpaperItem.report("error|the image cannot be decoded")
            }
        }
    }

    readonly property string screenshotRequest: media.cfg.Screenshot
    onScreenshotRequestChanged: {
        var sep = screenshotRequest.indexOf("|")
        if (sep <= 0) return
        var serial = screenshotRequest.substring(0, sep)
        var path = screenshotRequest.substring(sep + 1)
        var ok = media.grabToImage(function(result) {
            media.wallpaperItem.screenshotResult(serial, result.saveToFile(path) ? "ok" : "error|could not write " + path)
        })
        if (!ok) media.wallpaperItem.screenshotResult(serial, "error|the wallpaper is not on screen")
    }
}
