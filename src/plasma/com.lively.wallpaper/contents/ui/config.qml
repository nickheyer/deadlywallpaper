/*
    SPDX-License-Identifier: MIT

    Settings page shown in Plasma's wallpaper configuration dialog. Everything
    except the fill color is written by the Lively Wallpaper application.
*/

import QtQuick
import QtQuick.Controls as QQC2
import QtQuick.Layouts
import org.kde.kirigami as Kirigami
import org.kde.kquickcontrols as KQuickControls

// parentLayout and i18nd are injected into the context by Plasma's wallpaper config dialog
// (the system plugins use them the same way), so qmllint cannot resolve them here.
// qmllint disable unqualified
Kirigami.FormLayout {
    id: root
    twinFormLayouts: parentLayout

    property alias formLayout: root

    property string cfg_Source
    property string cfg_Kind
    property string cfg_CoreSocket
    property string cfg_Instance
    property string cfg_Scaler
    property int cfg_Volume
    property bool cfg_Interactive
    property alias cfg_FillColor: colorButton.color

    QQC2.Label {
        Kirigami.FormData.isSection: true
        Layout.fillWidth: true
        Layout.maximumWidth: Kirigami.Units.gridUnit * 30
        wrapMode: Text.WordWrap
        text: i18nd("plasma_wallpaper_com.lively.wallpaper",
                    "This wallpaper is controlled by the Lively Wallpaper application. Choose and configure wallpapers there; the settings below are written by Lively and shown here for reference.")
    }

    QQC2.Label {
        Kirigami.FormData.label: i18nd("plasma_wallpaper_com.lively.wallpaper", "Source:")
        Layout.fillWidth: true
        Layout.maximumWidth: Kirigami.Units.gridUnit * 30
        elide: Text.ElideMiddle
        text: root.cfg_Source === "" ? i18nd("plasma_wallpaper_com.lively.wallpaper", "(no wallpaper set)") : root.cfg_Source
    }

    QQC2.Label {
        Kirigami.FormData.label: i18nd("plasma_wallpaper_com.lively.wallpaper", "Kind:")
        text: root.cfg_Kind
    }

    KQuickControls.ColorButton {
        id: colorButton
        Kirigami.FormData.label: i18nd("plasma_wallpaper_com.lively.wallpaper", "Fill color:")
        dialogTitle: i18nd("plasma_wallpaper_com.lively.wallpaper", "Select Fill Color")
    }
}
