import QtQuick

// The phone (KDE Connect): its battery, a button to make it ring, and the latest calls
// and messages.
Column {
    id: view

    // NotchState.phone: { device, percent, charging }, null when no phone is in reach
    required property var phone
    // NotchState.phoneLog, newest first: [{ kind: "call" | "message", state, who, body, at }]
    required property var log

    signal ring(string device)

    spacing: 10

    // The phone, its battery and the ring button
    Item {
        width: view.width
        height: 28
        visible: view.phone !== null

        Row {
            anchors.left: parent.left
            anchors.verticalCenter: parent.verticalCenter
            spacing: 8

            Text {
                anchors.verticalCenter: parent.verticalCenter
                text: "📱"
                font.pixelSize: 14
            }
            Text {
                anchors.verticalCenter: parent.verticalCenter
                text: view.phone ? view.phone.device : ""
                color: "#f2f3f5"
                font.pixelSize: 13
                font.weight: Font.DemiBold
            }
            LevelIndicator {
                anchors.verticalCenter: parent.verticalCenter
                kind: "battery"
                value: view.phone ? view.phone.percent / 100 : 0
            }
            Text {
                anchors.verticalCenter: parent.verticalCenter
                visible: view.phone !== null && view.phone.charging
                text: qsTr("charging")
                color: "#34c759"
                font.pixelSize: 11
            }
        }
        PillButton {
            anchors.right: parent.right
            anchors.verticalCenter: parent.verticalCenter
            text: "🔔 " + qsTr("Ring")
            tint: "#c9ccd2"
            onClicked: view.ring(view.phone.device)
        }
    }

    Text {
        width: view.width
        visible: view.phone === null
        wrapMode: Text.WordWrap
        text: qsTr("No phone in reach. Pair it in KDE Connect and keep it on the same network.")
        color: "#8b909a"
        font.pixelSize: 12
    }

    Rectangle {
        width: view.width
        height: 1
        color: "#14ffffff"
    }

    Text {
        visible: view.log.length === 0
        text: qsTr("No calls or messages yet")
        color: "#8b909a"
        font.pixelSize: 12
    }

    Repeater {
        model: view.log.slice(0, 8)

        Item {
            id: entry

            required property var modelData
            readonly property bool missed: modelData.state === "missed_call"

            width: view.width
            height: 34

            Text {
                id: glyph
                anchors.left: parent.left
                anchors.verticalCenter: parent.verticalCenter
                width: 22
                text: entry.modelData.kind === "message" ? "💬" : entry.missed ? "↙" : "📞"
                color: "#ff6b5c"
                font.pixelSize: entry.missed ? 15 : 13
                font.weight: Font.DemiBold
            }
            Column {
                anchors.left: glyph.right
                anchors.leftMargin: 6
                anchors.right: when.left
                anchors.rightMargin: 10
                anchors.verticalCenter: parent.verticalCenter
                spacing: 2

                Text {
                    width: parent.width
                    elide: Text.ElideRight
                    text: entry.modelData.who
                    color: entry.missed ? "#ff6b5c" : "#f2f3f5"
                    font.pixelSize: 12
                    font.weight: Font.DemiBold
                }
                Text {
                    width: parent.width
                    elide: Text.ElideRight
                    text: entry.modelData.kind === "message" ? entry.modelData.body
                          : entry.missed ? qsTr("Missed call")
                          : entry.modelData.state === "talking" ? qsTr("Answered call") : qsTr("Incoming call")
                    color: "#8b909a"
                    font.pixelSize: 11
                }
            }
            Text {
                id: when
                anchors.right: parent.right
                anchors.verticalCenter: parent.verticalCenter
                text: Qt.formatTime(new Date(entry.modelData.at), "HH:mm")
                color: "#8b909a"
                font.pixelSize: 11
                font.features: { "tnum": 1 }
            }
        }
    }
}
