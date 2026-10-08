import QtQuick

// One download, copy, upload or script at work, as in
//   (↓)  film.mkv                                              54%
//        ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
//        Downloading · 540 MB of 1.4 GB · 3.2 MB/s · 2 min left
// Once done, a file it wrote can be opened or shown in its folder.
Item {
    id: row

    // NotchState.activities item, or an upload in the same shape
    required property var activity

    signal open(string path)
    signal reveal(string path)

    readonly property bool done: activity.state === "done"
    readonly property bool failed: activity.state === "failed"
    readonly property bool paused: activity.state === "paused"
    readonly property color tint: failed ? "#ff6b6b" : done ? "#34c759" : paused ? "#8b909a"
                                : activity.kind === "script" ? "#bf8cff" : activity.kind === "upload" ? "#34c759"
                                : "#4c8dff"

    // "850 KB", "1.4 GB"
    function formatBytes(bytes) {
        const units = ["B", "KB", "MB", "GB", "TB"]
        let value = bytes, unit = 0
        while (value >= 1000 && unit < units.length - 1) {
            value /= 1000
            unit++
        }
        return (unit >= 2 && value < 10 ? value.toLocaleString(Qt.locale(), "f", 1) : Math.round(value)) + " " + units[unit]
    }

    // "45 s left", "3 min left", "1 h 20 min left"
    function formatLeft(seconds) {
        const s = Math.ceil(seconds)
        const text = s < 60 ? qsTr("%1 s").arg(s)
                   : s < 3600 ? qsTr("%1 min").arg(Math.ceil(s / 60))
                   : qsTr("%1 h %2 min").arg(Math.floor(s / 3600)).arg(Math.floor(s % 3600 / 60))
        return qsTr("%1 left").arg(text)
    }

    readonly property string details: {
        const a = activity
        if (failed)
            return [a.status, qsTr("Failed")].filter(Boolean).join("  ·  ")
        if (done)
            return a.total > 0 ? qsTr("Done") + "  ·  " + formatBytes(a.total) : (a.status || qsTr("Done"))
        const parts = [paused ? qsTr("Paused") : a.status]
        if (a.total > 0)
            parts.push(qsTr("%1 of %2").arg(formatBytes(a.processed)).arg(formatBytes(a.total)))
        if (!paused && a.speed > 0) {
            parts.push(formatBytes(a.speed) + "/s")
            if (a.total > a.processed)
                parts.push(formatLeft((a.total - a.processed) / a.speed))
        }
        return parts.filter(Boolean).join("  ·  ")
    }

    implicitHeight: 40

    ProgressRing {
        id: ring
        anchors.left: parent.left
        anchors.verticalCenter: parent.verticalCenter
        size: 30
        percent: row.done ? 100 : row.failed ? 100 : row.activity.percent
        tint: row.tint
        glyph: row.done ? "✓" : row.failed ? "!" : row.paused ? "‖"
             : row.activity.kind === "download" ? "↓" : row.activity.kind === "upload" ? "↑"
             : row.activity.kind === "copy" ? "→" : "⚙"
    }

    Text {
        id: name
        anchors.left: ring.right
        anchors.leftMargin: 10
        anchors.right: trailing.left
        anchors.rightMargin: 10
        anchors.top: parent.top
        anchors.topMargin: 1
        elide: Text.ElideMiddle
        text: row.activity.name
        color: "#f2f3f5"
        font.pixelSize: 12
        font.weight: Font.DemiBold
    }

    // The bar, only while it runs
    Rectangle {
        id: track
        anchors.left: name.left
        anchors.right: name.right
        anchors.verticalCenter: parent.verticalCenter
        height: 3
        radius: 1.5
        visible: !row.done && !row.failed && row.activity.percent >= 0
        color: Qt.alpha(row.tint, 0.2)

        Rectangle {
            width: parent.width * Math.min(100, Math.max(0, row.activity.percent)) / 100
            height: parent.height
            radius: parent.radius
            color: row.tint

            Behavior on width { NumberAnimation { duration: 250 } }
        }
    }

    Text {
        anchors.left: name.left
        anchors.right: name.right
        anchors.bottom: parent.bottom
        anchors.bottomMargin: 1
        elide: Text.ElideRight
        text: row.details
        color: row.failed ? "#ff6b6b" : "#8b909a"
        font.pixelSize: 11
        font.features: { "tnum": 1 }
    }

    // The percentage, or what to do with the finished file
    Item {
        id: trailing
        anchors.right: parent.right
        anchors.verticalCenter: parent.verticalCenter
        width: actions.visible ? actions.width : percent.visible ? Math.max(36, percent.implicitWidth) : 0
        height: parent.height

        Text {
            id: percent
            anchors.right: parent.right
            anchors.verticalCenter: parent.verticalCenter
            visible: !row.done && !row.failed && row.activity.percent >= 0
            text: Math.floor(row.activity.percent) + "%"
            color: "#f2f3f5"
            font.pixelSize: 15
            font.weight: Font.DemiBold
            font.features: { "tnum": 1 }
        }

        Row {
            id: actions
            anchors.right: parent.right
            anchors.verticalCenter: parent.verticalCenter
            spacing: 6
            visible: row.done && row.activity.path !== ""

            PillButton {
                text: qsTr("Open")
                tint: "#34c759"
                onClicked: row.open(row.activity.path)
            }
            PillButton {
                text: qsTr("Show in folder")
                onClicked: row.reveal(row.activity.path)
            }
        }
    }
}
