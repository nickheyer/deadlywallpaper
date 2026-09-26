import QtQuick
import QtQuick.Layouts
import org.kde.kirigami as Kirigami

// Plasma's wallpaper settings page for this plugin. The wallpaper itself is chosen in the
// Deadly Wallpaper app; picking another wallpaper type here hands the screen back to Plasma.
ColumnLayout {
    id: root
    property string cfg_Title
    property string cfg_Kind

    Kirigami.InlineMessage {
        Layout.fillWidth: true
        visible: true
        type: Kirigami.MessageType.Information
        text: root.cfg_Title !== ""
            ? i18n("This screen shows “%1” from Deadly Wallpaper. Open Deadly Wallpaper to change it, or choose another wallpaper type above to stop it.", root.cfg_Title)
            : i18n("This screen is managed by Deadly Wallpaper. Open Deadly Wallpaper to choose a live wallpaper, or choose another wallpaper type above to stop it.")
    }

    Item { Layout.fillHeight: true }
}
