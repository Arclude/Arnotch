import QtQuick
import QtQuick.Shapes

// A ring that fills as work progresses, with a glyph in the middle: ↓ for a download,
// ✓ once done. A busy one (percent -1) has a short arc going round.
Item {
    id: ring

    property real size: 26
    // From 0 to 100; -1 when the end can't be told
    property real percent: 0
    property color tint: "#4c8dff"
    property string glyph: ""
    property real thickness: Math.max(2, size * 0.11)

    readonly property bool busy: percent < 0

    implicitWidth: size
    implicitHeight: size

    Shape {
        id: shape
        anchors.fill: parent
        preferredRendererType: Shape.CurveRenderer

        // The track
        ShapePath {
            strokeColor: Qt.alpha(ring.tint, 0.22)
            strokeWidth: ring.thickness
            fillColor: "transparent"
            PathAngleArc {
                centerX: ring.size / 2
                centerY: ring.size / 2
                radiusX: (ring.size - ring.thickness) / 2
                radiusY: radiusX
                startAngle: 0
                sweepAngle: 360
            }
        }
        // What's done, from the top clockwise
        ShapePath {
            strokeColor: ring.tint
            strokeWidth: ring.thickness
            fillColor: "transparent"
            capStyle: ShapePath.RoundCap
            PathAngleArc {
                centerX: ring.size / 2
                centerY: ring.size / 2
                radiusX: (ring.size - ring.thickness) / 2
                radiusY: radiusX
                startAngle: -90
                sweepAngle: ring.busy ? 90 : 360 * Math.min(100, ring.percent) / 100

                Behavior on sweepAngle {
                    enabled: !ring.busy
                    NumberAnimation { duration: 250 }
                }
            }
        }

        // Only a busy ring turns; it stops while hidden.
        RotationAnimator on rotation {
            running: ring.busy && ring.visible
            from: 0
            to: 360
            duration: 1100
            loops: Animation.Infinite
            onRunningChanged: if (!running) shape.rotation = 0
        }
    }

    Text {
        anchors.centerIn: parent
        text: ring.glyph
        color: ring.tint
        font.pixelSize: Math.round(ring.size * 0.46)
        font.weight: Font.Bold
    }
}
