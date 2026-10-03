import QtQuick

// A timer: quick buttons to start one, or the running one with its countdown.
Column {
    id: row

    // NotchState.timer: { label, endsAt, total } (milliseconds), or null
    required property var timer
    // Milliseconds since the epoch, ticking every second while a timer runs
    required property real now

    // The field for a custom timer is open: the island takes the keyboard.
    property bool editing: false

    signal started(int seconds, string label)
    signal cancelled()
    signal extended(int seconds)

    readonly property real remaining: timer ? Math.max(0, timer.endsAt - now) : 0

    function format(ms) {
        const secs = Math.ceil(ms / 1000)
        const h = Math.floor(secs / 3600), m = Math.floor(secs % 3600 / 60), s = secs % 60
        const pad = n => String(n).padStart(2, "0")
        return h > 0 ? `${h}:${pad(m)}:${pad(s)}` : `${pad(m)}:${pad(s)}`
    }

    // "10", "25m", "1h30m" or "90s", then an optional label: "10 laundry"
    function parse(text) {
        const match = text.trim().match(/^((?:\d+\s*[hms]\s*)+|\d+)\s*(.*)$/i)
        if (!match)
            return null
        const spec = match[1].toLowerCase().replace(/\s/g, "")
        let seconds = 0
        if (/^\d+$/.test(spec)) {
            seconds = parseInt(spec) * 60
        } else {
            for (const part of spec.match(/\d+[hms]/g))
                seconds += parseInt(part) * { h: 3600, m: 60, s: 1 }[part.slice(-1)]
        }
        return seconds > 0 ? { seconds: seconds, label: match[2] } : null
    }

    spacing: 6

    // Not running: quick starts
    Row {
        visible: !row.timer && !row.editing
        spacing: 6

        Text {
            anchors.verticalCenter: parent.verticalCenter
            text: "⏱"
            color: "#c9ccd2"
            font.pixelSize: 13
        }
        Repeater {
            model: [5, 15, 25]

            PillButton {
                required property int modelData
                text: qsTr("%1 min").arg(modelData)
                tint: "#c9ccd2"
                onClicked: row.started(modelData * 60, "")
            }
        }
        PillButton {
            text: qsTr("Custom…")
            tint: "#c9ccd2"
            onClicked: row.editing = true
        }
    }

    SendField {
        width: row.width
        visible: row.editing
        monospace: false
        placeholder: qsTr("Time and label, e.g. 10 laundry or 1h30m focus (Esc cancels)")
        onVisibleChanged: if (visible) focusInput()
        onSubmitted: text => {
            const timer = row.parse(text)
            if (timer) {
                row.started(timer.seconds, timer.label)
                row.editing = false
            }
        }
        onCancelled: row.editing = false
    }

    // Running
    Item {
        visible: row.timer !== null
        width: row.width
        height: 34

        Text {
            id: countdown
            anchors.left: parent.left
            anchors.verticalCenter: parent.verticalCenter
            text: "⏱ " + row.format(row.remaining)
            color: row.remaining < 60000 ? "#ffb020" : "#f2f3f5"
            font.pixelSize: 16
            font.weight: Font.DemiBold
            font.features: { "tnum": 1 }
        }
        Column {
            anchors.left: countdown.right
            anchors.leftMargin: 14
            anchors.right: buttons.left
            anchors.rightMargin: 10
            anchors.verticalCenter: parent.verticalCenter
            spacing: 4

            Text {
                width: parent.width
                elide: Text.ElideRight
                text: row.timer ? (row.timer.label || qsTr("Timer")) + "  ·  "
                                  + qsTr("ends at %1").arg(Qt.formatTime(new Date(row.timer.endsAt), "HH:mm")) : ""
                color: "#8b909a"
                font.pixelSize: 11
            }
            Rectangle {
                width: parent.width
                height: 4
                radius: 2
                color: "#1affffff"

                Rectangle {
                    width: parent.width * (row.timer ? 1 - row.remaining / row.timer.total : 0)
                    height: parent.height
                    radius: 2
                    color: "#ffb020"
                }
            }
        }
        Row {
            id: buttons
            anchors.right: parent.right
            anchors.verticalCenter: parent.verticalCenter
            spacing: 4

            PillButton {
                text: qsTr("+5 min")
                tint: "#c9ccd2"
                onClicked: row.extended(300)
            }
            IconButton {
                glyph: "×"
                hint: qsTr("Cancel the timer")
                tint: "#8b909a"
                onClicked: row.cancelled()
            }
        }
    }
}
