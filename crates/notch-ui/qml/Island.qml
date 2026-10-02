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
    LayerShell.Window.keyboardInteractivity: LayerShell.Window.KeyboardInteractivityNone
    LayerShell.Window.wantsToBeOnActiveScreen: targetScreen === null

    Binding on screen {
        when: root.targetScreen !== null
        value: root.targetScreen
    }

    // Room around the island for its shadow.
    // Only while open: the window must not be larger than the closed island, or its
    // transparent margin would swallow clicks meant for the windows below.
    readonly property int pad: expanded ? 22 : 0

    readonly property int collapsedWidth: 190
    // Matches the top panel by default, so the closed island never covers windows.
    readonly property int collapsedHeight: settings.height
    // The tab picked by the user; an island opened by an event always shows activity.
    property string tab: "activity"
    readonly property string shownTab: opened ? tab : "activity"
    readonly property bool onActivity: shownTab === "activity"

    // Show the detailed activity card instead of the headline.
    readonly property bool showCard: onActivity && notch.focus !== null && notch.focus.steps.length > 0 && !notch.notifying
    readonly property int expandedWidth: shownTab === "terminals" ? 540 : showCard ? 640 : 470
    readonly property int expandedHeight: expandedContent.implicitHeight + 28

    // Opened by a click; closed by another click or when the pointer leaves.
    property bool opened: false
    readonly property bool expanded: opened
                                     || (notch.alerting && settings.autoExpand)
                                     || (notch.showingActivity && settings.liveActivity)

    readonly property color moodColor: notch.mood === "waiting" ? "#ffb020"
                                     : notch.mood === "working" ? "#4c8dff"
                                     : "#34c759"

    width: Math.max(island.width, expanded ? expandedWidth : collapsedWidth) + pad * 2
    height: Math.max(island.height, expanded ? expandedHeight : collapsedHeight) + pad
    x: Screen.virtualX + (Screen.width - width) / 2
    y: Screen.virtualY
    color: "transparent"
    flags: Qt.FramelessWindowHint | Qt.WindowStaysOnTopHint | Qt.WindowDoesNotAcceptFocus
    visible: true

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
                else if (root.opened)
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
                visible: root.opened
                spacing: 6

                Repeater {
                    model: [
                        { id: "activity", label: qsTr("Activity") },
                        { id: "terminals", label: qsTr("Terminals") + "  " + root.notch.terminals.length }
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

            TerminalsView {
                width: parent.width
                visible: root.shownTab === "terminals"
                terminals: root.notch.terminals
                bridge: root.bridge
                onFocused: root.opened = false
            }

            ActivityCard {
                width: parent.width
                visible: root.showCard
                session: root.notch.focus ?? ({})
                mood: root.notch.mood
                moodColor: root.moodColor
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
