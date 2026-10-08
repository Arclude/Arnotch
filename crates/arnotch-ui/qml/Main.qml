import QtQuick
import QtCore
import Qt.labs.platform as Platform
import dev.arnotch

// Application root: owns the shared state, settings and tray icon,
// and creates one island window per selected screen.
QtObject {
    id: app

    property Settings settings: Settings {
        // "active" (follows focus), "all", or a screen name such as "HDMI-A-1"
        property string screen: "active"
        // "over": sit on top of a top panel, "below": stay under it
        property string panelMode: "over"
        property bool showClock: true
        // Height of the closed island; best set to the top panel's height
        property int height: 30
        property bool autoExpand: true
        // Answer Claude Code's permission requests from the island
        property bool permissionPrompts: true
        // Sharing service for dropped files: "off", "0x0", "catbox", "litterbox" or "custom"
        property string shareService: "off"
        // For "custom": a shell command, {file} is the quoted path, prints the link
        property string shareCommand: ""
        // Paths on the shelf, as a JSON array
        property string shelf: "[]"
        // Briefly open the island to show diffs and command output
        property bool liveActivity: true
        // Show volume, brightness and battery changes in the island
        property bool indicators: true
        // The running timer as JSON ({ label, endsAt, total }), empty when none
        property string timer: ""
        // Calls, messages and the battery of the phone (KDE Connect)
        property bool phone: true
        // Chat: let Claude search the web, and read files on this computer (read-only)
        property bool chatWeb: true
        property bool chatFiles: false
        // Show the playing song in the closed island (instead of the clock)
        property bool compactMedia: true
        // Dots on the island while apps use the microphone, the camera or record the screen
        property bool privacyDots: true
        // Read Claude's answers aloud: "voice" (questions asked by voice), "always" or "off"
        property string speakAnswers: "voice"
        // Wake arnotch by saying its name: the microphone stays on, listened to on this computer
        property bool wakeOn: false
        property string wakeWord: "Claude"
        onWakeOnChanged: app.updateWakeWord()
        onWakeWordChanged: app.updateWakeWord()
        // Let Claude in the chat open apps, play music, change the volume, ... (commands
        // that change more are asked first)
        property bool computerControl: true
        // Let Claude in the chat change files; each change waits for an Allow in the island,
        // and the chat shows the diff once it is made
        property bool chatEdit: true
        // The chat's model: "auto" (Haiku, Sonnet or Opus, chosen for each message), or
        // "haiku", "sonnet", "opus" for all of them
        property string chatModel: "auto"
        // "auto": also pick the model of Claude Code in terminals, for each prompt
        // (arnotchctl claude-hook reads it from this file); "off" leaves them be
        property string terminalModel: "off"
        // The Obsidian vault Claude writes its research to; empty for none
        property string obsidianVault: ""
        // The calendar's iCalendar (ICS) address, a secret; empty for no calendar
        property string calendarUrl: ""
        onCalendarUrlChanged: app.bridge.setCalendarUrl(calendarUrl)
        // Focus (Pomodoro): minutes of work, of a break, and of the long break after every
        // fourth round; and whether notifications wait (the desktop's too) while focusing
        property int focusMinutes: 25
        property int breakMinutes: 5
        property int longBreakMinutes: 15
        property bool focusSilence: true
        // City for the weather, e.g. "İstanbul" or "Kadıköy, İstanbul"; empty for none
        property string weatherCity: ""
        onWeatherCityChanged: app.bridge.setWeatherCity(weatherCity)
    }

    // The desktop's notifications are quiet while focusing.
    readonly property bool doNotDisturb: state.quiet
    onDoNotDisturbChanged: bridge.setDoNotDisturb(doNotDisturb)

    property NotchState state: NotchState {
        indicators: app.settings.indicators
        phoneAlerts: app.settings.phone
        focusMinutes: app.settings.focusMinutes
        breakMinutes: app.settings.breakMinutes
        longBreakMinutes: app.settings.longBreakMinutes
        quietFocus: app.settings.focusSilence
        // The shelf survives restarts.
        Component.onCompleted: {
            try {
                shelf = JSON.parse(app.settings.shelf)
            } catch (e) {
                shelf = []
            }
            try {
                if (app.settings.timer)
                    timer = JSON.parse(app.settings.timer)
            } catch (e) {
                timer = null
            }
        }
        onShelfChanged: app.settings.shelf = JSON.stringify(shelf)

        // The timer survives restarts; one that ran out meanwhile still rings once.
        onTimerChanged: app.settings.timer = timer ? JSON.stringify(timer) : ""
        onTimerFinished: app.bridge.playSound("alarm-clock-elapsed")

        // An answer that finished while nobody looks at the chat gets a short note.
        onChatFinished: (status, body) => {
            // ...or once it is written, when it isn't read aloud (that ends with speechEnded).
            if (app.state.conversing) {
                if (status !== "done")
                    app.state.conversing = false
                // (Reading may have ended first, when it couldn't start.)
                else if (!app.speakingTurn || app.spokenTurn === app.state.chatTurn)
                    app.bridge.startListening()
                return
            }
            for (let i = 0; i < app.islands.count; i++) {
                const island = app.islands.objectAt(i)
                if (island.expanded && island.shownTab === "chat")
                    return
            }
            const firstLine = body.split("\n").find(line => line.trim() !== "") ?? ""
            const shown = status === "done"
                ? app.state.say(qsTr("Claude answered"), firstLine.replace(/[*_`#>]/g, "").trim(), 6000, true)
                : app.state.say(qsTr("Claude couldn't answer"), firstLine, 8000, true)
            if (shown)
                app.state.notifying = true
        }

        // Asked by voice: the question goes to Claude like a typed one. An answer that isn't
        // read aloud is shown.
        onVoiceAsked: text => {
            if (app.state.endsConversation(text)) {
                app.endConversation()
                return
            }
            if (!app.askClaude(text, true))
                app.openIslands("chat")
        }

        // In a conversation, Claude listens again once it has said its answer...
        onSpeechEnded: turn => {
            app.spokenTurn = turn
            if (app.state.conversing && !app.state.chatBusy)
                app.bridge.startListening()
        }

        onPermissionRequested: requestId => {
            if (!app.settings.permissionPrompts) {
                app.bridge.answerPermission(requestId, "ask")
                return
            }
            for (let i = 0; i < app.islands.count; i++)
                app.islands.objectAt(i).tab = "activity"
        }
    }

    property NotchBridge bridge: NotchBridge {
        Component.onCompleted: {
            setCalendarUrl(app.settings.calendarUrl)
            setWeatherCity(app.settings.weatherCity)
            setDoNotDisturb(app.doNotDisturb)
            app.updateWakeWord()
        }
        onEventReceived: json => {
            const ev = JSON.parse(json)
            if (ev.type === "open_settings")
                app.openSettings()
            else if (ev.type === "open")
                app.openIslands(ev.tab)
            else if (ev.type === "toggle")
                app.toggleIslands(ev.tab)
            else if (ev.type === "close")
                app.closeIslands()
            else if (ev.type === "shelve")
                app.state.addToShelf(ev.paths)
            else if (ev.type === "pick")
                app.pick(ev.paths)
            else if (ev.type === "share")
                app.bridge.shareFile(ev.path, app.settings.shareService, app.settings.shareCommand)
            else if (ev.type === "listen")
                app.listen(ev.action)
            else if (ev.type === "wake")
                app.wake(ev.question ?? "")
            else
                app.state.handleEvent(ev)
        }
    }

    // `null` lets the compositor place the island on the active screen.
    readonly property var islandScreens: {
        const screens = Qt.application.screens
        if (settings.screen === "all")
            return screens
        if (settings.screen === "active")
            return [null]
        // A disconnected monitor falls back to the active screen.
        return [screens.find(s => s.name === settings.screen) ?? null]
    }

    property Instantiator islands: Instantiator {
        model: app.islandScreens

        Island {
            required property var modelData

            targetScreen: modelData
            notch: app.state
            settings: app.settings
            bridge: app.bridge
            onSettingsRequested: app.openSettings()
            onAsked: text => app.askClaude(text, false)
            onListenRequested: app.listen("start")
            onConversationEnded: app.endConversation()
        }
    }

    property Window settingsWindow: null

    // Whether the current answer is read aloud, and the last turn read to the end
    property bool speakingTurn: false
    property int spokenTurn: -1

    // Sends a message to Claude in the chat, with the files attached to it. The answer is
    // read aloud as the settings say; returns whether it is.
    function askClaude(text, byVoice) {
        const files = state.chatAttachments
        const speak = settings.speakAnswers === "always" || (settings.speakAnswers === "voice" && byVoice)
        speakingTurn = speak
        const turn = bridge.askClaude(text, files.join("\n"), settings.chatWeb, settings.chatFiles,
                                      settings.computerControl, settings.chatEdit, speak,
                                      settings.chatModel, settings.obsidianVault)
        state.startChatTurn(turn, text, files)
        state.chatAttachments = []
        return speak
    }

    // Ends talking by voice: no more listening or speaking.
    function endConversation() {
        state.conversing = false
        bridge.cancelListening()
        bridge.stopSpeaking()
        state.voice = null
    }

    // A keyboard shortcut or script: "start", "stop", "toggle" or "cancel" listening. Toggling
    // while Claude speaks interrupts it to listen.
    function listen(action) {
        if (action === "cancel") {
            endConversation()
        } else if (action === "stop" || (action === "toggle" && state.listening)) {
            bridge.stopListening()
        } else if (state.voice === null && !(state.conversing && state.chatBusy && !state.speaking)) {
            state.conversing = true
            bridge.startListening()
            // Heard from anywhere, even with the island out of sight.
            bridge.playSound("message-new-instant")
            bridge.notifyDesktop("listening", qsTr("Listening"), qsTr("Ask your question; a pause sends it."),
                                 "audio-input-microphone")
        }
    }

    function updateWakeWord() {
        bridge.setWakeWord(settings.wakeOn ? settings.wakeWord : "")
    }

    // The wake word was heard: listen for the question, or ask the one said along with it.
    function wake(question) {
        if (question) {
            bridge.playSound("message-new-instant")
            state.conversing = true
            state.voiceAsked(question)
        } else {
            listen("start")
        }
    }

    function openIslands(tab) {
        for (let i = 0; i < islands.count; i++) {
            const island = islands.objectAt(i)
            if (tab)
                island.tab = tab
            island.opened = true
        }
    }

    // Shows the drop targets on the islands for the files to be sent somewhere.
    function pick(paths) {
        const urls = paths.map(path => "file://" + encodeURI(path))
        for (let i = 0; i < islands.count; i++)
            islands.objectAt(i).choose(urls)
    }

    function toggleIslands(tab) {
        for (let i = 0; i < islands.count; i++) {
            const island = islands.objectAt(i)
            if (island.opened && (!tab || island.tab === tab)) {
                closeIslands()
                return
            }
        }
        openIslands(tab)
    }

    function closeIslands() {
        for (let i = 0; i < islands.count; i++) {
            islands.objectAt(i).opened = false
            islands.objectAt(i).pendingUrls = []
        }
        state.dismiss()
    }

    function openSettings() {
        // Created on demand so Qt Quick Controls is only loaded when the window is used.
        if (!settingsWindow)
            settingsWindow = Qt.createComponent("SettingsWindow.qml").createObject(null, { settings: app.settings })
        settingsWindow.show()
        settingsWindow.raise()
        settingsWindow.requestActivate()
    }

    property Platform.SystemTrayIcon tray: Platform.SystemTrayIcon {
        visible: true
        icon.source: "qrc:/icons/arnotch.svg"
        tooltip: "arnotch"
        onActivated: reason => {
            if (reason === Platform.SystemTrayIcon.Trigger)
                app.openSettings()
        }

        menu: Platform.Menu {
            Platform.MenuItem {
                text: qsTr("Settings…")
                onTriggered: app.openSettings()
            }
            Platform.MenuItem {
                text: qsTr("Send a test notification")
                onTriggered: app.state.say(qsTr("Hello!"), qsTr("arnotch is working"), 4000)
            }
            Platform.MenuSeparator {}
            Platform.MenuItem {
                text: qsTr("Quit")
                onTriggered: Qt.quit()
            }
        }
    }
}
