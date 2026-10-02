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
            return `${secs}s`
        if (secs < 3600)
            return `${Math.floor(secs / 60)}m ${String(secs % 60).padStart(2, "0")}s`
        return `${Math.floor(secs / 3600)}h ${String(Math.floor(secs % 3600 / 60)).padStart(2, "0")}m`
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
        model: view.terminals

        Column {
            id: entry

            required property var modelData
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
