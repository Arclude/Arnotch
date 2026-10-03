import QtQuick

// The Claude plan's rate limits: how much of the 5-hour and 7-day windows is used,
// and when each resets.
Row {
    id: limits

    // NotchState.limits: { five_hour: { used_percent, resets_at }, seven_day: {...} }
    required property var windows
    // Milliseconds since the epoch, ticking once a minute
    required property real now

    spacing: 16

    function color(used) {
        return used >= 95 ? "#ff5f57" : used >= 80 ? "#ffb020" : "#4c8dff"
    }

    function resetText(resetsAt) {
        const at = new Date(resetsAt * 1000)
        const today = new Date(now).toDateString() === at.toDateString()
        return today ? Qt.formatTime(at, "HH:mm") : at.toLocaleString(Qt.locale(), "ddd HH:mm")
    }

    Repeater {
        model: [
            { key: "five_hour", label: qsTr("5 h") },
            { key: "seven_day", label: qsTr("7 days") }
        ]

        Item {
            id: item

            required property var modelData
            readonly property var window: limits.windows ? limits.windows[modelData.key] ?? null : null

            visible: window !== null
            width: (limits.width - limits.spacing) / 2
            height: 30

            Text {
                id: label
                anchors.left: parent.left
                anchors.top: parent.top
                text: item.modelData.label
                color: "#c9ccd2"
                font.pixelSize: 11
                font.weight: Font.DemiBold
            }
            Text {
                anchors.right: parent.right
                anchors.top: parent.top
                text: item.window ? qsTr("%1% · resets %2").arg(Math.round(item.window.used_percent))
                                                          .arg(limits.resetText(item.window.resets_at)) : ""
                color: "#8b909a"
                font.pixelSize: 10
                font.features: { "tnum": 1 }
            }
            Rectangle {
                anchors.left: parent.left
                anchors.right: parent.right
                anchors.bottom: parent.bottom
                anchors.bottomMargin: 4
                height: 5
                radius: 2.5
                color: "#1affffff"

                Rectangle {
                    width: parent.width * Math.min(1, (item.window ? item.window.used_percent : 0) / 100)
                    height: parent.height
                    radius: parent.radius
                    color: limits.color(item.window ? item.window.used_percent : 0)
                }
            }
        }
    }
}
