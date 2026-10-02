import QtQuick

// Detailed view of one agent session: steps on the left,
// the latest diff or command output on the right.
Item {
    id: card

    // NotchState.focus
    required property var session
    required property string mood
    required property color moodColor

    readonly property int maxDiffLines: 10
    readonly property var diffLines: session.diff ? session.diff.lines.slice(0, maxDiffLines) : []

    implicitHeight: Math.max(sidebar.implicitHeight, panel.implicitHeight)

    Column {
        id: sidebar

        width: 150
        spacing: 8

        Face {
            size: 40
            mood: card.mood
            badge: card.mood === "idle" || card.mood === "happy" ? "transparent" : card.moodColor
        }

        Column {
            width: parent.width
            spacing: 1

            Text {
                width: parent.width
                elide: Text.ElideRight
                text: card.session.project
                color: "#f2f3f5"
                font.pixelSize: 14
                font.weight: Font.DemiBold
            }
            Text {
                width: parent.width
                elide: Text.ElideRight
                text: card.session.phase === "waiting" ? card.session.message
                    : card.session.prompt || "Claude Code"
                color: card.session.phase === "waiting" ? "#ffb020" : "#8b909a"
                font.pixelSize: 11
            }
        }

        Column {
            width: parent.width
            spacing: 3

            Repeater {
                model: card.session.steps

                Row {
                    id: step

                    required property var modelData

                    spacing: 6

                    Item {
                        width: 12
                        height: 15

                        Rectangle {
                            id: runningDot
                            anchors.centerIn: parent
                            visible: step.modelData.status === "running"
                            width: 6
                            height: 6
                            radius: 3
                            color: "#4c8dff"

                            SequentialAnimation on opacity {
                                running: runningDot.visible
                                loops: Animation.Infinite
                                onStopped: runningDot.opacity = 1
                                NumberAnimation { to: 0.3; duration: 500 }
                                NumberAnimation { to: 1; duration: 500 }
                            }
                        }
                        Text {
                            anchors.centerIn: parent
                            visible: !runningDot.visible
                            text: step.modelData.status === "fail" ? "✗" : "✓"
                            color: step.modelData.status === "fail" ? "#ff5f57" : "#34c759"
                            font.pixelSize: 11
                            font.weight: Font.Bold
                        }
                    }
                    Text {
                        text: step.modelData.tool
                        color: step.modelData.status === "running" ? "#f2f3f5" : "#8b909a"
                        font.pixelSize: 12
                    }
                }
            }
        }
    }

    // Diff or terminal
    Rectangle {
        id: panel

        anchors.left: sidebar.right
        anchors.leftMargin: 12
        anchors.right: parent.right
        visible: card.session.view !== ""
        implicitHeight: visible ? panelContent.implicitHeight + 16 : 0
        radius: 12
        color: "#14161b"
        border.color: "#12ffffff"
        border.width: 1

        Column {
            id: panelContent

            anchors.left: parent.left
            anchors.right: parent.right
            anchors.top: parent.top
            anchors.margins: 8
            spacing: 6

            // Header: file name or command, plus status
            Item {
                width: parent.width
                height: 18

                Text {
                    anchors.left: parent.left
                    anchors.right: status.left
                    anchors.rightMargin: 8
                    anchors.verticalCenter: parent.verticalCenter
                    elide: Text.ElideRight
                    text: card.session.view === "diff" ? card.session.diff.file : "$ " + card.session.command
                    color: "#c9ccd2"
                    font.pixelSize: 11
                    font.family: "monospace"
                    font.weight: Font.DemiBold
                }
                Text {
                    id: status
                    anchors.right: parent.right
                    anchors.verticalCenter: parent.verticalCenter
                    visible: card.session.view === "terminal"
                    text: card.session.result === "ok" ? qsTr("✓ done")
                        : card.session.result === "fail" ? qsTr("✗ failed")
                        : qsTr("running…")
                    color: card.session.result === "ok" ? "#34c759"
                         : card.session.result === "fail" ? "#ff5f57"
                         : "#4c8dff"
                    font.pixelSize: 11
                }
            }

            // Diff lines
            Column {
                width: parent.width
                visible: card.session.view === "diff"

                Repeater {
                    model: card.diffLines

                    Rectangle {
                        id: diffLine

                        required property var modelData

                        width: parent.width
                        height: 16
                        radius: 3
                        color: modelData.kind === "add" ? "#2034c759"
                             : modelData.kind === "del" ? "#26ff5f57"
                             : "transparent"

                        Text {
                            id: lineNumber
                            width: 26
                            anchors.verticalCenter: parent.verticalCenter
                            horizontalAlignment: Text.AlignRight
                            text: diffLine.modelData.n > 0 ? diffLine.modelData.n : ""
                            color: "#5c616b"
                            font.pixelSize: 10
                            font.family: "monospace"
                        }
                        Text {
                            anchors.left: lineNumber.right
                            anchors.leftMargin: 6
                            anchors.right: parent.right
                            anchors.verticalCenter: parent.verticalCenter
                            elide: Text.ElideRight
                            text: (diffLine.modelData.kind === "add" ? "+ "
                                 : diffLine.modelData.kind === "del" ? "- " : "  ") + diffLine.modelData.text
                            color: diffLine.modelData.kind === "add" ? "#7ee2a0"
                                 : diffLine.modelData.kind === "del" ? "#ff8f8a"
                                 : "#9a9fa8"
                            font.pixelSize: 11
                            font.family: "monospace"
                        }
                    }
                }

                Text {
                    visible: card.session.diff && (card.session.diff.truncated || card.session.diff.lines.length > card.maxDiffLines)
                    leftPadding: 32
                    text: "⋯"
                    color: "#5c616b"
                    font.pixelSize: 11
                }
            }

            // Command output
            Column {
                width: parent.width
                visible: card.session.view === "terminal"

                Repeater {
                    model: card.session.output

                    Text {
                        required property string modelData

                        width: parent.width
                        elide: Text.ElideRight
                        text: modelData
                        color: /^\s*[✗×✘]|\b(fail(ed|ing)?|error)\b/i.test(modelData) ? "#ff8f8a"
                             : /^\s*[✓✔]|\b(pass(ed)?|ok|success)\b/i.test(modelData) ? "#7ee2a0"
                             : "#9a9fa8"
                        font.pixelSize: 11
                        font.family: "monospace"
                    }
                }

                Text {
                    visible: card.session.output.length === 0
                    text: card.session.result === "running" ? qsTr("waiting for output…") : qsTr("(no output)")
                    color: "#5c616b"
                    font.pixelSize: 11
                    font.family: "monospace"
                }
            }
        }
    }
}
