import QtQuick
import QtQuick.Controls
import QtQuick.Layouts

ApplicationWindow {
    id: win

    required property var settings
    readonly property bool sharingOn: settings.shareService !== "off"

    title: qsTr("arnotch Settings")
    width: 520
    height: 560
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

    ColumnLayout {
        anchors.fill: parent
        anchors.margins: 20
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
            }

            Label { text: qsTr("Music") }
            Switch {
                text: qsTr("Show the playing song in the closed island")
                checked: win.settings.compactMedia
                onToggled: win.settings.compactMedia = checked
            }

            Label { text: qsTr("Indicators") }
            Switch {
                text: qsTr("Show volume, brightness and battery changes")
                checked: win.settings.indicators
                onToggled: win.settings.indicators = checked
            }
        }

        Item { Layout.fillHeight: true }

        Label {
            Layout.fillWidth: true
            wrapMode: Text.WordWrap
            opacity: 0.7
            text: qsTr("Tip: with a centered clock in your top panel, use “Over the panel” and remove the panel's clock widget, the island shows the time instead.")
        }
    }
}
