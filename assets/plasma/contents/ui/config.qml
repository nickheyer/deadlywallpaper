import QtQuick
import QtQuick.Layouts
import org.kde.kirigami as Kirigami

ColumnLayout {
    id: root
    property string cfg_Title
    property string cfg_Kind

    Kirigami.InlineMessage {
        Layout.fillWidth: true
        visible: true
        type: Kirigami.MessageType.Information
        text: root.cfg_Title !== ""
            ? i18n("Wallpaper: %1. Change it in Deadly Wallpaper, or select another wallpaper type above.", root.cfg_Title)
            : i18n("Choose a wallpaper in Deadly Wallpaper, or select another wallpaper type above.")
    }

    Item { Layout.fillHeight: true }
}
