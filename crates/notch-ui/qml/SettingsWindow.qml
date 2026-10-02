import QtQuick
import QtQuick.Controls
import QtQuick.Layouts

ApplicationWindow {
    id: win

    required property var settings

    title: qsTr("Notch Settings")
    width: 520
    height: 420
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

            Label { text: qsTr("Live activity") }
            Switch {
                text: qsTr("Briefly show diffs and command output")
                checked: win.settings.liveActivity
                onToggled: win.settings.liveActivity = checked
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
