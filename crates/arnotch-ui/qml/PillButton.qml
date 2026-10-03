import QtQuick

// A rounded text button. Uses a MouseArea so the click doesn't also reach
// the island's own tap handler.
Rectangle {
    id: button

    property string text
    property color tint: "#c9ccd2"
    property bool filled: false

    signal clicked()

    Accessible.role: Accessible.Button
    Accessible.name: text

    width: label.implicitWidth + 24
    height: 26
    radius: 13
    color: filled ? (area.pressed ? Qt.darker(tint, 1.3) : area.containsMouse ? Qt.lighter(tint, 1.1) : tint)
                  : (area.pressed ? Qt.alpha(tint, 0.3) : area.containsMouse ? Qt.alpha(tint, 0.2) : Qt.alpha(tint, 0.12))

    Text {
        id: label
        anchors.centerIn: parent
        text: button.text
        color: button.filled ? "#0b0c0f" : button.tint
        font.pixelSize: 12
        font.weight: Font.DemiBold
    }

    MouseArea {
        id: area
        anchors.fill: parent
        hoverEnabled: true
        cursorShape: Qt.PointingHandCursor
        onClicked: button.clicked()
    }
}
