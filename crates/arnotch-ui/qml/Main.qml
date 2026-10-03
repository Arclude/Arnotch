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
    }

    property NotchState state: NotchState {
        // The shelf survives restarts.
        Component.onCompleted: {
            try {
                shelf = JSON.parse(app.settings.shelf)
            } catch (e) {
                shelf = []
            }
        }
        onShelfChanged: app.settings.shelf = JSON.stringify(shelf)

        // An answer that finished while nobody looks at the chat gets a short note.
        onChatFinished: (status, body) => {
            for (let i = 0; i < app.islands.count; i++) {
                const island = app.islands.objectAt(i)
                if (island.expanded && island.shownTab === "chat")
                    return
            }
            const firstLine = body.split("\n").find(line => line.trim() !== "") ?? ""
            if (status === "done")
                app.state.say(qsTr("Claude answered"), firstLine.replace(/[*_`#>]/g, "").trim(), 6000)
            else
                app.state.say(qsTr("Claude couldn't answer"), firstLine, 8000)
            app.state.notifying = true
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
            else if (ev.type === "share")
                app.bridge.shareFile(ev.path, app.settings.shareService, app.settings.shareCommand)
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
        }
    }

    property Window settingsWindow: null

    function openIslands(tab) {
        for (let i = 0; i < islands.count; i++) {
            const island = islands.objectAt(i)
            if (tab)
                island.tab = tab
            island.opened = true
        }
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
        for (let i = 0; i < islands.count; i++)
            islands.objectAt(i).opened = false
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
