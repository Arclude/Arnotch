import QtQuick
import QtQuick.Shapes

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

        MediaButton {
            anchors.verticalCenter: parent.verticalCenter
            kind: "previous"
            hint: qsTr("Previous")
            visible: card.player.can_go_previous
            onClicked: card.bridge.mediaCommand(card.player.bus_name, "previous")
        }
        MediaButton {
            anchors.verticalCenter: parent.verticalCenter
            size: 32
            kind: card.player.playing ? "pause" : "play"
            hint: card.player.playing ? qsTr("Pause") : qsTr("Play")
            onClicked: card.bridge.mediaCommand(card.player.bus_name, "play_pause")
        }
        MediaButton {
            anchors.verticalCenter: parent.verticalCenter
            kind: "next"
            hint: qsTr("Next")
            visible: card.player.can_go_next
            onClicked: card.bridge.mediaCommand(card.player.bus_name, "next")
        }
    }

    // A round button with a drawn icon: font glyphs such as ⏮ and ▶ come from different
    // fonts with different sizes and baselines, so they never lined up.
    component MediaButton: Rectangle {
        id: button

        property string kind
        property string hint
        property real size: 28

        signal clicked()

        Accessible.role: Accessible.Button
        Accessible.name: hint

        width: size
        height: size
        radius: size / 2
        color: area.pressed ? "#4dffffff" : area.containsMouse ? "#2effffff" : "transparent"

        // A 12×12 icon, drawn around the button's centre.
        Item {
            id: icon

            readonly property real s: button.size >= 32 ? 1.15 : 1
            readonly property color ink: "#f2f3f5"

            anchors.centerIn: parent
            width: 12
            height: 12
            scale: s

            // Play, and the arrow of next/previous (mirrored for previous)
            Shape {
                anchors.fill: parent
                visible: button.kind !== "pause"
                preferredRendererType: Shape.CurveRenderer
                transform: Scale { origin.x: 6; xScale: button.kind === "previous" ? -1 : 1 }

                ShapePath {
                    fillColor: icon.ink
                    strokeColor: "transparent"
                    startX: button.kind === "play" ? 2.5 : 1
                    startY: 1
                    PathLine { x: button.kind === "play" ? 11 : 8.5; y: 6 }
                    PathLine { x: button.kind === "play" ? 2.5 : 1; y: 11 }
                    PathLine { x: button.kind === "play" ? 2.5 : 1; y: 1 }
                }
            }
            // The bar of next/previous
            Rectangle {
                visible: button.kind === "next" || button.kind === "previous"
                x: button.kind === "next" ? 9 : 1
                y: 1
                width: 2
                height: 10
                radius: 1
                color: icon.ink
            }
            // Pause
            Repeater {
                model: button.kind === "pause" ? 2 : 0

                Rectangle {
                    required property int index
                    x: index === 0 ? 2 : 7
                    y: 1
                    width: 3
                    height: 10
                    radius: 1
                    color: icon.ink
                }
            }
        }

        MouseArea {
            id: area
            anchors.fill: parent
            hoverEnabled: true
            cursorShape: Qt.PointingHandCursor
            onClicked: button.clicked()
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
