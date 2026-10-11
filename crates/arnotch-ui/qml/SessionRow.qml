import QtQuick

// One agent session: status dot, project, current tool and its detail.
Item {
    id: row

    required property string sessionId
    required property string project
    required property string phase
    required property string tool
    required property string detail
    required property string agent
    property string agentLabel: ""
    property color agentColor: "#8b909a"

    implicitHeight: 24

    readonly property color phaseColor: phase === "waiting" ? "#ffb020"
                                      : phase === "done" ? "#34c759"
                                      : "#4c8dff"

    readonly property string phaseText: phase === "started" ? qsTr("Thinking…")
                                       : phase === "waiting" ? qsTr("Needs you")
                                       : phase === "done" ? qsTr("Done")
                                       : tool

    Rectangle {
        id: dot
        anchors.verticalCenter: parent.verticalCenter
        width: 8
        height: 8
        radius: 4
        color: row.phaseColor

        SequentialAnimation on opacity {
            running: row.visible && (row.phase === "started" || row.phase === "tool")
            loops: Animation.Infinite
            onStopped: dot.opacity = 1
            NumberAnimation { to: 0.3; duration: 600 }
            NumberAnimation { to: 1; duration: 600 }
        }
    }

    Text {
        id: projectLabel
        anchors.left: dot.right
        anchors.leftMargin: 8
        anchors.verticalCenter: parent.verticalCenter
        width: Math.min(implicitWidth, 120)
        elide: Text.ElideRight
        text: row.project
        color: "#f2f3f5"
        font.pixelSize: 12
        font.weight: Font.DemiBold
    }

    // Which agent: only worth saying when it isn't Claude Code
    Text {
        id: agentText
        anchors.left: projectLabel.right
        anchors.leftMargin: visible ? 6 : 0
        anchors.verticalCenter: parent.verticalCenter
        visible: row.agent !== "claude"
        width: visible ? implicitWidth : 0
        text: row.agentLabel
        color: row.agentColor
        font.pixelSize: 11
    }

    Rectangle {
        id: chip
        anchors.left: agentText.right
        anchors.leftMargin: 8
        anchors.verticalCenter: parent.verticalCenter
        width: chipLabel.implicitWidth + 12
        height: 18
        radius: 5
        color: Qt.alpha(row.phaseColor, 0.16)

        Text {
            id: chipLabel
            anchors.centerIn: parent
            text: row.phaseText
            color: row.phaseColor
            font.pixelSize: 11
            font.weight: Font.Medium
        }
    }

    Text {
        anchors.left: chip.right
        anchors.leftMargin: 8
        anchors.right: parent.right
        anchors.verticalCenter: parent.verticalCenter
        elide: Text.ElideRight
        text: row.detail
        color: "#8b909a"
        font.pixelSize: 11
        font.family: "monospace"
    }
}
