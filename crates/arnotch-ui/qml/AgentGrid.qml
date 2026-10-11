import QtQuick

// The coding agents installed on this computer, each a tile with its face: a click opens it
// in a terminal. Agents not connected yet are marked; they connect in the settings.
Rectangle {
    id: grid

    required property var bridge
    required property var notch
    // Refreshed whenever the grid is shown: [{ name, label, installed, connected }]
    property var agents: []
    property string error: ""
    // Where agents open: the folder of the project being worked on, or home when empty
    property string folder: ""

    signal launched()

    function refresh() {
        try {
            agents = JSON.parse(bridge.agentStatuses()).filter(a => a.installed)
        } catch (e) {
            agents = []
        }
    }

    // The island is open: the list is read again then (an agent may have been installed).
    property bool active: false
    onActiveChanged: {
        if (active)
            refresh()
        else
            error = ""
    }
    Component.onCompleted: refresh()

    implicitHeight: tiles.implicitHeight + 16 + (errorText.visible ? errorText.implicitHeight + 6 : 0)
    radius: 16
    color: "#0affffff"
    border.width: 1
    border.color: "#0fffffff"

    Grid {
        id: tiles

        x: 8
        y: 8
        width: grid.width - 16
        columns: 2
        spacing: 6

        Repeater {
            model: grid.agents

            Rectangle {
                id: tile

                required property var modelData
                readonly property color tint: grid.notch.agentColor(modelData.name)

                width: (tiles.width - tiles.spacing) / 2
                height: 34
                radius: 17
                color: area.containsMouse ? "#1affffff" : "#0dffffff"
                border.width: 1
                border.color: area.containsMouse ? Qt.alpha(tint, 0.5) : "#0fffffff"

                Face {
                    id: tileFace
                    anchors.left: parent.left
                    anchors.leftMargin: 8
                    anchors.verticalCenter: parent.verticalCenter
                    size: 18
                    tint: Qt.lighter(tile.tint, 1.25)
                    shade: tile.tint
                }
                Column {
                    anchors.left: tileFace.right
                    anchors.leftMargin: 8
                    anchors.right: parent.right
                    anchors.rightMargin: 10
                    anchors.verticalCenter: parent.verticalCenter

                    Text {
                        width: parent.width
                        elide: Text.ElideRight
                        text: tile.modelData.label
                        color: "#e1e3e8"
                        font.pixelSize: 12
                        font.weight: Font.Medium
                    }
                    Text {
                        visible: !tile.modelData.connected
                        text: qsTr("not connected")
                        color: "#8b909a"
                        font.pixelSize: 9
                    }
                }

                MouseArea {
                    id: area
                    anchors.fill: parent
                    hoverEnabled: true
                    cursorShape: Qt.PointingHandCursor
                    onClicked: {
                        grid.error = grid.bridge.launchAgent(tile.modelData.name, grid.folder)
                        if (grid.error === "")
                            grid.launched()
                    }
                }
            }
        }
    }

    // Why the last tile did not open (no terminal found, say)
    Text {
        id: errorText
        x: 12
        y: tiles.y + tiles.implicitHeight + 6
        width: grid.width - 24
        visible: grid.error !== ""
        text: grid.error
        wrapMode: Text.Wrap
        color: "#ff8a80"
        font.pixelSize: 10
    }
}
