import QtQuick
import QtQuick.Dialogs

// Picks files to attach to a chat message. Loaded on demand, so Qt's dialogs are only
// loaded once the user asks for one.
FileDialog {
    title: qsTr("Attach files")
    fileMode: FileDialog.OpenFiles
}
