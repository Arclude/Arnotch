import QtQuick

// The next meeting: time, title, how soon, and a button to join its video call.
Item {
    id: row

    // NotchState.nextMeeting: { title, start, end, link }
    required property var meeting
    // Milliseconds since the epoch, ticking once a minute
    required property real now

    readonly property real startMs: meeting.start * 1000
    readonly property bool underway: startMs <= now
    readonly property int minutes: Math.max(0, Math.ceil((startMs - now) / 60000))

    implicitHeight: 34

    Rectangle {
        id: badge
        anchors.left: parent.left
        anchors.verticalCenter: parent.verticalCenter
        width: 34
        height: 34
        radius: 8
        color: row.underway || row.minutes <= 10 ? "#33ff9f0a" : "#1affffff"

        Text {
            anchors.centerIn: parent
            text: Qt.formatTime(new Date(row.startMs), "HH:mm")
            color: row.underway || row.minutes <= 10 ? "#ffb020" : "#c9ccd2"
            font.pixelSize: 10
            font.weight: Font.DemiBold
            font.features: { "tnum": 1 }
        }
    }

    Column {
        anchors.left: badge.right
        anchors.leftMargin: 12
        anchors.right: join.visible ? join.left : parent.right
        anchors.rightMargin: 8
        anchors.verticalCenter: parent.verticalCenter
        spacing: 2

        Text {
            width: parent.width
            elide: Text.ElideRight
            text: row.meeting.title
            color: "#f2f3f5"
            font.pixelSize: 13
            font.weight: Font.DemiBold
        }
        Text {
            width: parent.width
            elide: Text.ElideRight
            text: row.underway ? qsTr("Under way, until %1").arg(Qt.formatTime(new Date(row.meeting.end * 1000), "HH:mm"))
                  : row.minutes < 60 ? qsTr("In %1 min").arg(row.minutes)
                  : qsTr("In %1 h %2 min").arg(Math.floor(row.minutes / 60)).arg(row.minutes % 60)
            color: "#8b909a"
            font.pixelSize: 11
        }
    }

    PillButton {
        id: join
        anchors.right: parent.right
        anchors.verticalCenter: parent.verticalCenter
        visible: !!row.meeting.link
        text: qsTr("Join")
        tint: "#34c759"
        filled: row.underway || row.minutes <= 10
        onClicked: Qt.openUrlExternally(row.meeting.link)
    }
}
