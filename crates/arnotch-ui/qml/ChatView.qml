import QtQuick

// Chat with Claude: the conversation, newest at the bottom, and a field to ask.
Column {
    id: view

    // NotchState.chat: rows of { who, body, status }
    required property ListModel messages
    required property bool busy
    required property string mood

    signal asked(string text)
    signal stopped()
    signal cleared()
    signal cancelled()

    function focusInput() {
        field.focusInput()
    }

    spacing: 8

    onVisibleChanged: if (visible) focusInput()

    // Nothing asked yet
    Row {
        visible: view.messages.count === 0
        spacing: 12

        Face {
            anchors.verticalCenter: parent.verticalCenter
            size: 36
            mood: view.mood
        }

        Column {
            anchors.verticalCenter: parent.verticalCenter
            spacing: 2

            Text {
                text: qsTr("Ask Claude anything")
                color: "#f2f3f5"
                font.pixelSize: 14
                font.weight: Font.DemiBold
            }
            Text {
                text: qsTr("Short answers, no tools. Uses your Claude Code login.")
                color: "#8b909a"
                font.pixelSize: 11
            }
        }
    }

    // A Flickable with every message in a Column, not a ListView: a ListView guesses the
    // height of messages it hasn't created yet, and the guesses kept moving the view.
    Flickable {
        id: list

        // Stays at the bottom while the answer is written, unless the user scrolled up.
        property bool following: true

        function follow() {
            if (following && !moving)
                contentY = Math.max(0, contentHeight - height)
        }

        width: view.width
        height: Math.min(contentHeight, 300)
        visible: view.messages.count > 0
        clip: true
        contentWidth: width
        contentHeight: column.height
        boundsBehavior: Flickable.StopAtBounds

        onContentHeightChanged: Qt.callLater(follow)
        onMovementEnded: following = atYEnd

        Column {
            id: column

            width: list.width - 8
            spacing: 8

            Repeater {
                model: view.messages

                // Sending a message always shows the end of the chat again.
                onItemAdded: {
                    list.following = true
                    Qt.callLater(list.follow)
                }

                delegate: Item {
                    id: message

                    required property string who
                    required property string body
                    required property string status
                    readonly property bool mine: who === "user"

                    width: column.width
                    height: bubble.height

                    Rectangle {
                        id: bubble

                        anchors.right: message.mine ? parent.right : undefined
                        anchors.left: message.mine ? undefined : parent.left
                        width: message.mine ? Math.min(body.implicitWidth + 20, column.width * 0.8) : column.width
                        height: body.height + (message.mine ? 12 : 4)
                        radius: 10
                        color: message.mine ? "#264c8dff" : "transparent"

                        TextEdit {
                            id: body

                            anchors.left: parent.left
                            anchors.right: parent.right
                            anchors.verticalCenter: parent.verticalCenter
                            anchors.leftMargin: message.mine ? 10 : 2
                            anchors.rightMargin: message.mine ? 10 : 2
                            readOnly: true
                            selectByMouse: true
                            wrapMode: TextEdit.Wrap
                            selectionColor: "#4c8dff"
                            font.pixelSize: 12
                            textFormat: message.mine ? TextEdit.PlainText : TextEdit.MarkdownText
                            color: message.status === "failed" ? "#ff6b6b" : "#f2f3f5"
                            text: message.status === "failed"
                                  ? qsTr("Couldn't get an answer: %1").arg(message.body)
                                  : message.body
                                        || (message.status === "streaming" ? qsTr("Thinking…") : qsTr("(stopped)"))
                            opacity: message.body || message.status === "failed" ? 1 : 0.6
                            onLinkActivated: link => Qt.openUrlExternally(link)
                        }
                    }
                }
            }
        }

        // Scroll position, only when there is more than fits
        Rectangle {
            parent: list
            anchors.right: parent.right
            y: list.visibleArea.yPosition * list.height
            width: 3
            height: list.visibleArea.heightRatio * list.height
            radius: 1.5
            color: "#40ffffff"
            visible: list.contentHeight > list.height
        }
    }

    Row {
        width: view.width
        spacing: 6

        SendField {
            id: field
            width: parent.width - actions.width - parent.spacing
            monospace: false
            placeholder: view.messages.count ? qsTr("Reply to Claude…") : qsTr("Ask Claude…")
            onSubmitted: text => view.asked(text)
            onCancelled: view.cancelled()
        }

        Row {
            id: actions
            anchors.verticalCenter: parent.verticalCenter
            spacing: 2

            IconButton {
                visible: view.busy
                glyph: "■"
                hint: qsTr("Stop")
                tint: "#ff6b6b"
                onClicked: view.stopped()
            }
            IconButton {
                visible: view.messages.count > 0
                glyph: "+"
                hint: qsTr("New chat")
                onClicked: {
                    view.cleared()
                    view.focusInput()
                }
            }
        }
    }
}
