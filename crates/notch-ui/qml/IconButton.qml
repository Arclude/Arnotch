import QtQuick

// A small round button with a text glyph. Uses a MouseArea so the click
// doesn't also reach the island's own tap handler.
Rectangle {
    id: button

    property string glyph
    property string hint
    property color tint: "#c9ccd2"

    signal clicked()

    Accessible.role: Accessible.Button
    Accessible.name: hint

    width: 26
    height: 26
    radius: 13
    color: area.pressed ? Qt.alpha(tint, 0.3) : area.containsMouse ? Qt.alpha(tint, 0.18) : "transparent"

    Text {
        anchors.centerIn: parent
        text: button.glyph
        color: button.tint
        font.pixelSize: 12
    }

    MouseArea {
        id: area
        anchors.fill: parent
        hoverEnabled: true
        cursorShape: Qt.PointingHandCursor
        onClicked: button.clicked()
    }
}
