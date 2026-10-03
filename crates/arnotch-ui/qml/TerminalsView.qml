import QtQuick

// The open terminals: what runs where, with buttons to switch to a terminal
// or interrupt its command.
Column {
    id: view

    required property var terminals
    required property var bridge

    // Closes the island after switching to a terminal.
    signal focused()

    // The terminal whose text field is open, if any. The island asks for the keyboard
    // only while this is set.
    property string editingId: ""

    // Seconds since the epoch, ticking only while the list is on screen.
    property real now: Date.now() / 1000

    spacing: 4

    // The rows, updated in place by terminal id: rebuilding them on every rescan (once a
    // second while shown) reset their hover state and any text being typed.
    ListModel { id: rows }

    onTerminalsChanged: syncRows()
    Component.onCompleted: syncRows()

    function syncRows() {
        for (let i = 0; i < terminals.length; i++) {
            const t = terminals[i]
            const json = JSON.stringify(t)
            let j = i
            while (j < rows.count && rows.get(j).id !== t.id)
                j++
            if (j === rows.count) {
                rows.insert(i, { id: t.id, json: json })
                continue
            }
            if (j !== i)
                rows.move(j, i, 1)
            if (rows.get(i).json !== json)
                rows.setProperty(i, "json", json)
        }
        if (rows.count > terminals.length)
            rows.remove(terminals.length, rows.count - terminals.length)
    }

    onVisibleChanged: {
        bridge.setTerminalsVisible(visible)
        if (!visible)
            editingId = ""
    }

    Timer {
        interval: 1000
        repeat: true
        running: view.visible
        triggeredOnStart: true
        onTriggered: view.now = Date.now() / 1000
    }

    function elapsed(startedAt) {
        const secs = Math.max(0, Math.floor(now - startedAt))
        if (secs < 60)
            return qsTr("%1s").arg(secs)
        if (secs < 3600)
            return qsTr("%1m %2s").arg(Math.floor(secs / 60)).arg(String(secs % 60).padStart(2, "0"))
        return qsTr("%1h %2m").arg(Math.floor(secs / 3600)).arg(String(Math.floor(secs % 3600 / 60)).padStart(2, "0"))
    }

    function shortPath(path) {
        const parts = path.split("/").filter(Boolean)
        return parts.length > 2 ? "…/" + parts.slice(-2).join("/") : path
    }

    Text {
        visible: view.terminals.length === 0
        text: qsTr("No open terminals")
        color: "#8b909a"
        font.pixelSize: 12
    }

    Repeater {
        model: rows

        Column {
            id: entry

            required property string json
            readonly property var modelData: JSON.parse(json)
            readonly property bool editing: view.editingId === modelData.id

            width: view.width
            spacing: 4

            Rectangle {
                id: row

                readonly property var modelData: entry.modelData
                readonly property var command: modelData.command

                width: view.width
                height: 40
                radius: 10
                color: rowHover.hovered ? "#1affffff" : "#0dffffff"

                HoverHandler { id: rowHover }

                // Status dot: blue while a command runs, gray at the prompt
                Rectangle {
                    id: dot
                    anchors.left: parent.left
                    anchors.leftMargin: 12
                    anchors.verticalCenter: parent.verticalCenter
                    width: 8
                    height: 8
                    radius: 4
                    color: row.command ? "#4c8dff" : "#4a4e57"
                }

                Column {
                    anchors.left: dot.right
                    anchors.leftMargin: 10
                    anchors.right: buttons.left
                    anchors.rightMargin: 8
                    anchors.verticalCenter: parent.verticalCenter
                    spacing: 1

                    Text {
                        width: parent.width
                        elide: Text.ElideRight
                        text: row.command ? row.command.args : qsTr("%1 · idle").arg(row.modelData.shell)
                        color: row.command ? "#f2f3f5" : "#8b909a"
                        font.pixelSize: 12
                        font.family: "monospace"
                        font.weight: row.command ? Font.DemiBold : Font.Normal
                    }
                    Text {
                        width: parent.width
                        elide: Text.ElideMiddle
                        text: [row.modelData.app, view.shortPath(row.modelData.cwd),
                               row.command ? view.elapsed(row.command.started_at) : "",
                               row.command && row.command.cpu >= 1 ? `${Math.round(row.command.cpu)}% CPU` : ""]
                              .filter(Boolean).join("  ·  ")
                        color: "#6b707a"
                        font.pixelSize: 10
                    }
                }

                Row {
                    id: buttons
                    anchors.right: parent.right
                    anchors.rightMargin: 8
                    anchors.verticalCenter: parent.verticalCenter
                    spacing: 4

                    IconButton {
                        visible: row.command !== null
                        glyph: "■"
                        hint: qsTr("Stop (Ctrl+C, press again to terminate)")
                        tint: "#ff5f57"
                        onClicked: view.bridge.interruptCommand(row.modelData.shell_pid)
                    }
                    IconButton {
                        visible: row.modelData.can_send
                        glyph: "✎"
                        hint: qsTr("Type into this terminal")
                        tint: entry.editing ? "#4c8dff" : "#c9ccd2"
                        onClicked: view.editingId = entry.editing ? "" : row.modelData.id
                    }
                    IconButton {
                        visible: row.modelData.app_pid > 0
                        glyph: "↗"
                        hint: qsTr("Switch to this terminal")
                        tint: "#c9ccd2"
                        onClicked: {
                            view.bridge.focusTerminal(row.modelData.id)
                            view.focused()
                        }
                    }
                }
            }

            SendField {
                id: field
                width: view.width
                visible: entry.editing
                placeholder: row.command ? qsTr("Send to %1…").arg(row.command.name) : qsTr("Run a command…")
                onVisibleChanged: if (visible) focusInput()
                onSubmitted: text => {
                    view.bridge.sendText(row.modelData.id, text)
                    view.editingId = ""
                }
                onCancelled: view.editingId = ""
            }
        }
    }
}
