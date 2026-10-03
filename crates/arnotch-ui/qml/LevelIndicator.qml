import QtQuick
import QtQuick.Shapes

// A level shown in the closed island: volume, brightness, or the battery when it is
// plugged in. An icon, a bar and the percentage.
Row {
    id: indicator

    // "volume", "brightness" or "battery"
    property string kind: "volume"
    // From 0 to 1
    property real value: 0
    property bool muted: false

    readonly property color ink: "#f2f3f5"
    readonly property color fill: kind === "battery" ? "#34c759" : muted ? "#5c616b" : ink

    spacing: 10

    // 14×14 icon
    Item {
        anchors.verticalCenter: parent.verticalCenter
        width: 14
        height: 14

        // Speaker, with waves or a cross
        Shape {
            anchors.fill: parent
            visible: indicator.kind === "volume"
            preferredRendererType: Shape.CurveRenderer

            ShapePath {
                fillColor: indicator.ink
                strokeColor: "transparent"
                startX: 0; startY: 5
                PathLine { x: 3; y: 5 }
                PathLine { x: 7; y: 1.5 }
                PathLine { x: 7; y: 12.5 }
                PathLine { x: 3; y: 9 }
                PathLine { x: 0; y: 9 }
                PathLine { x: 0; y: 5 }
            }
            ShapePath {
                strokeColor: indicator.muted ? "transparent" : indicator.ink
                strokeWidth: 1.4
                fillColor: "transparent"
                capStyle: ShapePath.RoundCap
                startX: 9; startY: 4.5
                PathArc { x: 9; y: 9.5; radiusX: 3; radiusY: 3 }
            }
            ShapePath {
                strokeColor: indicator.muted || indicator.value < 0.5 ? "transparent" : indicator.ink
                strokeWidth: 1.4
                fillColor: "transparent"
                capStyle: ShapePath.RoundCap
                startX: 10.5; startY: 2
                PathArc { x: 10.5; y: 12; radiusX: 5.5; radiusY: 5.5 }
            }
            ShapePath {
                strokeColor: indicator.muted ? indicator.ink : "transparent"
                strokeWidth: 1.4
                fillColor: "transparent"
                capStyle: ShapePath.RoundCap
                startX: 9.5; startY: 5
                PathLine { x: 13.5; y: 9 }
                PathMove { x: 13.5; y: 5 }
                PathLine { x: 9.5; y: 9 }
            }
        }

        // Sun
        Item {
            anchors.fill: parent
            visible: indicator.kind === "brightness"

            Rectangle {
                anchors.centerIn: parent
                width: 6
                height: 6
                radius: 3
                color: indicator.ink
            }
            Repeater {
                model: 8

                Rectangle {
                    required property int index
                    x: 6.25
                    y: 0
                    width: 1.5
                    height: 2.5
                    radius: 0.75
                    color: indicator.ink
                    transform: Rotation { origin.x: 0.75; origin.y: 7; angle: index * 45 }
                }
            }
        }

        // Battery
        Item {
            anchors.fill: parent
            visible: indicator.kind === "battery"

            Rectangle {
                y: 3.5
                width: 12
                height: 7
                radius: 2
                color: "transparent"
                border.color: indicator.ink
                border.width: 1.2

                Rectangle {
                    x: 2
                    y: 2
                    width: Math.max(1, 8 * indicator.value)
                    height: 3
                    radius: 0.5
                    color: indicator.fill
                }
            }
            Rectangle {
                x: 12.5
                y: 5.5
                width: 1.5
                height: 3
                radius: 0.75
                color: indicator.ink
            }
        }
    }

    Rectangle {
        anchors.verticalCenter: parent.verticalCenter
        width: 140
        height: 5
        radius: 2.5
        color: "#26ffffff"

        Rectangle {
            width: parent.width * Math.max(0, Math.min(1, indicator.value))
            height: parent.height
            radius: parent.radius
            color: indicator.fill

            Behavior on width { NumberAnimation { duration: 120; easing.type: Easing.OutQuad } }
        }
    }

    Text {
        anchors.verticalCenter: parent.verticalCenter
        width: 32
        horizontalAlignment: Text.AlignRight
        text: Math.round(indicator.value * 100) + "%"
        color: "#c9ccd2"
        font.pixelSize: 11
        font.weight: Font.DemiBold
        font.features: { "tnum": 1 }
    }
}
