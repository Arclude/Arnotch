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
    // Asking by voice: NotchState.voice ({ state: "listening" | "transcribing", level }) or null
    property var voice: null
    // The answer is being read aloud
    property bool speaking: false
    // The latest loudness levels, oldest first, for the bars
    property var levels: []
    // What Claude may do without asking: "ask", "auto", "acceptEdits" or "plan"
    property string mode: "auto"
    readonly property var modes: ["auto", "acceptEdits", "plan", "ask"]
    onVoiceChanged: {
        if (!voice)
            levels = []
        else if (voice.state === "listening")
            levels = levels.concat([voice.level]).slice(-28)
    }

    signal asked(string text)
    signal stopped()
    signal cleared()
    signal cancelled()
    signal attached(var paths)
    signal detached(string path)
    // Start listening, stop and send what was said, or forget it
    signal silenced()
    signal listen()
    signal listened()
    signal unheard()
    // The next mode was chosen (the mode button, or Shift+Tab in the field)
    signal modeChosen(string mode)
    // "Carry out the plan" under an answer given in plan mode
    signal planApproved()

    function nextMode() {
        view.modeChosen(modes[(modes.indexOf(mode) + 1) % modes.length])
    }

    function modeText(name) {
        return name === "ask" ? qsTr("Ask first")
             : name === "acceptEdits" ? qsTr("Accept edits")
             : name === "plan" ? qsTr("Plan")
             : qsTr("Auto")
    }

    function toolText(name) {
        return name === "WebSearch" || name === "web_search" ? qsTr("Searching the web…")
             : name === "WebFetch" || name === "web_fetch" ? qsTr("Reading a web page…")
             : name === "Read" ? qsTr("Reading a file…")
             : name === "Bash" ? qsTr("Running a command…")
             : name === "Edit" ? qsTr("Changing a file…")
             : name === "Write" ? qsTr("Writing a file…")
             : name.startsWith("mcp__obsidian") ? qsTr("Writing a note in Obsidian…")
             : name.endsWith("Gmail__create_draft") || name.endsWith("Gmail__update_draft") ? qsTr("Saving a draft in Gmail…")
             : name.includes("Gmail__") ? qsTr("Reading Gmail…")
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
            level: view.voice ? view.voice.level : 0
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
                    required property string model
                    required property string changes
                    required property string mode
                    required property int index
                    readonly property bool mine: who === "user"
                    readonly property var changed: JSON.parse(changes || "[]")

                    width: column.width
                    height: bubble.height + (changeList.visible ? changeList.height + 6 : 0)
                            + (extra.visible ? extra.height + 4 : 0)
                            + (approve.visible ? approve.height + 6 : 0)

                    // The plan in the newest answer, while the chat is still in plan mode
                    PillButton {
                        id: approve
                        anchors.top: extra.visible ? extra.bottom : changeList.visible ? changeList.bottom : bubble.bottom
                        anchors.topMargin: 6
                        visible: !message.mine && message.mode === "plan" && message.status === "done"
                                 && view.mode === "plan" && message.index === view.messages.count - 1
                        text: qsTr("Carry out the plan")
                        tint: "#34c759"
                        filled: true
                        onClicked: view.planApproved()
                    }

                    // The files Claude changed while answering, under its answer
                    Column {
                        id: changeList
                        anchors.top: bubble.bottom
                        anchors.topMargin: 6
                        width: parent.width
                        spacing: 4
                        visible: message.changed.length > 0

                        Repeater {
                            model: message.changed

                            ChangeCard {
                                required property var modelData
                                width: changeList.width
                                diff: modelData
                                // Many changes start folded, so the answer stays in view.
                                folded: message.changed.length > 2
                            }
                        }
                    }

                    // Attached files under a message, or the tool Claude is using
                    Text {
                        id: extra
                        anchors.top: changeList.visible ? changeList.bottom : bubble.bottom
                        anchors.topMargin: 4
                        anchors.right: message.mine ? parent.right : undefined
                        anchors.left: message.mine ? undefined : parent.left
                        anchors.leftMargin: 2
                        visible: text !== ""
                        text: message.mine ? (message.files ? "📎 " + message.files.split("\n").join(", ") : "")
                                           : message.tool ? view.toolText(message.tool) : message.model
                        // The model answering, in grey, once no tool is in use.
                        color: message.mine || !message.tool ? "#8b909a" : "#8fb4ff"
                        font.pixelSize: 10
                        font.italic: !message.mine && message.tool !== ""
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

        IconButton {
            anchors.verticalCenter: parent.verticalCenter
            visible: view.voice === null
            glyph: "🎙"
            hint: qsTr("Ask by voice")
            onClicked: view.listen()
        }

        // Listening: bars that follow the voice; a click sends, × forgets it
        Rectangle {
            id: listeningPanel
            visible: view.voice !== null
            width: field.width + 26 + parent.spacing
            height: 30
            radius: 8
            color: "#1a1d23"
            border.color: view.voice && view.voice.state === "listening" ? "#ff9f0a" : "#26ffffff"
            border.width: 1

            readonly property bool listening: view.voice !== null && view.voice.state === "listening"

            Row {
                id: bars
                anchors.left: parent.left
                anchors.leftMargin: 10
                anchors.verticalCenter: parent.verticalCenter
                spacing: 2
                visible: listeningPanel.listening

                Repeater {
                    model: 28

                    Rectangle {
                        required property int index
                        // The newest level on the right
                        readonly property real level: view.levels[view.levels.length - 28 + index] ?? 0
                        anchors.verticalCenter: parent.verticalCenter
                        width: 3
                        height: 3 + Math.round(level * level * 18)
                        radius: 1.5
                        color: "#ff9f0a"
                        opacity: 0.45 + level * 0.55
                    }
                }
            }
            Text {
                anchors.left: listeningPanel.listening ? bars.right : parent.left
                anchors.leftMargin: 10
                anchors.right: unhear.left
                anchors.rightMargin: 6
                anchors.verticalCenter: parent.verticalCenter
                elide: Text.ElideRight
                text: listeningPanel.listening ? qsTr("Listening… click when done") : qsTr("Turning it into text…")
                color: listeningPanel.listening ? "#c9ccd2" : "#8b909a"
                font.pixelSize: 12
            }
            MouseArea {
                anchors.fill: parent
                cursorShape: listeningPanel.listening ? Qt.PointingHandCursor : Qt.ArrowCursor
                onClicked: if (listeningPanel.listening) view.listened()
            }
            IconButton {
                id: unhear
                anchors.right: parent.right
                anchors.rightMargin: 2
                anchors.verticalCenter: parent.verticalCenter
                glyph: "×"
                hint: qsTr("Cancel")
                onClicked: view.unheard()
            }
        }

        SendField {
            id: field
            visible: view.voice === null
            width: parent.width - actions.width - 52 - parent.spacing * 3
            monospace: false
            placeholder: view.messages.count ? qsTr("Reply to Claude…") : qsTr("Ask Claude…")
            onSubmitted: text => view.asked(text)
            onCancelled: view.cancelled()
            onBacktabbed: view.nextMode()
        }

        Row {
            id: actions
            anchors.verticalCenter: parent.verticalCenter
            spacing: 2

            // What Claude may do without asking; a click (or Shift+Tab) moves to the next
            PillButton {
                anchors.verticalCenter: parent.verticalCenter
                text: view.modeText(view.mode)
                tint: view.mode === "plan" ? "#5ac8c8" : view.mode === "acceptEdits" ? "#c792ea"
                    : view.mode === "ask" ? "#8b909a" : "#8fb4ff"
                onClicked: view.nextMode()
            }
            IconButton {
                visible: view.speaking
                glyph: "🔇"
                hint: qsTr("Stop reading aloud")
                tint: "#ff9f0a"
                onClicked: view.silenced()
            }
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
