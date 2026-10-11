import QtQuick

// One pill per plan: "● Claude 64%" with the busiest window's use, or "● Hermes 21k" with
// today's tokens where there are no limits. A click shows that plan's card.
Row {
    id: pills

    // [{ key, label, text, used }]: `used` (0 to 100) colours the dot, -1 for none
    required property var items
    required property var notch
    // The pill whose card is shown
    required property string chosen

    signal toggled(string key)

    spacing: 8

    function dotColor(item) {
        if (item.used < 0)
            return pills.notch.agentColor(item.key)
        return item.used >= 90 ? "#ff5f57" : item.used >= 70 ? "#ffb020" : "#34c759"
    }

    Repeater {
        model: pills.items

        Rectangle {
            id: pill

            required property var modelData
            readonly property bool current: pills.chosen === modelData.key
            readonly property color dot: pills.dotColor(modelData)

            width: content.implicitWidth + 20
            height: 24
            radius: 12
            color: current ? Qt.alpha(dot, 0.16) : area.containsMouse ? "#14ffffff" : "#0dffffff"
            border.width: 1
            border.color: current ? Qt.alpha(dot, 0.55) : "#14ffffff"

            Row {
                id: content
                anchors.centerIn: parent
                spacing: 6

                Rectangle {
                    anchors.verticalCenter: parent.verticalCenter
                    width: 7
                    height: 7
                    radius: 3.5
                    color: pill.dot
                }
                Text {
                    anchors.verticalCenter: parent.verticalCenter
                    text: pill.modelData.label + "  " + pill.modelData.text
                    color: pill.current ? "#f2f3f5" : "#c9ccd2"
                    font.pixelSize: 11
                    font.weight: Font.Medium
                    font.features: { "tnum": 1 }
                }
            }

            MouseArea {
                id: area
                anchors.fill: parent
                hoverEnabled: true
                cursorShape: Qt.PointingHandCursor
                onClicked: pills.toggled(pill.modelData.key)
            }
        }
    }
}
