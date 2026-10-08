import QtQuick

// A file Claude changed in the chat: its name, how many lines went in and out, and the
// changed lines (an excerpt: diff.truncated says more exist).
Rectangle {
    id: card

    // { file, lines: [{ kind: "add" | "del" | "ctx", n, text }], truncated }
    required property var diff
    readonly property int added: diff.lines.filter(l => l.kind === "add").length
    readonly property int removed: diff.lines.filter(l => l.kind === "del").length
    // Folded to its header; a click shows the lines.
    property bool folded: false

    height: content.height + 12
    radius: 8
    color: "#14ffffff"
    border.color: "#1fffffff"
    border.width: 1

    Column {
        id: content

        x: 6
        y: 6
        width: card.width - 12
        spacing: 3

        Item {
            width: parent.width
            height: 16

            Text {
                id: name
                anchors.left: parent.left
                anchors.right: counts.left
                anchors.rightMargin: 8
                anchors.verticalCenter: parent.verticalCenter
                elide: Text.ElideMiddle
                text: (card.folded ? "▸ " : "▾ ") + "✎ " + card.diff.file
                color: "#c9ccd2"
                font.pixelSize: 11
                font.weight: Font.DemiBold
            }
            Row {
                id: counts
                anchors.right: parent.right
                anchors.verticalCenter: parent.verticalCenter
                spacing: 6

                Text {
                    visible: card.added > 0
                    text: "+" + card.added + (card.diff.truncated ? "…" : "")
                    color: "#7ee2a0"
                    font.pixelSize: 10
                    font.family: "monospace"
                }
                Text {
                    visible: card.removed > 0
                    text: "−" + card.removed + (card.diff.truncated ? "…" : "")
                    color: "#ff8f8a"
                    font.pixelSize: 10
                    font.family: "monospace"
                }
            }
            MouseArea {
                anchors.fill: parent
                cursorShape: Qt.PointingHandCursor
                onClicked: card.folded = !card.folded
            }
        }

        Repeater {
            model: card.folded ? [] : card.diff.lines

            Rectangle {
                id: line

                required property var modelData

                width: content.width
                height: 16
                radius: 3
                color: modelData.kind === "add" ? "#2034c759"
                     : modelData.kind === "del" ? "#26ff5f57"
                     : "transparent"

                Text {
                    id: number
                    width: 26
                    anchors.verticalCenter: parent.verticalCenter
                    horizontalAlignment: Text.AlignRight
                    text: line.modelData.n > 0 ? line.modelData.n : ""
                    color: "#5c616b"
                    font.pixelSize: 10
                    font.family: "monospace"
                }
                Text {
                    anchors.left: number.right
                    anchors.leftMargin: 6
                    anchors.right: parent.right
                    anchors.verticalCenter: parent.verticalCenter
                    elide: Text.ElideRight
                    text: (line.modelData.kind === "add" ? "+ "
                         : line.modelData.kind === "del" ? "- " : "  ") + line.modelData.text
                    color: line.modelData.kind === "add" ? "#7ee2a0"
                         : line.modelData.kind === "del" ? "#ff8f8a"
                         : "#9a9fa8"
                    font.pixelSize: 11
                    font.family: "monospace"
                }
            }
        }

        Text {
            visible: !card.folded && card.diff.truncated
            leftPadding: 32
            text: "⋯"
            color: "#5c616b"
            font.pixelSize: 11
        }
    }
}
