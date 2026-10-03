import QtQuick
import QtQuick.Shapes

// The notch mascot. `mood` drives the eyes: idle | working | waiting | happy.
// Animations only run while the face is visible, so hidden faces cost nothing, and
// only while something moves: timers start each short move, so pauses draw no frames.
Item {
    id: face

    property string mood: "idle"
    property real size: 40
    property color badge: "transparent"

    property bool blinking: false

    implicitWidth: size * 1.3
    implicitHeight: size

    onMoodChanged: if (mood !== "working") eyes.lookX = 0

    Rectangle {
        id: head
        anchors.fill: parent
        radius: height * 0.34
        gradient: Gradient {
            GradientStop { position: 0.0; color: "#ffffff" }
            GradientStop { position: 1.0; color: "#d9dee7" }
        }
        transform: Translate { id: bob }

        Item {
            id: eyes

            property real lookX: 0

            Behavior on lookX { NumberAnimation { duration: 420; easing.type: Easing.InOutQuad } }

            anchors.centerIn: parent
            // Whole pixels: a small face would otherwise redraw for every fraction of one.
            anchors.horizontalCenterOffset: Math.round(lookX * face.size * 0.13)
            anchors.verticalCenterOffset: face.mood === "waiting" ? -face.size * 0.04 : 0
            width: face.size * 0.62
            height: face.size * 0.32

            Repeater {
                model: 2

                Item {
                    id: eye

                    required property int index

                    x: index === 0 ? 0 : eyes.width - width
                    width: face.size * 0.15
                    height: eyes.height

                    Rectangle {
                        anchors.centerIn: parent
                        visible: face.mood !== "happy"
                        width: parent.width
                        height: face.blinking ? Math.max(2, face.size * 0.04)
                              : face.mood === "waiting" ? parent.height
                              : parent.height * 0.78
                        radius: width / 2
                        color: "#16181d"
                        // No animation on the height: a cartoon blink is instant, and at a high
                        // refresh rate an eased one costs a dozen frames for no visible gain.
                    }

                    // "^" eye when happy
                    Shape {
                        id: happyEye
                        anchors.fill: parent
                        anchors.margins: -face.size * 0.03
                        visible: face.mood === "happy"
                        preferredRendererType: Shape.CurveRenderer

                        ShapePath {
                            strokeColor: "#16181d"
                            strokeWidth: Math.max(2, face.size * 0.07)
                            fillColor: "transparent"
                            capStyle: ShapePath.RoundCap
                            startX: 0
                            startY: happyEye.height * 0.7
                            PathQuad {
                                x: happyEye.width
                                y: happyEye.height * 0.7
                                controlX: happyEye.width / 2
                                controlY: happyEye.height * 0.0
                            }
                        }
                    }
                }
            }
        }
    }

    Rectangle {
        visible: face.badge.a > 0
        width: face.size * 0.3
        height: width
        radius: width / 2
        x: -width * 0.25
        y: -width * 0.25
        color: face.badge
        border.color: "#0b0c0f"
        border.width: Math.max(1.5, face.size * 0.05)
    }

    // Random blinking
    Timer {
        running: face.visible && (face.mood === "idle" || face.mood === "waiting")
        repeat: true
        interval: 2600
        onTriggered: {
            interval = 2200 + Math.random() * 3000
            face.blinking = true
            unblink.start()
        }
    }
    Timer {
        id: unblink
        interval: 110
        onTriggered: face.blinking = false
    }

    // Glance left and right while working
    Timer {
        running: face.visible && face.mood === "working"
        repeat: true
        triggeredOnStart: true
        interval: 1300
        onTriggered: eyes.lookX = eyes.lookX < 0 ? 1 : -1
    }

    // Hop to get attention while waiting for the user
    Timer {
        running: face.visible && face.mood === "waiting"
        repeat: true
        interval: 1840
        onTriggered: hop.restart()
        onRunningChanged: if (!running) hop.stop()
    }
    SequentialAnimation {
        id: hop
        onStopped: bob.y = 0
        NumberAnimation { target: bob; property: "y"; to: -Math.round(face.size * 0.12); duration: 180; easing.type: Easing.OutQuad }
        NumberAnimation { target: bob; property: "y"; to: 0; duration: 260; easing.type: Easing.OutBounce }
    }
}
