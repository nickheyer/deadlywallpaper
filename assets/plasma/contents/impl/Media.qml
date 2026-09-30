import QtQuick
import QtMultimedia

// Video, stream, GIF and picture wallpapers. The content item covers the configured region
// (the whole screen, or a slice of a span across screens) and optional colour adjustments run
// through one fragment shader.
//
// Videos and GIFs loop through two players taking turns: while one plays a pass, the other
// waits on its first frame, and the last `LoopBlend` seconds of a pass cross-fade into the
// next, so the hand-over neither stalls nor jumps.
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

    // Cross-fade length in milliseconds: at least two frames, at most a third of the clip.
    function blendMs(fps, durationMs) {
        var frames = fps > 1 ? fps : 30
        return Math.min(Math.max(media.cfg.LoopBlend * 1000, 2000 / frames), durationMs / 3)
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
                sourceComponent: media.kind === "video" ? videoComponent
                               : media.kind === "videostream" ? streamComponent
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

    // One pass of a video: a player with its own picture and sound. `gain` scales the volume
    // through the cross-fade.
    component Pass: Item {
        id: pass
        property alias player: mp
        property real gain: 1
        readonly property real fps: {
            var rate = mp.metaData.value(MediaMetaData.VideoFrameRate)
            return rate > 1 ? rate : 30
        }
        MediaPlayer {
            id: mp
            source: media.cfg.Source
            loops: 1
            playbackRate: media.cfg.Speed
            videoOutput: vout
            audioOutput: AudioOutput {
                muted: media.cfg.Muted
                volume: media.cfg.Volume * pass.gain
            }
        }
        VideoOutput {
            id: vout
            anchors.fill: parent
            fillMode: media.videoFillModeFor(media.cfg.Fit)
        }
    }

    Component {
        id: videoComponent
        Item {
            id: video
            property bool reported: false
            // The pass on screen; the other waits paused on its first frame.
            property int active: 0
            property bool fading: false
            readonly property Pass current: active === 0 ? p0 : p1
            readonly property Pass standby: active === 0 ? p1 : p0

            Pass {
                id: p0
                anchors.fill: parent
                z: video.active === 0 ? 0 : 1
                opacity: video.active === 0 ? 1 : 0
            }
            Pass {
                id: p1
                anchors.fill: parent
                z: video.active === 1 ? 0 : 1
                opacity: video.active === 1 ? 1 : 0
            }

            NumberAnimation {
                id: fadeIn
                property: "opacity"
                from: 0
                to: 1
                easing.type: Easing.Linear
            }
            NumberAnimation {
                id: fadeOut
                property: "gain"
                from: 1
                to: 0
                easing.type: Easing.Linear
            }

            function report(mp) {
                if (mp.mediaStatus === MediaPlayer.LoadedMedia || mp.mediaStatus === MediaPlayer.BufferedMedia) {
                    if (!video.reported) {
                        video.reported = true
                        media.wallpaperItem.report("playing")
                    }
                } else if (mp.mediaStatus === MediaPlayer.InvalidMedia) {
                    media.wallpaperItem.report("error|" + (mp.errorString !== "" ? mp.errorString : "the media cannot be played"))
                }
            }

            // Start the standby playing under a fade that lasts the rest of this pass.
            function startFade() {
                var ms = media.blendMs(video.current.fps, video.current.player.duration) / Math.max(media.cfg.Speed, 0.05)
                video.fading = true
                video.standby.gain = 0
                video.standby.player.play()
                fadeIn.target = video.standby
                fadeIn.duration = ms
                fadeIn.restart()
                fadeOut.target = video.current
                fadeOut.duration = ms
                fadeOut.restart()
            }

            // The pass ended: the standby is the pass on screen, the old one rewinds and waits.
            function handOver() {
                var old = video.current
                var next = video.standby
                fadeIn.stop()
                fadeOut.stop()
                next.opacity = 1
                next.gain = 1
                video.fading = false
                video.active = 1 - video.active
                old.player.pause()
                old.player.position = 0
                old.opacity = 0
                old.gain = 1
            }

            // A seek back out of the window: the standby returns to its first frame.
            function cancelFade() {
                fadeIn.stop()
                fadeOut.stop()
                video.fading = false
                video.standby.player.pause()
                video.standby.player.position = 0
                video.standby.opacity = 0
                video.current.gain = 1
                video.standby.gain = 1
            }

            function onTick(mp) {
                if (mp !== video.current.player || mp.duration <= 0) return
                var ms = media.blendMs(video.current.fps, mp.duration)
                var start = mp.duration - ms
                if (video.fading && mp.position < start - 250) {
                    cancelFade()
                } else if (!video.fading && mp.position >= start && !media.cfg.Paused) {
                    startFade()
                }
            }

            Connections {
                target: p0.player
                function onPositionChanged() { video.onTick(p0.player) }
                function onMediaStatusChanged() {
                    video.report(p0.player)
                    if (p0.player.mediaStatus === MediaPlayer.EndOfMedia && video.active === 0) video.handOver()
                }
                function onErrorOccurred(error, errorString) { media.wallpaperItem.report("error|" + errorString) }
            }
            Connections {
                target: p1.player
                function onPositionChanged() { video.onTick(p1.player) }
                function onMediaStatusChanged() {
                    if (p1.player.mediaStatus === MediaPlayer.EndOfMedia && video.active === 1) video.handOver()
                    else if (p1.player.mediaStatus === MediaPlayer.InvalidMedia) video.report(p1.player)
                }
                function onErrorOccurred(error, errorString) { media.wallpaperItem.report("error|" + errorString) }
            }

            Component.onCompleted: {
                // The standby decodes its first frame and holds it.
                p1.player.pause()
                if (!media.cfg.Paused) p0.player.play()
            }

            readonly property bool paused: media.cfg.Paused
            onPausedChanged: {
                if (paused) {
                    video.current.player.pause()
                    if (video.fading) {
                        video.standby.player.pause()
                        fadeIn.pause()
                        fadeOut.pause()
                    }
                } else {
                    video.current.player.play()
                    if (video.fading) {
                        video.standby.player.play()
                        fadeIn.resume()
                        fadeOut.resume()
                    }
                }
            }
            readonly property string seekRequest: media.cfg.Seek
            onSeekRequestChanged: {
                var parts = seekRequest.split(":")
                var mp = video.current.player
                if (parts.length < 3 || mp.duration <= 0) return
                var value = parseFloat(parts[2])
                var target = parts[1] === "relative" ? mp.position + mp.duration * value / 100.0 : mp.duration * value / 100.0
                mp.position = Math.max(0, Math.min(mp.duration, target))
            }
        }
    }

    // Live streams play once through a single player; there is no end to blend into.
    Component {
        id: streamComponent
        Item {
            id: stream
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
                        if (!stream.reported) {
                            stream.reported = true
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

    // One pass of a GIF. The frame rate is measured from the frame changes themselves,
    // since the decoder does not expose the delays.
    component GifPass: AnimatedImage {
        id: gif
        property real fps: 0
        property real lastChange: 0
        source: media.cfg.Source
        fillMode: media.fillModeFor(media.cfg.Fit)
        playing: false
        speed: media.cfg.Speed
        smooth: false
        onCurrentFrameChanged: {
            var now = Date.now()
            if (gif.lastChange > 0 && now > gif.lastChange) {
                var measured = 1000 / (now - gif.lastChange)
                gif.fps = gif.fps > 0 ? gif.fps * 0.8 + measured * 0.2 : measured
            }
            gif.lastChange = now
        }
    }

    Component {
        id: gifComponent
        Item {
            id: anim
            property bool reported: false
            property int active: 0
            property bool fading: false
            readonly property GifPass current: active === 0 ? g0 : g1
            readonly property GifPass standby: active === 0 ? g1 : g0

            GifPass {
                id: g0
                anchors.fill: parent
                z: anim.active === 0 ? 0 : 1
                opacity: anim.active === 0 ? 1 : 0
            }
            GifPass {
                id: g1
                anchors.fill: parent
                z: anim.active === 1 ? 0 : 1
                opacity: anim.active === 1 ? 1 : 0
            }
            NumberAnimation {
                id: gifFade
                property: "opacity"
                from: 0
                to: 1
                easing.type: Easing.Linear
            }

            function fadeFrames() {
                var fps = anim.current.fps > 1 ? anim.current.fps : 10
                var frames = Math.round(media.cfg.LoopBlend * fps)
                return Math.min(Math.max(frames, 2), Math.floor(anim.current.frameCount / 3))
            }

            function startFade() {
                var fps = anim.current.fps > 1 ? anim.current.fps : 10
                anim.fading = true
                anim.standby.currentFrame = 0
                anim.standby.playing = true
                gifFade.target = anim.standby
                gifFade.duration = fadeFrames() * 1000 / fps / Math.max(media.cfg.Speed, 0.05)
                gifFade.restart()
            }

            function handOver() {
                var old = anim.current
                var next = anim.standby
                gifFade.stop()
                next.opacity = 1
                anim.fading = false
                anim.active = 1 - anim.active
                old.playing = false
                old.currentFrame = 0
                old.opacity = 0
            }

            function onFrame(g) {
                if (g !== anim.current || g.frameCount < 2) return
                if (anim.fading && g.currentFrame === 0) {
                    // The pass wrapped: the standby, a few frames in, carries on.
                    handOver()
                } else if (!anim.fading && g.currentFrame >= g.frameCount - fadeFrames() && !media.cfg.Paused) {
                    startFade()
                }
            }

            Connections {
                target: g0
                function onCurrentFrameChanged() { anim.onFrame(g0) }
                function onStatusChanged() {
                    if (g0.status === AnimatedImage.Ready) {
                        if (!anim.reported) {
                            anim.reported = true
                            media.wallpaperItem.report("playing")
                        }
                        g0.playing = !media.cfg.Paused
                    } else if (g0.status === AnimatedImage.Error) {
                        media.wallpaperItem.report("error|the image cannot be decoded")
                    }
                }
            }
            Connections {
                target: g1
                function onCurrentFrameChanged() { anim.onFrame(g1) }
                function onStatusChanged() {
                    if (g1.status === AnimatedImage.Error) media.wallpaperItem.report("error|the image cannot be decoded")
                }
            }

            readonly property bool paused: media.cfg.Paused
            onPausedChanged: {
                anim.current.paused = paused
                if (anim.fading) {
                    anim.standby.paused = paused
                    if (paused) gifFade.pause(); else gifFade.resume()
                }
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
