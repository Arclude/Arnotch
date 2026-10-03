import QtQuick

// Chat with Claude: the conversation, newest at the bottom, and a field to ask.
Column {
    id: view

    // NotchState.chat: rows of { who, body, status }
    required property ListModel messages
    required property bool busy
    required property string mood
    // Files waiting to go with the next message (absolute paths)
    required property var attachments
    // A file dialog is open: the island must not close when it takes the focus.
    readonly property bool dialogOpen: dialogLoader.active

    signal asked(string text)
    signal stopped()
    signal cleared()
    signal cancelled()
    signal attached(var paths)
    signal detached(string path)

    function toolText(name) {
        return name === "WebSearch" || name === "web_search" ? qsTr("Searching the web…")
             : name === "WebFetch" || name === "web_fetch" ? qsTr("Reading a web page…")
             : name === "Read" ? qsTr("Reading a file…")
             : qsTr("Looking through files…")
    }

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
                text: qsTr("Attach files with 📎 or drop them on the island. Uses your Claude Code login.")
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
                    required property string files
                    required property string tool
                    readonly property bool mine: who === "user"

                    width: column.width
                    height: bubble.height + (extra.visible ? extra.height + 4 : 0)

                    // Attached files under a message, or the tool Claude is using
                    Text {
                        id: extra
                        anchors.top: bubble.bottom
                        anchors.topMargin: 4
                        anchors.right: message.mine ? parent.right : undefined
                        anchors.left: message.mine ? undefined : parent.left
                        anchors.leftMargin: 2
                        visible: text !== ""
                        text: message.mine ? (message.files ? "📎 " + message.files.split("\n").join(", ") : "")
                                           : (message.tool ? view.toolText(message.tool) : "")
                        color: message.mine ? "#8b909a" : "#8fb4ff"
                        font.pixelSize: 10
                        font.italic: !message.mine
                        elide: Text.ElideMiddle
                        width: Math.min(implicitWidth, column.width)
                    }

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

    // Files waiting to be sent
    Flow {
        width: view.width
        spacing: 4
        visible: view.attachments.length > 0

        Repeater {
            model: view.attachments

            Rectangle {
                id: chip

                required property string modelData

                width: chipText.implicitWidth + 30
                height: 22
                radius: 11
                color: "#1affffff"

                Text {
                    id: chipText
                    anchors.left: parent.left
                    anchors.leftMargin: 9
                    anchors.verticalCenter: parent.verticalCenter
                    text: "📎 " + chip.modelData.split("/").pop()
                    color: "#c9ccd2"
                    font.pixelSize: 10
                }
                Text {
                    anchors.right: parent.right
                    anchors.rightMargin: 8
                    anchors.verticalCenter: parent.verticalCenter
                    text: "×"
                    color: "#8b909a"
                    font.pixelSize: 12

                    MouseArea {
                        anchors.fill: parent
                        anchors.margins: -4
                        cursorShape: Qt.PointingHandCursor
                        onClicked: view.detached(chip.modelData)
                    }
                }
            }
        }
    }

    Loader {
        id: dialogLoader
        active: false
        source: "AttachDialog.qml"
        onLoaded: item.open()

        Connections {
            target: dialogLoader.item
            function onAccepted() {
                const paths = []
                for (const url of dialogLoader.item.selectedFiles) {
                    const text = String(url)
                    if (text.startsWith("file://"))
                        paths.push(decodeURIComponent(text.slice("file://".length)))
                }
                view.attached(paths)
                dialogLoader.active = false
                view.focusInput()
            }
            function onRejected() {
                dialogLoader.active = false
            }
        }
    }

    Row {
        width: view.width
        spacing: 6

        IconButton {
            anchors.verticalCenter: parent.verticalCenter
            glyph: "📎"
            hint: qsTr("Attach files")
            onClicked: dialogLoader.active = true
        }

        SendField {
            id: field
            width: parent.width - actions.width - 26 - parent.spacing * 2
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
