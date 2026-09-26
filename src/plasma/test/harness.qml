/*
    SPDX-License-Identifier: MIT

    Standalone host for LivelyContent.qml (no Plasma required):

      QT_FORCE_STDERR_LOGGING=1 /usr/lib/qt6/bin/qml harness.qml -- \
          --kind video --source /path/file.mp4 --core ws://127.0.0.1:PORT --instance test \
          --scaler uniformFill --volume 0 --interactive false --timeout 90000 \
          [--width 960 --height 540] [--span X,Y,W,H,VW,VH]

    --span mirrors the render hosts' option: this window shows the slice (X,Y,W,H) of a
    wallpaper stretched over a virtual screen VWxVH. Every frame exchanged with the core is
    echoed on stderr. The window closes when the core sends cmd_close; --timeout (ms) aborts
    with exit code 2.
*/

import QtQuick
import QtQuick.Window
import "../com.lively.wallpaper/contents/ui" as Lively

Window {
    id: window
    width: parseInt(args.width)
    height: parseInt(args.height)
    visible: true
    title: "Lively wallpaper harness (" + args.kind + ")"

    readonly property var args: parseArguments()
    readonly property var span: parseSpan(args.span)

    function parseArguments() {
        var argv = Qt.application.arguments
        var values = {
            kind: "none", source: "", core: "", instance: "", scaler: "uniformFill",
            volume: "0", interactive: "false", timeout: "120000", fillColor: "#202020",
            width: "960", height: "540", span: ""
        }
        var start = argv.indexOf("--") + 1
        for (var i = start; i < argv.length; i++) {
            if (argv[i].indexOf("--") === 0 && i + 1 < argv.length) {
                values[argv[i].substring(2)] = argv[i + 1]
                i++
            }
        }
        return values
    }

    // "X,Y,W,H,VW,VH" -> six integers; all zero when the option is absent.
    function parseSpan(text) {
        var numbers = [0, 0, 0, 0, 0, 0]
        if (text === "")
            return numbers
        var parts = text.split(",")
        if (parts.length !== 6) {
            console.warn("harness --span needs six comma separated integers, got '" + text + "'")
            Qt.exit(2)
            return numbers
        }
        for (var i = 0; i < 6; i++)
            numbers[i] = parseInt(parts[i])
        return numbers
    }

    Lively.LivelyContent {
        id: content
        anchors.fill: parent
        source: window.args.source
        kind: window.args.kind
        coreSocket: window.args.core
        instance: window.args.instance
        scaler: window.args.scaler
        volume: parseInt(window.args.volume)
        interactive: window.args.interactive === "true"
        fillColor: window.args.fillColor
        spanX: window.span[0]
        spanY: window.span[1]
        spanWidth: window.span[2]
        spanHeight: window.span[3]
        spanVirtualWidth: window.span[4]
        spanVirtualHeight: window.span[5]

        onMessageSent: (text) => console.warn("harness plugin->core " + text)
        onMessageReceived: (text) => console.warn("harness core->plugin " + text)
        onSocketStatusChanged: console.warn("harness socket status " + socketStatus)
        onReadyChanged: console.warn("harness ready " + ready)
        onClosed: {
            console.warn("harness cmd_close received, quitting")
            quitTimer.start()
        }
    }

    Timer {
        id: quitTimer
        interval: 300
        onTriggered: Qt.quit()
    }

    Timer {
        interval: parseInt(window.args.timeout)
        running: true
        onTriggered: {
            console.warn("harness timeout after " + interval + " ms")
            Qt.exit(2)
        }
    }
}
