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
    LayerShell.Window.scope: "arnotch"
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
    // Volume, brightness or charge just changed: the closed island widens to show it.
    readonly property bool levelShown: notch.showingLevel && !expanded
    // Talking with Claude by voice: the closed island shows whether it listens, thinks or speaks.
    readonly property bool conversationShown: notch.conversing && !levelShown && !expanded
    // A download, copy or script at work: the closed island shows it with its progress.
    readonly property bool activityShown: notch.runningActivities.length > 0 && !levelShown && !conversationShown && !expanded
    // Music playing: the closed island shows the song and a pause button.
    readonly property bool mediaShown: settings.compactMedia && notch.mediaActive && !levelShown && !activityShown
                                       && !conversationShown && !expanded
    // Matches the top panel by default, so the closed island never covers windows.
    readonly property int collapsedHeight: settings.height
    // The tab picked by the user; an island opened by an event always shows activity.
    property string tab: "activity"
    // Files are being dragged over the island: it shows drop targets instead of a tab.
    property bool dragging: false
    // Files waiting for the user to pick where they go (a screenshot): file:// urls
    property var pendingUrls: []
    readonly property bool choosing: pendingUrls.length > 0
    // The drop targets are shown: files dragged over the island, or waiting for a pick
    readonly property bool targetsShown: dragging || choosing
    readonly property string shownTab: targetsShown ? "drop" : opened ? tab : "activity"

    // Asks Claude about a mail in the chat, which it shows.
    function askAboutMail(text) {
        mailAsked(text)
        tab = "chat"
        opened = true
    }

    // Shows the targets for files to click one of; gives up after 20 seconds.
    function choose(urls) {
        pendingUrls = urls
        chooseTimeout.restart()
    }

    Timer {
        id: chooseTimeout
        interval: 20000
        onTriggered: root.pendingUrls = []
    }
    readonly property bool sharingOn: settings.shareService !== "off"

    signal settingsRequested()
    // The user asked Claude something in the chat.
    signal asked(string text)
    // ...about a mail: Claude may read Gmail and save drafts
    signal mailAsked(string text)
    // The user wants to ask by voice, or to stop talking by voice.
    signal listenRequested()
    signal conversationEnded()

    readonly property bool onActivity: shownTab === "activity"

    // Show the detailed activity card instead of the headline.
    // (A plan chosen from the usage pills takes its place.)
    readonly property bool showCard: onActivity && notch.focus !== null && !notch.notifying && shownPlan === null
                                     && (notch.focus.steps.length > 0 || !!notch.focus.permission)
    readonly property int expandedWidth: targetsShown ? 620
                                       : shownTab === "terminals" || shownTab === "shelf" || shownTab === "mail" ? 540
                                       : showCard || shownTab === "chat" || agentsShown ? 640 : 470
    readonly property int maxExpandedWidth: 640
    // The agents' tiles beside the headline, when the user opened the island
    readonly property bool agentsShown: onActivity && opened && !showCard && !targetsShown && agentGrid.agents.length > 0

    // The usage pills: Claude's limits from its status line, then the other agents' plans.
    readonly property var planItems: {
        const items = []
        const limits = notch.limits
        const brief = name => notch.agentLabel(name).replace(" CLI", "").replace(" Code", "")
        if (limits) {
            const windows = [limits.five_hour, limits.seven_day].filter(w => w)
            if (windows.length)
                items.push({ key: "claude", label: brief("claude"), text: Math.round(windows[0].used_percent) + "%",
                             used: Math.max(...windows.map(w => w.used_percent)) })
        }
        for (const plan of notch.plans) {
            if (plan.windows && plan.windows.length)
                items.push({ key: plan.agent, label: brief(plan.agent), text: Math.round(plan.windows[0].used_percent) + "%",
                             used: Math.max(...plan.windows.map(w => w.used_percent)) })
            else if (plan.tokens)
                items.push({ key: plan.agent, label: brief(plan.agent), used: -1,
                             text: plan.tokens.today >= 1000 ? Math.round(plan.tokens.today / 1000) + "k"
                                 : plan.tokens.today > 0 ? String(plan.tokens.today) : "–" })
        }
        return items
    }
    // The plan whose card is shown, in PlanCard's shape, or null
    readonly property var shownPlan: {
        const key = notch.planShown
        if (!key || !planItems.some(item => item.key === key))
            return null
        if (key === "claude") {
            const limits = notch.limits
            return { key: "claude", label: "Claude", name: "", tokens: null, updated: notch.limitsUpdated,
                     windows: [limits.five_hour ? Object.assign({ minutes: 300 }, limits.five_hour) : null,
                               limits.seven_day ? Object.assign({ minutes: 10080 }, limits.seven_day) : null].filter(w => w) }
        }
        const plan = notch.plans.find(p => p.agent === key)
        return { key: key, label: notch.agentLabel(key).replace(" CLI", ""), name: plan.name ?? "",
                 windows: plan.windows ?? [], tokens: plan.tokens ?? null, updated: plan.updated * 1000 }
    }
    readonly property int expandedHeight: expandedContent.implicitHeight + 28

    readonly property bool wantsKeyboard: expanded && (terminalsView.editingId !== "" || activityCard.wantsKeyboard
                                                       || timerRow.editing
                                                       || shownTab === "chat")
    // Later: the window must first lose Qt.WindowDoesNotAcceptFocus (see flags).
    onWantsKeyboardChanged: if (wantsKeyboard) Qt.callLater(requestActivate)
    // A text field keeps the island open when the pointer leaves; clicking another
    // window (which takes the keyboard) closes it. The chat is kept for next time.
    onActiveChanged: {
        if (active)
            closeDelay.stop()
        else if (opened && wantsKeyboard && !chatView.dialogOpen)
            closeDelay.restart()
    }

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
    // A plan card is for a look while the island is open; next time it shows the activity.
    onOpenedChanged: if (!opened) notch.planShown = ""
    readonly property bool expanded: opened
                                     || (notch.alerting && settings.autoExpand)
                                     || (notch.showingActivity && settings.liveActivity)
                                     || targetsShown || notch.uploading

    readonly property color moodColor: notch.mood === "waiting" ? "#ffb020"
                                     : notch.mood === "working" ? "#4c8dff"
                                     : "#34c759"

    // Always as wide as the widest island: the compositor centres the window, so a
    // width change would move it, and KWin keeps sending drag positions relative to
    // where the window was when the drag entered (the drop targets were off by one).
    width: maxExpandedWidth + 44
    // The window grows to the tallest the island has been and stays so: resizing it is a
    // round trip to the compositor, which costs the first frame or two of the opening.
    // The transparent rest lets clicks through anyway (see inputRect).
    height: windowHeight
    readonly property real wantedHeight: heightSpring.extent + pad
    // Starts tall enough for what the island shows opened (a chat or a question from Claude
    // can take most of the screen), so an opening needs no resize mid-way, which made it
    // stutter; from then on it only grows. (The assignment ends the binding.)
    property real windowHeight: wantedHeight
    Component.onCompleted: windowHeight = Math.max(wantedHeight, Math.min(Screen.height * 0.85, 900))
    // Changes that come together (the content's height settles over a few bindings when
    // the island opens) are applied once.
    onWantedHeightChanged: Qt.callLater(growHeight)
    function growHeight() {
        if (wantedHeight > windowHeight)
            windowHeight = wantedHeight
    }
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
        pendingUrls = []
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
                console.info(`arnotch: dropped ${urls.length} file(s) on the island`)
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
        width: widthSpring.value
        height: heightSpring.value
        topLeftRadius: 0
        topRightRadius: 0
        bottomLeftRadius: root.expanded ? 26 : Math.round(root.collapsedHeight * 0.45)
        bottomRightRadius: bottomLeftRadius
        color: "#0b0c0f"
        border.color: "#1affffff"
        border.width: 1
        clip: true

        // A real spring: it settles in about 300 ms with a ~5% overshoot (which the window's
        // shadow margin has room for), and a change of mind mid-way keeps its momentum.
        Spring {
            id: widthSpring
            target: root.expanded ? root.expandedWidth : root.levelShown ? 310
                  : root.conversationShown ? compactConversation.fitWidth
                  : root.activityShown ? compactActivity.fitWidth
                  : root.mediaShown ? compactMedia.fitWidth
                  : root.collapsedWidth
        }
        Spring {
            id: heightSpring
            target: root.expanded ? root.expandedHeight : root.collapsedHeight
        }
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
                if (root.choosing) {
                    // A click beside the targets cancels the pick.
                    root.pendingUrls = []
                } else if (root.expanded) {
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
                id: compactFace
                anchors.left: parent.left
                anchors.leftMargin: 12
                anchors.verticalCenter: parent.verticalCenter
                size: Math.round(root.collapsedHeight * 0.58)
                mood: root.notch.faceMood
                level: root.notch.voice ? root.notch.voice.level : 0
            }

            // Microphone (orange), camera (green) and screen recording (purple) in use,
            // as dots on the face's corner
            Row {
                anchors.right: compactFace.right
                anchors.rightMargin: -5
                anchors.top: compactFace.top
                anchors.topMargin: -3
                spacing: 2
                visible: root.settings.privacyDots

                Repeater {
                    model: [
                        { on: root.notch.privacy.microphone.length > 0, color: "#ff9f0a" },
                        { on: root.notch.privacy.camera.length > 0, color: "#30d158" },
                        { on: root.notch.privacy.screen.length > 0, color: "#bf5af2" }
                    ].filter(d => d.on)

                    Rectangle {
                        required property var modelData
                        width: 7
                        height: 7
                        radius: 3.5
                        color: modelData.color
                        border.color: "#0b0c0f"
                        border.width: 1
                    }
                }
            }

            // Talking by voice: bars that follow the voice (or Claude's), what is going on, and ×
            Item {
                id: compactConversation
                anchors.fill: parent
                visible: root.conversationShown

                readonly property string state: root.notch.conversation
                readonly property bool waiting: state === "transcribing" || state === "thinking"
                readonly property color tint: state === "listening" ? "#ff9f0a" : state === "speaking" ? "#34c759" : "#8fb4ff"
                // Claude's voice isn't measured: its bars move on their own.
                property var spoken: []
                // Moves the waiting dots
                property real phase: 0
                readonly property var labels: [qsTr("Listening"), qsTr("Understanding…"), qsTr("Thinking…"), qsTr("Speaking")]
                // The widest label: the island keeps one width while the state changes.
                property real labelWidth: 0
                readonly property real fitWidth: Math.round(conversationBars.anchors.leftMargin + conversationBars.width
                                                            + 10 + labelWidth + 12 + conversationEnd.width + 4)

                Repeater {
                    model: compactConversation.labels

                    Text {
                        required property string modelData
                        visible: false
                        text: modelData
                        font: conversationLabel.font
                        function measure() {
                            compactConversation.labelWidth = Math.max(compactConversation.labelWidth, implicitWidth)
                        }
                        onImplicitWidthChanged: measure()
                        Component.onCompleted: measure()
                    }
                }

                Timer {
                    running: compactConversation.visible && compactConversation.state === "speaking"
                    repeat: true
                    interval: 90
                    onTriggered: compactConversation.spoken = compactConversation.spoken.concat([0.3 + Math.random() * 0.7]).slice(-16)
                    onRunningChanged: if (!running) compactConversation.spoken = []
                }
                Timer {
                    running: compactConversation.visible && compactConversation.waiting
                    repeat: true
                    interval: 100
                    onTriggered: compactConversation.phase += 1
                }

                Row {
                    id: conversationBars
                    anchors.left: parent.left
                    anchors.leftMargin: 12 + Math.round(root.collapsedHeight * 0.58 * 1.3) + 12
                    anchors.verticalCenter: parent.verticalCenter
                    spacing: 2

                    readonly property var levels: compactConversation.state === "speaking" ? compactConversation.spoken
                                                : root.notch.voiceLevels

                    Repeater {
                        model: 16

                        Rectangle {
                            required property int index
                            readonly property real level: conversationBars.levels[conversationBars.levels.length - 16 + index] ?? 0
                            anchors.verticalCenter: parent.verticalCenter
                            width: 3
                            height: compactConversation.waiting ? 3 : 3 + Math.round(level * level * (root.collapsedHeight - 12))
                            radius: 1.5
                            color: compactConversation.tint
                            // Waiting: a wave of light runs along the dots.
                            opacity: compactConversation.waiting
                                     ? 0.3 + 0.7 * Math.max(0, Math.cos((compactConversation.phase - index) * 0.4))
                                     : 0.45 + level * 0.55

                            Behavior on height { NumberAnimation { duration: 80 } }
                        }
                    }
                }
                Text {
                    id: conversationLabel
                    anchors.left: conversationBars.right
                    anchors.leftMargin: 10
                    anchors.right: conversationEnd.left
                    anchors.rightMargin: 6
                    anchors.verticalCenter: parent.verticalCenter
                    elide: Text.ElideRight
                    text: compactConversation.state === "listening" ? compactConversation.labels[0]
                        : compactConversation.state === "transcribing" ? compactConversation.labels[1]
                        : compactConversation.state === "thinking" ? compactConversation.labels[2]
                        : compactConversation.state === "speaking" ? compactConversation.labels[3] : ""
                    color: compactConversation.tint
                    font.pixelSize: Math.min(12, Math.round(root.collapsedHeight * 0.42))
                    font.weight: Font.DemiBold
                }
                IconButton {
                    id: conversationEnd
                    anchors.right: parent.right
                    anchors.rightMargin: 4
                    anchors.verticalCenter: parent.verticalCenter
                    width: Math.min(26, root.collapsedHeight - 4)
                    height: width
                    glyph: "×"
                    hint: qsTr("End the conversation")
                    onClicked: root.conversationEnded()
                }
            }

            // A transfer or script at work: its ring, name and percentage
            Item {
                id: compactActivity
                anchors.fill: parent
                visible: root.activityShown

                readonly property var current: root.notch.runningActivities[0]
                                               ?? ({ kind: "", name: "", percent: 0, state: "running" })
                readonly property int others: root.notch.runningActivities.length - 1
                readonly property bool sessions: root.notch.sessions.count > 0
                readonly property real rightEdge: sessions ? 48 : 12
                readonly property real fitWidth: {
                    const left = compactRing.anchors.leftMargin + compactRing.size + 8
                    const right = compactPercent.implicitWidth + 10 + rightEdge
                                  + (compactActivityTimer.visible ? compactActivityTimer.implicitWidth + 10 : 0)
                    const most = 400 + (root.notch.timer ? 60 : 0)
                    return Math.round(Math.max(root.collapsedWidth + 60, Math.min(most, left + compactActivityName.implicitWidth + right)))
                }

                ProgressRing {
                    id: compactRing
                    anchors.left: parent.left
                    anchors.leftMargin: 12 + Math.round(root.collapsedHeight * 0.58 * 1.3) + 10
                    anchors.verticalCenter: parent.verticalCenter
                    size: Math.round(root.collapsedHeight * 0.66)
                    // All of them together when there are several
                    percent: compactActivity.others > 0 && root.notch.activitiesProgress >= 0
                             ? root.notch.activitiesProgress * 100 : compactActivity.current.percent
                    tint: compactActivity.current.state === "paused" ? "#8b909a"
                        : compactActivity.current.kind === "script" ? "#bf8cff" : "#4c8dff"
                    glyph: compactActivity.current.kind === "download" ? "↓" : compactActivity.current.kind === "copy" ? "→" : ""
                }
                Text {
                    id: compactActivityName
                    anchors.left: compactRing.right
                    anchors.leftMargin: 8
                    anchors.right: compactActivityTimer.visible ? compactActivityTimer.left : compactPercent.left
                    anchors.rightMargin: 10
                    anchors.verticalCenter: parent.verticalCenter
                    elide: Text.ElideMiddle
                    text: compactActivity.current.name
                          + (compactActivity.others > 0 ? "  " + qsTr("+%1 more").arg(compactActivity.others) : "")
                    color: "#f2f3f5"
                    font.pixelSize: Math.min(12, Math.round(root.collapsedHeight * 0.42))
                    font.weight: Font.DemiBold
                }
                Text {
                    id: compactActivityTimer
                    anchors.right: compactPercent.left
                    anchors.rightMargin: 10
                    anchors.verticalCenter: parent.verticalCenter
                    visible: root.notch.timer !== null
                    text: root.notch.timerIcon + " " + timerRow.format(root.notch.timer ? root.notch.timer.endsAt - root.notch.timerNow : 0)
                    color: root.notch.timerColor
                    font.pixelSize: Math.min(13, Math.round(root.collapsedHeight * 0.45))
                    font.weight: Font.DemiBold
                    font.features: { "tnum": 1 }
                }
                Text {
                    id: compactPercent
                    anchors.right: parent.right
                    anchors.rightMargin: compactActivity.rightEdge
                    anchors.verticalCenter: parent.verticalCenter
                    text: compactActivity.current.state === "paused" ? qsTr("paused")
                        : compactRing.percent >= 0 ? Math.floor(compactRing.percent) + "%" : "…"
                    color: compactActivity.current.state === "paused" ? "#8b909a" : Qt.lighter(compactRing.tint, 1.25)
                    font.pixelSize: Math.min(13, Math.round(root.collapsedHeight * 0.45))
                    font.weight: Font.DemiBold
                    font.features: { "tnum": 1 }
                }
            }

            LevelIndicator {
                anchors.centerIn: parent
                anchors.horizontalCenterOffset: 10
                visible: root.levelShown
                kind: root.notch.level ? root.notch.level.kind : "volume"
                value: root.notch.level ? root.notch.level.value : 0
                muted: root.notch.level ? root.notch.level.muted : false
            }

            // The playing song: cover, title and artist, and a pause button on the right
            Item {
                id: compactMedia
                anchors.fill: parent
                visible: root.mediaShown

                readonly property var player: root.notch.media ?? ({ title: "", artist: "", art_url: "", playing: false, bus_name: "" })
                // Just wide enough for the song (a short one leaves no gap), at most as wide
                // as a long one gets: then it is cut short.
                readonly property real fitWidth: {
                    const sessions = root.notch.sessions.count > 0
                    const left = compactCover.anchors.leftMargin + compactCover.size + 8
                    const right = compactCountdown.visible
                        ? compactCountdown.implicitWidth + 10 + (sessions ? 86 : 40)
                        : 6 + compactPause.size + (sessions ? 48 : 8)
                    const most = (sessions ? 420 : 380) + (root.notch.timer ? 60 : 0)
                    return Math.round(Math.max(root.collapsedWidth + 60, Math.min(most, left + compactTitle.implicitWidth + 4 + right)))
                }

                MediaCard.Cover {
                    id: compactCover
                    anchors.left: parent.left
                    anchors.leftMargin: 12 + Math.round(root.collapsedHeight * 0.58 * 1.3) + 10
                    anchors.verticalCenter: parent.verticalCenter
                    size: Math.round(root.collapsedHeight * 0.66)
                    url: compactMedia.visible ? compactMedia.player.art_url : ""
                }
                Text {
                    id: compactTitle
                    anchors.left: compactCover.right
                    anchors.leftMargin: 8
                    anchors.right: compactCountdown.visible ? compactCountdown.left : compactPause.left
                    anchors.rightMargin: compactCountdown.visible ? 10 : root.notch.sessions.count > 0 ? 48 : 8
                    anchors.verticalCenter: parent.verticalCenter
                    elide: Text.ElideRight
                    textFormat: Text.StyledText
                    text: `<b>${compactMedia.player.title.replace(/&/g, "&amp;").replace(/</g, "&lt;")}</b>`
                          + (compactMedia.player.artist ? `<font color="#8b909a">  ${compactMedia.player.artist.replace(/&/g, "&amp;").replace(/</g, "&lt;")}</font>` : "")
                    color: "#f2f3f5"
                    font.pixelSize: Math.min(12, Math.round(root.collapsedHeight * 0.42))
                }
                // The timer, left of the session count and the pause button
                Text {
                    id: compactCountdown
                    anchors.right: parent.right
                    anchors.rightMargin: root.notch.sessions.count > 0 ? 86 : 40
                    anchors.verticalCenter: parent.verticalCenter
                    visible: root.notch.timer !== null
                    text: root.notch.timerIcon + " " + timerRow.format(root.notch.timer ? root.notch.timer.endsAt - root.notch.timerNow : 0)
                    color: root.notch.timerColor
                    font.pixelSize: Math.min(13, Math.round(root.collapsedHeight * 0.45))
                    font.weight: Font.DemiBold
                    font.features: { "tnum": 1 }
                }
                MediaCard.MediaButton {
                    id: compactPause
                    anchors.right: parent.right
                    anchors.rightMargin: 6
                    anchors.verticalCenter: parent.verticalCenter
                    size: Math.min(26, root.collapsedHeight - 4)
                    kind: compactMedia.player.playing ? "pause" : "play"
                    hint: compactMedia.player.playing ? qsTr("Pause") : qsTr("Play")
                    onClicked: root.bridge.mediaCommand(compactMedia.player.bus_name, "play_pause")
                }
            }

            // The timer's countdown in place of the clock (beside the song while one plays)
            Text {
                visible: root.notch.timer !== null && !root.levelShown && !root.mediaShown && !root.activityShown && !root.conversationShown
                anchors.centerIn: parent
                text: root.notch.timerIcon + " " + timerRow.format(root.notch.timer ? root.notch.timer.endsAt - root.notch.timerNow : 0)
                color: root.notch.timerColor
                font.pixelSize: Math.min(13, Math.round(root.collapsedHeight * 0.45))
                font.weight: Font.DemiBold
                font.features: { "tnum": 1 }
            }

            Text {
                anchors.centerIn: parent
                visible: root.settings.showClock && !root.levelShown && !root.mediaShown && !root.activityShown && !root.conversationShown && root.notch.timer === null
                text: root.notch.time
                color: "#f2f3f5"
                font.pixelSize: Math.min(13, Math.round(root.collapsedHeight * 0.45))
                font.weight: Font.DemiBold
                font.features: { "tnum": 1 }
            }

            // The weather, on the right while nothing else is there
            Text {
                anchors.right: parent.right
                anchors.rightMargin: 12
                anchors.verticalCenter: parent.verticalCenter
                visible: root.notch.weather !== null && !root.levelShown && !root.mediaShown && !root.activityShown && !root.conversationShown
                         && root.notch.sessions.count === 0 && !(root.notch.media !== null && root.notch.media.playing)
                text: root.notch.weatherIcon(root.notch.weather) + " " + (root.notch.weather ? Math.round(root.notch.weather.temperature) + "°" : "")
                color: "#c9ccd2"
                font.pixelSize: Math.min(12, Math.round(root.collapsedHeight * 0.42))
                font.weight: Font.DemiBold
            }

            // Music playing and no agent at work: its cover. Static, so it costs nothing.
            MediaCard.Cover {
                anchors.right: parent.right
                anchors.rightMargin: 10
                anchors.verticalCenter: parent.verticalCenter
                visible: !root.levelShown && !root.mediaShown && !root.activityShown && !root.conversationShown && root.notch.sessions.count === 0
                         && root.notch.media !== null && root.notch.media.playing
                size: Math.round(root.collapsedHeight * 0.62)
                url: visible ? root.notch.media.art_url : ""
            }

            Row {
                anchors.right: parent.right
                // Left of the pause button while a song is shown
                anchors.rightMargin: root.mediaShown ? 40 : 12
                anchors.verticalCenter: parent.verticalCenter
                spacing: 6
                visible: !root.levelShown && !root.conversationShown && root.notch.sessions.count > 0

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

            // Laid out at the width the island is heading for, not the width it has mid-way:
            // at the narrow width of the start, text wraps onto many lines and the height
            // the island aims for shot far down before coming back up.
            anchors.top: parent.top
            anchors.topMargin: 14
            anchors.horizontalCenter: parent.horizontalCenter
            width: root.expandedWidth - 28
            spacing: 10
            opacity: root.expanded ? 1 : 0
            visible: opacity > 0

            Behavior on opacity { NumberAnimation { duration: 220 } }

            // Tabs, only when the user opened the island
            Row {
                visible: root.opened && !root.targetsShown
                spacing: 6

                Repeater {
                    model: [
                        { id: "activity", label: qsTr("Activity") },
                        { id: "chat", label: qsTr("Chat") },
                        { id: "terminals", label: qsTr("Terminals") + "  " + root.notch.terminals.length },
                        { id: "shelf", label: qsTr("Shelf") + (root.notch.shelf.length ? "  " + root.notch.shelf.length : "") }
                    ].concat(root.settings.mailAddress ? [{ id: "mail", label: qsTr("Mail") + (root.notch.unreadMails ? "  " + root.notch.unreadMails : "") }] : [])
                     .concat(root.settings.phone ? [{ id: "phone", label: qsTr("Phone") + (root.notch.phone ? "  " + root.notch.phone.percent + "%" : "") }] : [])

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

            // Downloads, copies and scripts at work (and what just ended), then files being shared
            Repeater {
                model: root.targetsShown ? []
                     : (root.onActivity ? root.notch.activities : []).concat(root.notch.uploads.map(u => ({
                           key: "upload:" + u.id, kind: "upload", name: u.file, status: qsTr("Uploading"),
                           percent: u.progress * 100, speed: 0, processed: 0, total: 0, state: "running", path: ""
                       })))

                ProgressRow {
                    required property var modelData
                    width: expandedContent.width
                    activity: modelData
                    onOpen: path => Qt.openUrlExternally("file://" + encodeURI(path))
                    onReveal: path => root.bridge.showInFolder(path)
                }
            }

            // Who uses the microphone, camera or screen: "● Microphone: Brave"
            Flow {
                width: parent.width
                spacing: 14
                visible: root.onActivity && root.opened && root.notch.privacyInUse && root.settings.privacyDots && !root.targetsShown

                Repeater {
                    model: [
                        { apps: root.notch.privacy.microphone, label: qsTr("Microphone"), color: "#ff9f0a" },
                        { apps: root.notch.privacy.camera, label: qsTr("Camera"), color: "#30d158" },
                        { apps: root.notch.privacy.screen, label: qsTr("Screen recording"), color: "#bf5af2" }
                    ].filter(u => u.apps.length > 0)

                    Row {
                        required property var modelData
                        spacing: 6

                        Rectangle {
                            anchors.verticalCenter: parent.verticalCenter
                            width: 7
                            height: 7
                            radius: 3.5
                            color: parent.modelData.color
                        }
                        Text {
                            text: parent.modelData.label + ": " + parent.modelData.apps.join(", ")
                            color: "#c9ccd2"
                            font.pixelSize: 11
                        }
                    }
                }
            }

            Text {
                visible: root.choosing
                width: parent.width
                elide: Text.ElideMiddle
                text: qsTr("Where should %1 go?").arg(root.choosing ? decodeURIComponent(root.pendingUrls[0].split("/").pop()) : "")
                color: "#f2f3f5"
                font.pixelSize: 13
                font.weight: Font.DemiBold
            }

            DropTargets {
                id: dropTargets
                width: parent.width
                visible: root.targetsShown
                clickable: root.choosing
                onTileClicked: index => dropOn(index, root.pendingUrls)
                claudeTerminals: root.notch.claudeTerminals
                sharingOn: root.sharingOn
                onShelve: urls => root.dropped(() => root.shelve(urls))
                onSendToClaude: (terminalId, urls) => root.dropped(() => root.sendToClaude(terminalId, urls))
                onShare: urls => root.dropped(() => root.share(urls))
                onAttachToChat: urls => root.dropped(() => {
                    root.notch.addChatAttachments(root.notch.localPaths(urls))
                    root.tab = "chat"
                    root.opened = true
                })
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

            MailView {
                width: parent.width
                visible: root.shownTab === "mail"
                mails: root.notch.mails
                error: root.notch.mailError
                address: root.settings.mailAddress
                onRefresh: root.bridge.setMailAddress(root.settings.mailAddress)
                onOpen: mail => {
                    Qt.openUrlExternally(mail.link)
                    root.opened = false
                }
                onSummarize: mail => root.askAboutMail(qsTr("Summarize this email in a few sentences: what it says, what it asks of me, and whether it needs an answer. Read it with the Gmail tools (message id %1). From: %2 <%3>, subject: %4")
                                                       .arg(mail.id).arg(mail.from).arg(mail.address).arg(mail.subject))
                onDraft: mail => root.askAboutMail(qsTr("Write a reply to this email, in its language. Read the message and its thread with the Gmail tools first (message id %1), show me the reply here, and save it in Gmail as a draft with create_draft and replyToMessageId %1. Don't send it. From: %2 <%3>, subject: %4")
                                                   .arg(mail.id).arg(mail.from).arg(mail.address).arg(mail.subject))
            }

            PhoneView {
                width: parent.width
                visible: root.shownTab === "phone"
                phone: root.notch.phone
                log: root.notch.phoneLog
                onRing: device => root.bridge.ringPhone(device)
            }

            ChatView {
                id: chatView
                width: parent.width
                visible: root.shownTab === "chat"
                messages: root.notch.chat
                busy: root.notch.chatBusy
                mood: root.notch.faceMood
                attachments: root.notch.chatAttachments
                onAttached: paths => root.notch.addChatAttachments(paths)
                onDetached: path => root.notch.chatAttachments = root.notch.chatAttachments.filter(p => p !== path)
                voice: root.notch.voice
                onAsked: text => root.asked(text)
                mode: root.settings.chatMode
                onModeChosen: chosen => root.settings.chatMode = chosen
                onPlanApproved: {
                    root.settings.chatMode = root.settings.chatWorkMode
                    root.asked(qsTr("Carry out the plan."))
                }
                onListen: root.listenRequested()
                onListened: root.bridge.stopListening()
                onUnheard: root.conversationEnded()
                speaking: root.notch.speaking
                onStopped: {
                    root.conversationEnded()
                    root.bridge.stopChat()
                }
                onSilenced: root.bridge.stopSpeaking()
                onCleared: {
                    root.bridge.newChat()
                    root.notch.clearChat()
                }
                onCancelled: root.opened = false
            }

            TerminalsView {
                id: terminalsView
                width: parent.width
                visible: root.shownTab === "terminals"
                terminals: root.notch.terminals
                bridge: root.bridge
                onFocused: root.opened = false
            }

            // Usage of the plans; a click shows one in place of the headline.
            UsagePills {
                visible: root.onActivity && root.opened && !root.targetsShown && root.planItems.length > 0
                items: root.planItems
                notch: root.notch
                chosen: root.notch.planShown
                onToggled: key => root.notch.planShown = root.notch.planShown === key ? "" : key
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
                usage: {
                    root.notch.usageRevision // dependency
                    return root.notch.focusId ? root.notch.usage[root.notch.focusId] ?? null : null
                }
                onAnswered: decision => {
                    const requestId = root.notch.focus.permission.request_id
                    root.bridge.answerPermission(requestId, decision)
                    root.notch.closePermission(requestId)
                }
                onAnsweredWith: reply => {
                    const requestId = root.notch.focus.permission.request_id
                    root.bridge.answerPermissionWith(requestId, JSON.stringify(reply))
                    root.notch.closePermission(requestId)
                }
            }

            // The face and what's new (or a plan), with the agents' tiles beside it
            Item {
                id: headline

                readonly property real leftWidth: root.agentsShown ? width - agentGrid.width - 14 : width

                width: parent.width
                height: Math.max(left.implicitHeight, root.agentsShown ? agentGrid.implicitHeight : 0)
                visible: root.onActivity && !root.showCard

                Column {
                    id: left
                    width: headline.leftWidth
                    anchors.verticalCenter: parent.verticalCenter

                    PlanCard {
                        width: parent.width
                        visible: root.shownPlan !== null
                        plan: root.shownPlan ?? ({ key: "claude", label: "", name: "", windows: [], tokens: null, updated: 0 })
                        notch: root.notch
                        now: root.notch.now
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
                                width: headline.leftWidth - 90
                                elide: Text.ElideRight
                                text: root.notch.headline
                                color: "#f2f3f5"
                                font.pixelSize: 15
                                font.weight: Font.DemiBold
                            }
                            Text {
                                width: headline.leftWidth - 90
                                elide: Text.ElideRight
                                text: root.notch.subline || root.notch.time
                                color: "#8b909a"
                                font.pixelSize: 12
                            }
                        }
                    }
                }

                AgentGrid {
                    id: agentGrid
                    anchors.right: parent.right
                    anchors.verticalCenter: parent.verticalCenter
                    width: 300
                    visible: root.agentsShown
                    active: root.opened
                    bridge: root.bridge
                    notch: root.notch
                    folder: root.notch.focus?.cwd ?? ""
                    onLaunched: root.opened = false
                }
            }

            // The weather: "☀️ 19°  Clear · Kadıköy, İstanbul   ↑21° ↓15°"
            Row {
                visible: root.onActivity && root.opened && root.notch.weather !== null && !root.targetsShown
                spacing: 8

                readonly property var w: root.notch.weather ?? ({ place: "", temperature: 0, high: 0, low: 0, code: 0, is_day: true })

                Text {
                    anchors.verticalCenter: parent.verticalCenter
                    text: root.notch.weatherIcon(parent.w) + " " + Math.round(parent.w.temperature) + "°"
                    color: "#f2f3f5"
                    font.pixelSize: 13
                    font.weight: Font.DemiBold
                }
                Text {
                    anchors.verticalCenter: parent.verticalCenter
                    text: [root.notch.weatherText(parent.w.code), parent.w.place].filter(Boolean).join("  ·  ")
                    color: "#c9ccd2"
                    font.pixelSize: 12
                }
                Text {
                    anchors.verticalCenter: parent.verticalCenter
                    text: "↑" + Math.round(parent.w.high) + "°  ↓" + Math.round(parent.w.low) + "°"
                    color: "#8b909a"
                    font.pixelSize: 11
                    font.features: { "tnum": 1 }
                }
            }

            TimerRow {
                id: timerRow
                width: parent.width
                visible: root.onActivity && (root.opened || root.notch.timer !== null) && !root.targetsShown
                timer: root.notch.timer
                icon: root.notch.timerIcon
                tint: root.notch.timerColor
                now: root.notch.timerNow
                onStarted: (seconds, label) => root.notch.startTimer(seconds, label)
                onFocusStarted: root.notch.startFocus(root.notch.focusMinutes)
                onSkipped: root.notch.nextFocusPhase()
                onCancelled: root.notch.stopTimer()
                onExtended: seconds => root.notch.extendTimer(seconds)
            }

            MeetingRow {
                width: parent.width
                visible: root.onActivity && root.notch.nextMeeting !== null && (!root.showCard || root.opened)
                meeting: root.notch.nextMeeting ?? ({ title: "", start: 0, end: 0, link: "" })
                now: root.notch.now
            }

            // Under the session card too, when the user opened the island (not when it
            // peeks on its own to show activity).
            MediaCard {
                width: parent.width
                visible: root.onActivity && root.notch.media !== null && (!root.showCard || root.opened)
                player: root.notch.media ?? ({ bus_name: "", identity: "", title: "", artist: "", art_url: "",
                                               playing: false, can_go_next: false, can_go_previous: false })
                bridge: root.bridge
            }

            Column {
                width: parent.width
                visible: root.onActivity && root.notch.sessions.count > (root.showCard ? 1 : 0)

                Repeater {
                    model: root.notch.sessions

                    SessionRow {
                        width: expandedContent.width
                        agentLabel: root.notch.agentLabel(agent)
                        agentColor: root.notch.agentColor(agent)
                        // The focused session already has the card.
                        visible: !root.showCard || sessionId !== root.notch.focusId
                    }
                }
            }
        }
    }
}
