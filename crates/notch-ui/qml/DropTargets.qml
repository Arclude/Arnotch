import QtQuick

// Shown while files are dragged over the island: one tile per thing that can
// be done with them. Dropping anywhere else on the island puts them on the shelf.
Row {
    id: targets

    // NotchState.claudeTerminals
    required property var claudeTerminals
    required property bool sharingOn

    // Each tile is its own DropArea; the island tracks enter/exit to stay open.
    signal targetEntered()
    signal targetExited()
    signal shelve(var urls)
    signal sendToClaude(string terminalId, var urls)
    signal share(var urls)

    readonly property var tiles: {
        const list = [{ kind: "shelf", glyph: "▤", title: qsTr("Keep on shelf"), subtitle: qsTr("Drag it out later") }]
        for (const t of claudeTerminals.slice(0, 2)) {
            const folder = t.cwd.split("/").filter(Boolean).pop() ?? t.cwd
            list.push({ kind: "claude", glyph: "@", title: qsTr("Send to Claude"), subtitle: folder, terminalId: t.id })
        }
        list.push({ kind: "share", glyph: "↑", title: qsTr("Share link"),
                    subtitle: sharingOn ? qsTr("Upload and copy the link") : qsTr("Set up in settings") })
        return list
    }

    spacing: 8

    Repeater {
        model: targets.tiles

        DropArea {
            id: tile

            required property var modelData

            width: (targets.width - targets.spacing * (targets.tiles.length - 1)) / targets.tiles.length
            height: 86

            onEntered: drag => {
                drag.accept(Qt.CopyAction)
                targets.targetEntered()
            }
            onExited: targets.targetExited()
            onDropped: drop => {
                drop.accept(Qt.CopyAction)
                const urls = drop.urls
                if (modelData.kind === "shelf")
                    targets.shelve(urls)
                else if (modelData.kind === "claude")
                    targets.sendToClaude(modelData.terminalId, urls)
                else
                    targets.share(urls)
            }

            Rectangle {
                anchors.fill: parent
                radius: 14
                color: tile.containsDrag ? "#264c8dff" : "#0dffffff"
                border.color: tile.containsDrag ? "#4c8dff" : "#26ffffff"
                border.width: 1

                Behavior on color { ColorAnimation { duration: 120 } }

                Column {
                    anchors.centerIn: parent
                    width: parent.width - 16
                    spacing: 3

                    Text {
                        anchors.horizontalCenter: parent.horizontalCenter
                        text: tile.modelData.glyph
                        color: tile.containsDrag ? "#4c8dff" : "#c9ccd2"
                        font.pixelSize: 22
                        font.weight: Font.DemiBold
                    }
                    Text {
                        width: parent.width
                        horizontalAlignment: Text.AlignHCenter
                        elide: Text.ElideRight
                        text: tile.modelData.title
                        color: "#f2f3f5"
                        font.pixelSize: 12
                        font.weight: Font.DemiBold
                    }
                    Text {
                        width: parent.width
                        horizontalAlignment: Text.AlignHCenter
                        elide: Text.ElideMiddle
                        text: tile.modelData.subtitle
                        color: "#8b909a"
                        font.pixelSize: 10
                    }
                }
            }
        }
    }
}
