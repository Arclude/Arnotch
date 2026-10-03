import QtQuick

// Files kept on the shelf. Each one can be dragged out into another app,
// shared, sent to Claude, opened or removed. Below them, the clipboard history.
Column {
    id: view

    required property var shelf
    required property var bridge
    required property var claudeTerminals
    required property bool sharingOn
    // Latest text entries of the clipboard history, newest first: [{ text, secret }]
    required property var clipboard

    signal remove(string path)
    signal share(string path)
    signal sendToClaude(string terminalId, string path)

    spacing: 4

    function glyphFor(name) {
        const ext = name.includes(".") ? name.split(".").pop().toLowerCase() : ""
        if (["png", "jpg", "jpeg", "gif", "webp", "svg", "bmp", "heic"].includes(ext)) return "▣"
        if (["mp4", "mkv", "webm", "mov", "avi"].includes(ext)) return "▶"
        if (["mp3", "flac", "ogg", "wav", "m4a"].includes(ext)) return "♪"
        if (["zip", "tar", "gz", "xz", "zst", "7z", "rar"].includes(ext)) return "▥"
        if (["pdf", "doc", "docx", "odt", "txt", "md"].includes(ext)) return "▤"
        return "◇"
    }

    function humanSize(bytes) {
        const units = ["B", "KB", "MB", "GB"]
        let size = bytes
        let unit = 0
        while (size >= 1024 && unit < units.length - 1) {
            size /= 1024
            unit++
        }
        return `${size < 10 && unit > 0 ? size.toFixed(1) : Math.round(size)} ${units[unit]}`
    }

    Text {
        visible: view.shelf.length === 0
        text: qsTr("Drop files on the island to keep them here")
        color: "#8b909a"
        font.pixelSize: 12
    }

    Repeater {
        model: view.shelf

        Rectangle {
            id: row

            required property string modelData
            readonly property string name: modelData.split("/").pop()
            readonly property string folder: modelData.slice(0, modelData.length - name.length - 1)
            readonly property var info: JSON.parse(view.bridge.fileInfo(modelData))
            readonly property string url: "file://" + encodeURI(modelData)

            width: view.width
            height: 40
            radius: 10
            color: dragHandler.active ? "#264c8dff" : rowHover.hovered ? "#1affffff" : "#0dffffff"
            opacity: info.exists ? 1 : 0.5

            // Dragging the row out hands the file to the app it is dropped on.
            Drag.active: dragHandler.active
            Drag.dragType: Drag.Automatic
            Drag.supportedActions: Qt.CopyAction
            // CRLF built from char codes: qmlcachegen (Qt 6.11) emits broken C++
            // for a "\r\n" literal here.
            Drag.mimeData: { "text/uri-list": url + String.fromCharCode(13, 10) }

            HoverHandler {
                id: rowHover
                cursorShape: row.info.exists ? Qt.OpenHandCursor : Qt.ArrowCursor
            }
            DragHandler {
                id: dragHandler
                target: null
                enabled: row.info.exists
            }

            Text {
                id: icon
                anchors.left: parent.left
                anchors.leftMargin: 12
                anchors.verticalCenter: parent.verticalCenter
                width: 16
                horizontalAlignment: Text.AlignHCenter
                text: row.info.dir ? "▭" : view.glyphFor(row.name)
                color: "#c9ccd2"
                font.pixelSize: 14
            }

            Column {
                anchors.left: icon.right
                anchors.leftMargin: 10
                anchors.right: buttons.left
                anchors.rightMargin: 8
                anchors.verticalCenter: parent.verticalCenter
                spacing: 1

                Text {
                    width: parent.width
                    elide: Text.ElideMiddle
                    text: row.name
                    color: "#f2f3f5"
                    font.pixelSize: 12
                    font.weight: Font.DemiBold
                }
                Text {
                    width: parent.width
                    elide: Text.ElideMiddle
                    text: row.info.exists ? (row.info.dir ? row.folder : `${view.humanSize(row.info.size)}  ·  ${row.folder}`)
                                          : qsTr("Missing  ·  %1").arg(row.folder)
                    color: row.info.exists ? "#6b707a" : "#ff8f8a"
                    font.pixelSize: 10
                }
            }

            Row {
                id: buttons
                anchors.right: parent.right
                anchors.rightMargin: 8
                anchors.verticalCenter: parent.verticalCenter
                spacing: 2

                IconButton {
                    visible: row.info.exists && !row.info.dir && view.sharingOn
                    glyph: "↑"
                    hint: qsTr("Share link")
                    onClicked: view.share(row.modelData)
                }
                IconButton {
                    visible: row.info.exists && view.claudeTerminals.length > 0
                    glyph: "@"
                    hint: qsTr("Send to Claude")
                    tint: "#4c8dff"
                    onClicked: view.sendToClaude(view.claudeTerminals[0].id, row.modelData)
                }
                IconButton {
                    visible: row.info.exists
                    glyph: "↗"
                    hint: qsTr("Open")
                    onClicked: Qt.openUrlExternally(row.url)
                }
                IconButton {
                    glyph: "×"
                    hint: qsTr("Remove from shelf")
                    tint: "#8b909a"
                    onClicked: view.remove(row.modelData)
                }
            }
        }
    }

    // Recently copied text (KDE's clipboard history); a click copies it again.
    Text {
        visible: view.clipboard.length > 0
        topPadding: 6
        text: qsTr("Clipboard")
        color: "#8b909a"
        font.pixelSize: 11
        font.weight: Font.DemiBold
    }

    Repeater {
        model: view.clipboard

        Rectangle {
            id: clip

            required property var modelData
            property bool copied: false

            width: view.width
            height: 28
            radius: 8
            color: clipArea.pressed ? "#264c8dff" : clipArea.containsMouse ? "#1affffff" : "#0dffffff"

            Text {
                anchors.left: parent.left
                anchors.right: copiedMark.left
                anchors.leftMargin: 12
                anchors.rightMargin: 8
                anchors.verticalCenter: parent.verticalCenter
                elide: Text.ElideRight
                // One line: line breaks shown as ⏎. Keys and tokens stay hidden.
                text: clip.modelData.secret ? qsTr("🔒 Hidden: looks like a key or token")
                                            : clip.modelData.text.trim().replace(/\s*\n\s*/g, "  ⏎  ")
                color: clip.modelData.secret ? "#8b909a" : "#c9ccd2"
                font.pixelSize: 12
                textFormat: Text.PlainText
            }

            Text {
                id: copiedMark
                anchors.right: parent.right
                anchors.rightMargin: 12
                anchors.verticalCenter: parent.verticalCenter
                text: clip.copied ? qsTr("Copied") : ""
                color: "#34c759"
                font.pixelSize: 10
            }

            MouseArea {
                id: clipArea
                anchors.fill: parent
                hoverEnabled: true
                cursorShape: Qt.PointingHandCursor
                onClicked: {
                    view.bridge.copyText(clip.modelData.text)
                    clip.copied = true
                    copiedTimer.restart()
                }
            }

            Timer {
                id: copiedTimer
                interval: 1200
                onTriggered: clip.copied = false
            }
        }
    }
}
