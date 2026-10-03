import QtQuick

// What's playing, with the player's buttons.
Item {
    id: card

    // NotchState.media: { bus_name, identity, title, artist, art_url, playing, can_go_next, can_go_previous }
    required property var player
    required property var bridge

    implicitHeight: 44

    Cover {
        id: cover
        anchors.left: parent.left
        anchors.verticalCenter: parent.verticalCenter
        size: 44
        url: card.player.art_url
    }

    Column {
        anchors.left: cover.right
        anchors.leftMargin: 12
        anchors.right: buttons.left
        anchors.rightMargin: 8
        anchors.verticalCenter: parent.verticalCenter
        spacing: 2

        Text {
            width: parent.width
            elide: Text.ElideRight
            text: card.player.title
            color: "#f2f3f5"
            font.pixelSize: 13
            font.weight: Font.DemiBold
        }
        Text {
            width: parent.width
            elide: Text.ElideRight
            text: [card.player.artist, card.player.identity].filter(Boolean).join(" · ")
            color: "#8b909a"
            font.pixelSize: 11
        }
    }

    Row {
        id: buttons
        anchors.right: parent.right
        anchors.verticalCenter: parent.verticalCenter
        spacing: 2

        IconButton {
            glyph: "⏮"
            hint: qsTr("Previous")
            visible: card.player.can_go_previous
            onClicked: card.bridge.mediaCommand(card.player.bus_name, "previous")
        }
        IconButton {
            width: 30
            height: 30
            radius: 15
            glyph: card.player.playing ? "⏸" : "▶"
            hint: card.player.playing ? qsTr("Pause") : qsTr("Play")
            tint: "#f2f3f5"
            onClicked: card.bridge.mediaCommand(card.player.bus_name, "play_pause")
        }
        IconButton {
            glyph: "⏭"
            hint: qsTr("Next")
            visible: card.player.can_go_next
            onClicked: card.bridge.mediaCommand(card.player.bus_name, "next")
        }
    }

    // Album art, or a note when the player has none.
    component Cover: Rectangle {
        id: coverBox

        property real size
        property string url

        width: size
        height: size
        radius: Math.round(size * 0.18)
        color: "#1affffff"
        clip: true

        Text {
            anchors.centerIn: parent
            visible: art.status !== Image.Ready
            text: "♪"
            color: "#8b909a"
            font.pixelSize: Math.round(coverBox.size * 0.5)
        }

        Image {
            id: art
            anchors.fill: parent
            source: coverBox.url
            // Covers can be large; decode only what is shown.
            sourceSize: Qt.size(coverBox.size * 2, coverBox.size * 2)
            fillMode: Image.PreserveAspectCrop
            asynchronous: true
            visible: status === Image.Ready
        }
    }
}
