import QtQuick

// The Gmail inbox: the newest messages, unread ones bold and bounces in red. Pointing at
// one shows what can be done with it: open it in Gmail, or have Claude summarize it or
// draft a reply (saved in Gmail as a draft, never sent).
Column {
    id: view

    // NotchState.mails, newest first: [{ uid, id, from, address, subject, snippet, at,
    // unread, bounce, link }]
    required property var mails
    // What went wrong checking the inbox, "" when nothing
    required property string error
    required property string address

    signal open(var mail)
    signal summarize(var mail)
    signal draft(var mail)
    signal refresh()

    function when(seconds) {
        const date = new Date(seconds * 1000)
        const today = new Date()
        return date.toDateString() === today.toDateString()
               ? Qt.formatTime(date, "HH:mm") : Qt.formatDate(date, "d MMM")
    }

    spacing: 8

    Item {
        width: view.width
        height: 26

        Text {
            anchors.left: parent.left
            anchors.verticalCenter: parent.verticalCenter
            text: "✉️  " + view.address
            color: "#c9ccd2"
            font.pixelSize: 12
            font.weight: Font.DemiBold
        }
        IconButton {
            anchors.right: parent.right
            anchors.verticalCenter: parent.verticalCenter
            glyph: "⟳"
            hint: qsTr("Check now")
            onClicked: view.refresh()
        }
    }

    Text {
        width: view.width
        visible: view.error !== ""
        text: qsTr("Couldn't check the inbox: %1").arg(view.error)
        color: "#ff6b6b"
        font.pixelSize: 11
        wrapMode: Text.Wrap
    }

    Text {
        visible: view.mails.length === 0 && view.error === ""
        text: qsTr("Nothing in the inbox from the last 30 days.")
        color: "#8b909a"
        font.pixelSize: 12
    }

    Flickable {
        id: list

        width: view.width
        height: Math.min(contentHeight, 360)
        visible: view.mails.length > 0
        clip: true
        contentWidth: width
        contentHeight: rows.height
        boundsBehavior: Flickable.StopAtBounds

        Column {
            id: rows
            width: list.width - 8
            spacing: 2

            Repeater {
                model: view.mails

                Rectangle {
                    id: row

                    required property var modelData
                    readonly property var mail: modelData
                    readonly property bool pointed: hover.hovered

                    width: rows.width
                    height: content.height + 12
                    radius: 8
                    color: pointed ? "#14ffffff" : "transparent"

                    HoverHandler { id: hover }

                    // A bounce: the mail the user sent didn't arrive.
                    Rectangle {
                        visible: row.mail.bounce
                        x: 0
                        y: 6
                        width: 3
                        height: row.height - 12
                        radius: 1.5
                        color: "#ff453a"
                    }

                    Column {
                        id: content
                        x: 12
                        y: 6
                        width: row.width - 20
                        spacing: 2

                        Item {
                            width: content.width
                            height: fromText.height

                            Rectangle {
                                id: unreadDot
                                visible: row.mail.unread
                                anchors.verticalCenter: parent.verticalCenter
                                width: 6
                                height: 6
                                radius: 3
                                color: "#4c8dff"
                            }
                            Text {
                                id: fromText
                                anchors.left: unreadDot.visible ? unreadDot.right : parent.left
                                anchors.leftMargin: unreadDot.visible ? 6 : 0
                                anchors.right: timeText.left
                                anchors.rightMargin: 8
                                text: row.mail.bounce ? "⚠ " + qsTr("Returned: not delivered") : row.mail.from
                                color: row.mail.bounce ? "#ff6b6b" : "#f2f3f5"
                                font.pixelSize: 12
                                font.weight: row.mail.unread ? Font.DemiBold : Font.Normal
                                elide: Text.ElideRight
                            }
                            Text {
                                id: timeText
                                anchors.right: parent.right
                                anchors.verticalCenter: parent.verticalCenter
                                text: view.when(row.mail.at)
                                color: "#8b909a"
                                font.pixelSize: 10
                            }
                        }
                        Text {
                            width: content.width
                            text: row.mail.subject || qsTr("(no subject)")
                            color: row.mail.bounce ? "#ff9f9a" : "#e1e3e8"
                            font.pixelSize: 11
                            font.weight: row.mail.unread ? Font.DemiBold : Font.Normal
                            elide: Text.ElideRight
                        }
                        Text {
                            width: content.width
                            visible: text !== ""
                            text: row.mail.snippet
                            color: "#8b909a"
                            font.pixelSize: 11
                            wrapMode: Text.Wrap
                            maximumLineCount: row.pointed ? 3 : 1
                            elide: Text.ElideRight
                        }
                        Flow {
                            width: content.width
                            visible: row.pointed
                            spacing: 6
                            topPadding: 4

                            PillButton {
                                text: qsTr("Open in Gmail")
                                onClicked: view.open(row.mail)
                            }
                            PillButton {
                                text: qsTr("Summarize")
                                tint: "#8fb4ff"
                                onClicked: view.summarize(row.mail)
                            }
                            PillButton {
                                text: qsTr("Draft a reply")
                                tint: "#34c759"
                                visible: !row.mail.bounce
                                onClicked: view.draft(row.mail)
                            }
                        }
                    }
                }
            }
        }
    }
}
