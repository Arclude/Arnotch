import QtQuick
import QtQuick.Window
import org.kde.layershell as LayerShell

// One island at the top center of a screen.
Window {
    id: root

    // The screen to live on, or null to follow the active screen.
    property var targetScreen: null
    required property NotchState notch
    required property var settings
    required property var bridge

    // Layer-shell placement (Wayland). Ignored on X11, where x/y below are used.
    LayerShell.Window.scope: "notch"
    LayerShell.Window.anchors: LayerShell.Window.AnchorTop
    LayerShell.Window.layer: LayerShell.Window.LayerOverlay
    // -1 ignores other panels' reserved space (sits over a top panel), 0 respects it (sits below).
    LayerShell.Window.exclusionZone: settings.panelMode === "below" ? 0 : -1
    // The island takes the keyboard only while a text field is open.
    LayerShell.Window.keyboardInteractivity: wantsKeyboard ? LayerShell.Window.KeyboardInteractivityOnDemand
                                                           : LayerShell.Window.KeyboardInteractivityNone
    LayerShell.Window.wantsToBeOnActiveScreen: targetScreen === null

    Binding on screen {
        when: root.targetScreen !== null
        value: root.targetScreen
    }

    // Room around the island for its shadow.
    readonly property int pad: expanded ? 22 : 0
    // Only the island takes input; the rest of the window lets clicks through to
    // the windows below (applied as the window mask by app.cpp).
    readonly property rect inputRect: Qt.rect(island.x, island.y, island.width, island.height)

    readonly property int collapsedWidth: 190
    // Matches the top panel by default, so the closed island never covers windows.
    readonly property int collapsedHeight: settings.height
    // The tab picked by the user; an island opened by an event always shows activity.
    property string tab: "activity"
    // Files are being dragged over the island: it shows drop targets instead of a tab.
    property bool dragging: false
    readonly property string shownTab: dragging ? "drop" : opened ? tab : "activity"
    readonly property bool sharingOn: settings.shareService !== "off"

    signal settingsRequested()
    readonly property bool onActivity: shownTab === "activity"

    // Show the detailed activity card instead of the headline.
    readonly property bool showCard: onActivity && notch.focus !== null && !notch.notifying
                                     && (notch.focus.steps.length > 0 || !!notch.focus.permission)
    readonly property int expandedWidth: dragging ? 620
                                       : shownTab === "terminals" || shownTab === "shelf" ? 540
                                       : showCard ? 640 : 470
    readonly property int maxExpandedWidth: 640
    readonly property int expandedHeight: expandedContent.implicitHeight + 28

    readonly property bool wantsKeyboard: expanded && (terminalsView.editingId !== "" || activityCard.replying)
    onWantsKeyboardChanged: if (wantsKeyboard) requestActivate()

    // The terminal running the focused Claude session, to send replies to.
    readonly property string replyTerminalId: {
        const session = notch.focus
        if (!session || !session.cwd)
            return ""
        const match = notch.terminals.find(t => t.can_send && t.command && t.command.name === "claude" && t.cwd === session.cwd)
        return match ? match.id : ""
    }

    // Opened by a click; closed by another click or when the pointer leaves.
    property bool opened: false
    readonly property bool expanded: opened
                                     || (notch.alerting && settings.autoExpand)
                                     || (notch.showingActivity && settings.liveActivity)
                                     || dragging || notch.uploading

    readonly property color moodColor: notch.mood === "waiting" ? "#ffb020"
                                     : notch.mood === "working" ? "#4c8dff"
                                     : "#34c759"

    // Always as wide as the widest island: the compositor centres the window, so a
    // width change would move it, and KWin keeps sending drag positions relative to
    // where the window was when the drag entered (the drop targets were off by one).
    width: maxExpandedWidth + 44
    height: Math.max(island.height, expanded ? expandedHeight : collapsedHeight) + pad
    x: Screen.virtualX + (Screen.width - width) / 2
    y: Screen.virtualY
    color: "transparent"
    flags: Qt.FramelessWindowHint | Qt.WindowStaysOnTopHint | (wantsKeyboard ? 0 : Qt.WindowDoesNotAcceptFocus)
    visible: true

    // Moving between drop targets leaves one area before entering the next;
    // only a drag that stays out for a moment really left the island.
    // Files are over the island or one of its targets. Leaving one area for the next
    // briefly reads false, so the island only closes when the drag stays out.
    readonly property bool dragInside: islandDrop.containsDrag
    onDragInsideChanged: {
        if (dragInside) {
            dragging = true
            dragLeave.stop()
        } else {
            dragLeave.restart()
        }
    }

    Timer {
        id: dragLeave
        interval: 300
        onTriggered: if (!root.dragInside) root.dragging = false
    }

    function dropped(action) {
        dragging = false
        dragLeave.stop()
        action()
    }

    function shelve(urls) {
        notch.addToShelf(notch.localPaths(urls))
        tab = "shelf"
        opened = true
    }

    function share(urls) {
        if (!sharingOn) {
            settingsRequested()
            return
        }
        for (const path of notch.localPaths(urls))
            bridge.shareFile(path, settings.shareService, settings.shareCommand)
    }

    function sendToClaude(terminalId, urls) {
        bridge.sendFilesToClaude(terminalId, notch.localPaths(urls).join("\n"))
        opened = false
    }

    // The whole island accepts files. It is the only DropArea: the tile under the
    // pointer is worked out from the position, and dropping elsewhere shelves them.
    DropArea {
        id: islandDrop
        anchors.fill: parent
        onEntered: drag => {
            if (!drag.hasUrls) {
                drag.accepted = false
                return
            }
            drag.accept(Qt.CopyAction)
            dropTargets.hoveredTile = dropTargets.tileAt(islandDrop, drag.x, drag.y)
        }
        onPositionChanged: drag => dropTargets.hoveredTile = dropTargets.tileAt(islandDrop, drag.x, drag.y)
        onExited: dropTargets.hoveredTile = -1
        onDropped: drop => {
            drop.accept(Qt.CopyAction)
            const urls = drop.urls
            const index = dropTargets.tileAt(islandDrop, drop.x, drop.y)
            if (index >= 0) {
                root.dropped(() => dropTargets.dropOn(index, urls))
            } else {
                console.info(`notch: dropped ${urls.length} file(s) on the island`)
                dropTargets.hoveredTile = -1
                root.dropped(() => root.shelve(urls))
            }
        }
    }

    // Keeps the island open briefly after the pointer leaves, so it doesn't flicker.
    Timer {
        id: closeDelay
        interval: 600
        onTriggered: root.opened = false
    }

    // Soft shadow from a few translucent layers: plain rectangles, no shaders,
    // so it stays cheap on every GPU driver.
    Item {
        anchors.fill: island
        opacity: root.expanded ? 1 : 0
        visible: opacity > 0

        Behavior on opacity { NumberAnimation { duration: 200 } }

        Repeater {
            model: 6

            Rectangle {
                required property int index

                readonly property real spread: (index + 1) * 3

                x: -spread
                y: 2 - spread + spread * 0.5
                width: island.width + spread * 2
                height: island.height + spread * 2
                radius: island.bottomLeftRadius + spread
                color: "#000000"
                opacity: 0.09
            }
        }
    }

    Rectangle {
        id: island

        anchors.top: parent.top
        anchors.horizontalCenter: parent.horizontalCenter
        width: root.expanded ? root.expandedWidth : root.collapsedWidth
        height: root.expanded ? root.expandedHeight : root.collapsedHeight
        topLeftRadius: 0
        topRightRadius: 0
        bottomLeftRadius: root.expanded ? 26 : Math.round(root.collapsedHeight * 0.45)
        bottomRightRadius: bottomLeftRadius
        color: "#0b0c0f"
        border.color: "#1affffff"
        border.width: 1
        clip: true

        Behavior on width { NumberAnimation { duration: 380; easing.type: Easing.OutBack; easing.overshoot: 0.9 } }
        Behavior on height { NumberAnimation { duration: 380; easing.type: Easing.OutBack; easing.overshoot: 0.9 } }
        Behavior on bottomLeftRadius { NumberAnimation { duration: 300 } }

        HoverHandler {
            cursorShape: Qt.PointingHandCursor
            onHoveredChanged: {
                if (hovered)
                    closeDelay.stop()
                else if (root.opened && !root.wantsKeyboard)
                    closeDelay.restart()
            }
        }

        TapHandler {
            onTapped: {
                if (root.expanded) {
                    root.opened = false
                    root.notch.dismiss()
                } else {
                    root.opened = true
                }
            }
        }

        // Collapsed: small face, clock and activity summary
        Item {
            anchors.fill: parent
            opacity: root.expanded ? 0 : 1
            visible: opacity > 0

            Behavior on opacity { NumberAnimation { duration: 140 } }

            Face {
                anchors.left: parent.left
                anchors.leftMargin: 12
                anchors.verticalCenter: parent.verticalCenter
                size: Math.round(root.collapsedHeight * 0.58)
                mood: root.notch.mood
            }

            Text {
                anchors.centerIn: parent
                visible: root.settings.showClock
                text: root.notch.time
                color: "#f2f3f5"
                font.pixelSize: Math.min(13, Math.round(root.collapsedHeight * 0.45))
                font.weight: Font.DemiBold
                font.features: { "tnum": 1 }
            }

            Row {
                anchors.right: parent.right
                anchors.rightMargin: 12
                anchors.verticalCenter: parent.verticalCenter
                spacing: 6
                visible: root.notch.sessions.count > 0

                Rectangle {
                    anchors.verticalCenter: parent.verticalCenter
                    width: 7
                    height: 7
                    radius: 3.5
                    color: root.moodColor
                }
                Text {
                    text: root.notch.sessions.count
                    color: "#c9ccd2"
                    font.pixelSize: 12
                    font.weight: Font.DemiBold
                }
            }
        }

        // Expanded: big face, headline and session list
        Column {
            id: expandedContent

            anchors.top: parent.top
            anchors.left: parent.left
            anchors.right: parent.right
            anchors.margins: 14
            spacing: 10
            opacity: root.expanded ? 1 : 0
            visible: opacity > 0

            Behavior on opacity { NumberAnimation { duration: 220 } }

            // Tabs, only when the user opened the island
            Row {
                visible: root.opened && !root.dragging
                spacing: 6

                Repeater {
                    model: [
                        { id: "activity", label: qsTr("Activity") },
                        { id: "terminals", label: qsTr("Terminals") + "  " + root.notch.terminals.length },
                        { id: "shelf", label: qsTr("Shelf") + (root.notch.shelf.length ? "  " + root.notch.shelf.length : "") }
                    ]

                    Rectangle {
                        id: tabChip

                        required property var modelData
                        readonly property bool current: root.tab === modelData.id

                        width: tabLabel.implicitWidth + 20
                        height: 24
                        radius: 12
                        color: current ? "#26ffffff" : tabArea.containsMouse ? "#12ffffff" : "transparent"

                        Text {
                            id: tabLabel
                            anchors.centerIn: parent
                            text: tabChip.modelData.label
                            color: tabChip.current ? "#f2f3f5" : "#8b909a"
                            font.pixelSize: 12
                            font.weight: tabChip.current ? Font.DemiBold : Font.Normal
                        }

                        MouseArea {
                            id: tabArea
                            anchors.fill: parent
                            hoverEnabled: true
                            cursorShape: Qt.PointingHandCursor
                            onClicked: root.tab = tabChip.modelData.id
                        }
                    }
                }
            }

            // Files being shared, as in "Uploading report.pdf  54%"
            Repeater {
                model: root.dragging ? [] : root.notch.uploads

                Column {
                    required property var modelData

                    width: expandedContent.width
                    spacing: 6

                    Item {
                        width: parent.width
                        height: 16

                        Text {
                            anchors.left: parent.left
                            anchors.right: percent.left
                            anchors.rightMargin: 8
                            elide: Text.ElideMiddle
                            text: qsTr("Uploading %1").arg(modelData.file)
                            color: "#f2f3f5"
                            font.pixelSize: 12
                        }
                        Text {
                            id: percent
                            anchors.right: parent.right
                            text: Math.round(modelData.progress * 100) + "%"
                            color: "#8b909a"
                            font.pixelSize: 12
                            font.features: { "tnum": 1 }
                        }
                    }
                    Rectangle {
                        width: parent.width
                        height: 4
                        radius: 2
                        color: "#1affffff"

                        Rectangle {
                            width: parent.width * modelData.progress
                            height: parent.height
                            radius: 2
                            color: "#34c759"

                            Behavior on width { NumberAnimation { duration: 150 } }
                        }
                    }
                }
            }

            DropTargets {
                id: dropTargets
                width: parent.width
                visible: root.dragging
                claudeTerminals: root.notch.claudeTerminals
                sharingOn: root.sharingOn
                onShelve: urls => root.dropped(() => root.shelve(urls))
                onSendToClaude: (terminalId, urls) => root.dropped(() => root.sendToClaude(terminalId, urls))
                onShare: urls => root.dropped(() => root.share(urls))
            }

            ShelfView {
                width: parent.width
                visible: root.shownTab === "shelf"
                shelf: root.notch.shelf
                bridge: root.bridge
                claudeTerminals: root.notch.claudeTerminals
                sharingOn: root.sharingOn
                onRemove: path => root.notch.removeFromShelf(path)
                onShare: path => root.bridge.shareFile(path, root.settings.shareService, root.settings.shareCommand)
                onSendToClaude: (terminalId, path) => {
                    root.bridge.sendFilesToClaude(terminalId, path)
                    root.opened = false
                }
            }

            TerminalsView {
                id: terminalsView
                width: parent.width
                visible: root.shownTab === "terminals"
                terminals: root.notch.terminals
                bridge: root.bridge
                onFocused: root.opened = false
            }

            ActivityCard {
                id: activityCard
                width: parent.width
                replyTerminalId: root.replyTerminalId
                onReplied: text => root.bridge.sendText(root.replyTerminalId, text)
                visible: root.showCard
                // Placeholder while no session is focused (the card is hidden then).
                session: root.notch.focus ?? ({ project: "", phase: "", message: "", prompt: "", steps: [],
                                                 view: "", diff: null, command: "", output: [], result: "" })
                mood: root.notch.mood
                moodColor: root.moodColor
                onAnswered: decision => {
                    const requestId = root.notch.focus.permission.request_id
                    root.bridge.answerPermission(requestId, decision)
                    root.notch.closePermission(requestId)
                }
            }

            Row {
                visible: root.onActivity && !root.showCard
                spacing: 14

                Face {
                    size: 44
                    mood: root.notch.mood
                    badge: root.notch.mood === "idle" || root.notch.mood === "happy" ? "transparent" : root.moodColor
                }

                Column {
                    anchors.verticalCenter: parent.verticalCenter
                    spacing: 2

                    Text {
                        width: expandedContent.width - 90
                        elide: Text.ElideRight
                        text: root.notch.headline
                        color: "#f2f3f5"
                        font.pixelSize: 15
                        font.weight: Font.DemiBold
                    }
                    Text {
                        width: expandedContent.width - 90
                        elide: Text.ElideRight
                        text: root.notch.subline || root.notch.time
                        color: "#8b909a"
                        font.pixelSize: 12
                    }
                }
            }

            Column {
                width: parent.width
                visible: root.onActivity && root.notch.sessions.count > (root.showCard ? 1 : 0)

                Repeater {
                    model: root.notch.sessions

                    SessionRow {
                        width: expandedContent.width
                        // The focused session already has the card.
                        visible: !root.showCard || sessionId !== root.notch.focusId
                    }
                }
            }
        }
    }
}
