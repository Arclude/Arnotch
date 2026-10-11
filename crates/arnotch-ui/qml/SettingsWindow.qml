import QtQuick
import QtQuick.Controls
import QtQuick.Layouts

ApplicationWindow {
    id: win

    required property var settings
    // NotchState: what the settings offer to choose from (the microphones)
    required property var notch
    readonly property bool sharingOn: settings.shareService !== "off"

    title: qsTr("arnotch Settings")
    width: 560
    height: 640
    minimumWidth: 420
    minimumHeight: 300

    function screenOptions() {
        const options = [
            { text: qsTr("Active screen (follows focus)"), value: "active" },
            { text: qsTr("All screens"), value: "all" }
        ]
        for (const s of Qt.application.screens) {
            const name = [s.manufacturer, s.model].filter(Boolean).join(" ")
            options.push({ text: `${s.name}${name ? " — " + name : ""} (${s.width}×${s.height})`, value: s.name })
        }
        if (!options.some(o => o.value === settings.screen))
            options.push({ text: qsTr("%1 (disconnected)").arg(settings.screen), value: settings.screen })
        return options
    }

    ScrollView {
        id: scroll
        anchors.fill: parent
        contentWidth: availableWidth

        ColumnLayout {
            width: scroll.availableWidth - 40
            x: 20
            y: 20
            spacing: 16

            GridLayout {
                Layout.fillWidth: true
                columns: 2
                columnSpacing: 16
                rowSpacing: 12

                Label { text: qsTr("Show on") }
                ComboBox {
                    id: screenBox
                    Layout.fillWidth: true
                    textRole: "text"
                    valueRole: "value"
                    model: win.screenOptions()
                    onModelChanged: currentIndex = indexOfValue(win.settings.screen)
                    Component.onCompleted: currentIndex = indexOfValue(win.settings.screen)
                    onActivated: win.settings.screen = currentValue

                    Connections {
                        target: Qt.application
                        function onScreensChanged() { screenBox.model = win.screenOptions() }
                    }
                }

                Label { text: qsTr("Top panel") }
                ComboBox {
                    Layout.fillWidth: true
                    textRole: "text"
                    valueRole: "value"
                    model: [
                        { text: qsTr("Over the panel (replaces its center)"), value: "over" },
                        { text: qsTr("Below the panel"), value: "below" }
                    ]
                    Component.onCompleted: currentIndex = indexOfValue(win.settings.panelMode)
                    onActivated: win.settings.panelMode = currentValue
                }

                Label { text: qsTr("Height") }
                RowLayout {
                    SpinBox {
                        from: 20
                        to: 48
                        value: win.settings.height
                        onValueModified: win.settings.height = value
                    }
                    Label {
                        opacity: 0.7
                        text: qsTr("px, match your top panel")
                    }
                }

                Label { text: qsTr("Clock") }
                Switch {
                    text: qsTr("Show the time in the island")
                    checked: win.settings.showClock
                    onToggled: win.settings.showClock = checked
                }

                Label { text: qsTr("Events") }
                Switch {
                    text: qsTr("Open the island on new events")
                    checked: win.settings.autoExpand
                    onToggled: win.settings.autoExpand = checked
                }

                Label { text: qsTr("Permissions") }
                Switch {
                    text: qsTr("Answer Claude's permission requests here")
                    checked: win.settings.permissionPrompts
                    onToggled: win.settings.permissionPrompts = checked
                }

                Label { text: qsTr("Sharing") }
                ComboBox {
                    Layout.fillWidth: true
                    textRole: "text"
                    valueRole: "value"
                    model: [
                        { text: qsTr("Off"), value: "off" },
                        { text: qsTr("0x0.st (kept 30 days to 1 year)"), value: "0x0" },
                        { text: qsTr("catbox.moe (permanent)"), value: "catbox" },
                        { text: qsTr("litterbox (deleted after 24 hours)"), value: "litterbox" },
                        { text: qsTr("Custom command"), value: "custom" }
                    ]
                    Component.onCompleted: currentIndex = indexOfValue(win.settings.shareService)
                    onActivated: win.settings.shareService = currentValue
                }

                Item { visible: win.settings.shareService === "custom"; implicitWidth: 1 }
                TextField {
                    Layout.fillWidth: true
                    visible: win.settings.shareService === "custom"
                    placeholderText: "curl -F file=@{file} https://example.com"
                    text: win.settings.shareCommand
                    onEditingFinished: win.settings.shareCommand = text
                }

                Item { visible: win.sharingOn; implicitWidth: 1 }
                Label {
                    Layout.fillWidth: true
                    visible: win.sharingOn
                    wrapMode: Text.WordWrap
                    opacity: 0.7
                    font.pixelSize: 11
                    text: qsTr("Shared files are uploaded to this service and anyone with the link can download them.")
                }

                Label { text: qsTr("Live activity") }
                Switch {
                    text: qsTr("Briefly show diffs and command output")
                    checked: win.settings.liveActivity
                    onToggled: win.settings.liveActivity = checked
                }

                Label { text: qsTr("Calendar") }
                TextField {
                    Layout.fillWidth: true
                    // The address gives access to the calendar: shown only while edited.
                    echoMode: TextInput.PasswordEchoOnEdit
                    placeholderText: qsTr("Secret iCal address (.ics)")
                    text: win.settings.calendarUrl
                    onEditingFinished: win.settings.calendarUrl = text.trim()
                }

                Label { text: qsTr("Weather") }
                TextField {
                    Layout.fillWidth: true
                    placeholderText: qsTr("City, e.g. İstanbul or Kadıköy, İstanbul")
                    text: win.settings.weatherCity
                    onEditingFinished: win.settings.weatherCity = text.trim()
                }

                Label { text: qsTr("Focus") }
                ColumnLayout {
                    RowLayout {
                        SpinBox {
                            from: 5
                            to: 120
                            stepSize: 5
                            value: win.settings.focusMinutes
                            onValueModified: win.settings.focusMinutes = value
                        }
                        Label { text: qsTr("min focus") }
                    }
                    RowLayout {
                        SpinBox {
                            from: 1
                            to: 30
                            value: win.settings.breakMinutes
                            onValueModified: win.settings.breakMinutes = value
                        }
                        Label { text: qsTr("min break") }
                        SpinBox {
                            from: 5
                            to: 60
                            stepSize: 5
                            value: win.settings.longBreakMinutes
                            onValueModified: win.settings.longBreakMinutes = value
                        }
                        Label { text: qsTr("min every 4th") }
                    }
                    Switch {
                        text: qsTr("Hold notifications while focusing")
                        checked: win.settings.focusSilence
                        onToggled: win.settings.focusSilence = checked
                    }
                }

                Label { text: qsTr("Chat") }
                Column {
                    Switch {
                        text: qsTr("Claude can search the web")
                        checked: win.settings.chatWeb
                        onToggled: win.settings.chatWeb = checked
                    }
                    Switch {
                        text: qsTr("Claude can read files on this computer")
                        checked: win.settings.chatFiles
                        onToggled: win.settings.chatFiles = checked
                    }
                    Switch {
                        text: qsTr("Claude can open apps and control the computer")
                        checked: win.settings.computerControl
                        onToggled: win.settings.computerControl = checked
                    }
                    Label {
                        leftPadding: 8
                        wrapMode: Text.WordWrap
                        width: 320
                        opacity: 0.7
                        font.pixelSize: 11
                        visible: win.settings.computerControl
                        text: qsTr("Opening apps, music, volume and brightness happen right away; other commands wait for your Allow in the island.")
                    }
                    Switch {
                        text: qsTr("Claude can change files")
                        checked: win.settings.chatEdit
                        onToggled: win.settings.chatEdit = checked
                    }
                    Label {
                        leftPadding: 8
                        wrapMode: Text.WordWrap
                        width: 320
                        opacity: 0.7
                        font.pixelSize: 11
                        visible: win.settings.chatEdit
                        text: qsTr("Each change waits for your Allow in the island; the chat shows what changed.")
                    }
                }

                Label { text: qsTr("Model") }
                ColumnLayout {
                    Layout.fillWidth: true
                    ComboBox {
                        Layout.fillWidth: true
                        textRole: "text"
                        valueRole: "value"
                        model: [
                            { text: qsTr("Automatic, for each message"), value: "auto" },
                            { text: "Haiku", value: "haiku" },
                            { text: "Sonnet", value: "sonnet" },
                            { text: "Opus", value: "opus" }
                        ]
                        Component.onCompleted: currentIndex = indexOfValue(win.settings.chatModel)
                        onActivated: win.settings.chatModel = currentValue
                    }
                    Label {
                        Layout.fillWidth: true
                        wrapMode: Text.WordWrap
                        opacity: 0.7
                        font.pixelSize: 11
                        visible: win.settings.chatModel === "auto"
                        text: qsTr("Commands and small talk go to Haiku, questions and research to Sonnet, code and analysis to Opus.")
                    }
                    Switch {
                        text: qsTr("Also for Claude Code in terminals")
                        checked: win.settings.terminalModel === "auto"
                        onToggled: win.settings.terminalModel = checked ? "auto" : "off"
                    }
                    Label {
                        Layout.fillWidth: true
                        wrapMode: Text.WordWrap
                        opacity: 0.7
                        font.pixelSize: 11
                        visible: win.settings.terminalModel === "auto"
                        text: qsTr("A prompt for another model is held back, the session switches (/model, /effort) and the prompt is typed again. Sessions go up at once, but down only while short or after three light prompts: switching drops the cache. Needs a terminal arnotch can type into.")
                    }
                }

                Label { text: qsTr("Notes") }
                ColumnLayout {
                    Layout.fillWidth: true
                    TextField {
                        Layout.fillWidth: true
                        placeholderText: qsTr("Obsidian vault folder")
                        text: win.settings.obsidianVault
                        onEditingFinished: win.settings.obsidianVault = text.trim()
                    }
                    Label {
                        Layout.fillWidth: true
                        wrapMode: Text.WordWrap
                        opacity: 0.7
                        font.pixelSize: 11
                        text: qsTr("Claude writes what it researches there, in Araştırmalar (needs Node.js for npx).")
                    }
                }

                Label { text: qsTr("Read aloud") }
                ComboBox {
                    Layout.fillWidth: true
                    textRole: "text"
                    valueRole: "value"
                    model: [
                        { text: qsTr("Answers to questions asked by voice"), value: "voice" },
                        { text: qsTr("Every answer"), value: "always" },
                        { text: qsTr("Off"), value: "off" }
                    ]
                    Component.onCompleted: currentIndex = indexOfValue(win.settings.speakAnswers)
                    onActivated: win.settings.speakAnswers = currentValue
                }

                Label { text: qsTr("Microphone") }
                ColumnLayout {
                    Layout.fillWidth: true
                    ComboBox {
                        id: microphone
                        Layout.fillWidth: true
                        textRole: "text"
                        valueRole: "value"
                        model: {
                            const choices = [
                                { text: qsTr("Automatic: a headset's when plugged in"), value: "" },
                                { text: qsTr("The system's default"), value: "default" }
                            ]
                            for (const input of win.notch.microphones)
                                choices.push({ text: input.description, value: input.name })
                            // Chosen but unplugged: still shown, so the choice isn't lost.
                            if (!choices.some(c => c.value === win.settings.microphone))
                                choices.push({ text: qsTr("%1 (not plugged in)").arg(win.settings.microphone),
                                               value: win.settings.microphone })
                            return choices
                        }
                        onModelChanged: currentIndex = indexOfValue(win.settings.microphone)
                        Component.onCompleted: currentIndex = indexOfValue(win.settings.microphone)
                        onActivated: win.settings.microphone = currentValue
                    }
                    Label {
                        Layout.fillWidth: true
                        wrapMode: Text.WordWrap
                        opacity: 0.7
                        font.pixelSize: 11
                        text: qsTr("For asking by voice and the wake word. A laptop's own microphone hears the room and the speakers too; a headset's is understood best.")
                    }
                }

                Label { text: qsTr("Wake word") }
                ColumnLayout {
                    Layout.fillWidth: true
                    Switch {
                        text: qsTr("Wake arnotch by saying its name")
                        checked: win.settings.wakeOn
                        onToggled: win.settings.wakeOn = checked
                    }
                    TextField {
                        Layout.fillWidth: true
                        visible: win.settings.wakeOn
                        placeholderText: "Claude"
                        text: win.settings.wakeWord
                        onEditingFinished: win.settings.wakeWord = text.trim() || "Claude"
                    }
                    Label {
                        Layout.fillWidth: true
                        wrapMode: Text.WordWrap
                        opacity: 0.7
                        font.pixelSize: 11
                        visible: win.settings.wakeOn
                        text: qsTr("Say the name, then your question after the tone, or both at once (\"Claude, what's the weather?\"). The microphone stays on, so arnotch shows in the privacy dots; what it hears is turned into text on this computer and nothing is kept. Needs a speech model; base is the quickest: arnotchctl voice-model base")
                    }
                }

                Label { text: qsTr("Phone") }
                Switch {
                    text: qsTr("Calls, messages and battery (KDE Connect)")
                    checked: win.settings.phone
                    onToggled: win.settings.phone = checked
                }

                Label { text: qsTr("Music") }
                Switch {
                    text: qsTr("Show the playing song in the closed island")
                    checked: win.settings.compactMedia
                    onToggled: win.settings.compactMedia = checked
                }

                Label { text: qsTr("Privacy") }
                Switch {
                    text: qsTr("Show when apps use the microphone, camera or screen")
                    checked: win.settings.privacyDots
                    onToggled: win.settings.privacyDots = checked
                }

                Label { text: qsTr("Indicators") }
                Switch {
                    text: qsTr("Show volume, brightness and battery changes")
                    checked: win.settings.indicators
                    onToggled: win.settings.indicators = checked
                }
            }

            Label {
                Layout.fillWidth: true
                Layout.bottomMargin: 20
                wrapMode: Text.WordWrap
                opacity: 0.7
                text: qsTr("Tip: with a centered clock in your top panel, use “Over the panel” and remove the panel's clock widget, the island shows the time instead.")
            }
        }
    }
}
