import QtQuick

// Shown while files are dragged over the island: one tile per thing that can
// be done with them. Dropping anywhere else on the island puts them on the shelf.
// The island owns the DropArea and asks tileAt()/dropOn() where things land.
Row {
    id: targets

    // NotchState.claudeTerminals
    required property var claudeTerminals
    required property bool sharingOn

    // The tile under the dragged files, -1 when none. The island's single DropArea sets
    // it from the pointer position: nested DropAreas let the first one that grabbed
    // the drag keep it, so the highlight lagged behind the pointer.
    property int hoveredTile: -1

    signal shelve(var urls)
    signal sendToClaude(string terminalId, var urls)
    signal share(var urls)
    signal attachToChat(var urls)

    readonly property var tiles: {
        const list = [{ kind: "shelf", glyph: "▤", title: qsTr("Keep on shelf"), subtitle: qsTr("Drag it out later") }]
        for (const t of claudeTerminals.slice(0, 2)) {
            const folder = t.cwd.split("/").filter(Boolean).pop() ?? t.cwd
            list.push({ kind: "claude", glyph: "@", title: qsTr("Send to Claude"), subtitle: `${folder} · ${t.app}`, terminalId: t.id })
        }
        list.push({ kind: "chat", glyph: "✦", title: qsTr("Ask in chat"), subtitle: qsTr("Attach to a message") })
        list.push({ kind: "share", glyph: "↑", title: qsTr("Share link"),
                    subtitle: sharingOn ? qsTr("Upload and copy the link") : qsTr("Set up in settings") })
        return list
    }

    spacing: 8

    // Index of the tile at (x, y) in `item`'s coordinates, -1 when none.
    function tileAt(item, x, y) {
        if (!visible)
            return -1
        const p = item.mapToItem(targets, x, y)
        for (let i = 0; i < tileRepeater.count; i++) {
            const t = tileRepeater.itemAt(i)
            if (p.x >= t.x && p.x < t.x + t.width && p.y >= t.y && p.y < t.y + t.height)
                return i
        }
        return -1
    }

    function dropOn(index, urls) {
        const tile = tiles[index]
        console.info(`arnotch: dropped ${urls.length} file(s) on the ${tile.kind} target`)
        hoveredTile = -1
        if (tile.kind === "shelf")
            shelve(urls)
        else if (tile.kind === "claude")
            sendToClaude(tile.terminalId, urls)
        else if (tile.kind === "chat")
            attachToChat(urls)
        else
            share(urls)
    }

    Repeater {
        id: tileRepeater
        model: targets.tiles

        Item {
            id: tile

            required property var modelData
            required property int index
            readonly property bool containsDrag: targets.hoveredTile === index

            width: (targets.width - targets.spacing * (targets.tiles.length - 1)) / targets.tiles.length
            height: 86

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
