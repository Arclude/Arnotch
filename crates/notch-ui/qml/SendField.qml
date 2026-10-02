import QtQuick

// A one-line text field that sends on Enter and cancels on Escape.
Rectangle {
    id: field

    property string placeholder: qsTr("Type and press Enter…")
    property alias text: input.text

    signal submitted(string text)
    signal cancelled()

    function focusInput() {
        input.forceActiveFocus()
    }

    height: 30
    radius: 8
    color: "#1a1d23"
    border.color: input.activeFocus ? "#4c8dff" : "#26ffffff"
    border.width: 1

    Text {
        anchors.fill: input
        verticalAlignment: Text.AlignVCenter
        visible: input.text.length === 0
        text: field.placeholder
        color: "#5c616b"
        font: input.font
    }

    TextInput {
        id: input

        anchors.left: parent.left
        anchors.right: hint.left
        anchors.leftMargin: 10
        anchors.rightMargin: 8
        anchors.verticalCenter: parent.verticalCenter
        clip: true
        color: "#f2f3f5"
        selectionColor: "#4c8dff"
        font.pixelSize: 12
        font.family: "monospace"

        Keys.onReturnPressed: {
            if (text.length > 0) {
                field.submitted(text)
                text = ""
            }
        }
        Keys.onEnterPressed: Keys.returnPressed(event)
        Keys.onEscapePressed: field.cancelled()
    }

    Text {
        id: hint
        anchors.right: parent.right
        anchors.rightMargin: 10
        anchors.verticalCenter: parent.verticalCenter
        text: "↵"
        color: "#5c616b"
        font.pixelSize: 12
    }
}
