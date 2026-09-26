/*
    SPDX-License-Identifier: MIT

    Plasma 6 wallpaper plugin for Lively Wallpaper. This file only binds the
    plugin configuration into LivelyContent, which is framework agnostic.
*/

import QtQuick
import org.kde.plasma.plasmoid

WallpaperItem {
    id: root

    // Delays ksplash until the configured wallpaper has loaded (or failed).
    loading: !content.ready

    LivelyContent {
        id: content
        anchors.fill: parent

        source: root.configuration.Source
        kind: root.configuration.Kind
        coreSocket: root.configuration.CoreSocket
        instance: root.configuration.Instance
        scaler: root.configuration.Scaler
        volume: root.configuration.Volume
        interactive: root.configuration.Interactive
        fillColor: root.configuration.FillColor
        spanX: root.configuration.SpanX
        spanY: root.configuration.SpanY
        spanWidth: root.configuration.SpanWidth
        spanHeight: root.configuration.SpanHeight
        spanVirtualWidth: root.configuration.SpanVirtualWidth
        spanVirtualHeight: root.configuration.SpanVirtualHeight
    }
}
