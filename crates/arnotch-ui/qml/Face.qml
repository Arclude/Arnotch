import QtQuick
import QtQuick.Shapes

// The notch mascot. `mood` drives the eyes: idle | working | waiting | happy | listening
// | speaking.
// Animations only run while the face is visible, so hidden faces cost nothing, and
// only while something moves: timers start each short move, so pauses draw no frames.
Item {
    id: face

    property string mood: "idle"
    property real size: 40
    property color badge: "transparent"
    // The head's colour, top and bottom: white for the notch itself, an agent's colour for
    // its tile.
    property color tint: "#ffffff"
    property color shade: "#d9dee7"
    // While listening: how loud the voice is, from 0 to 1
    property real level: 0

    property bool blinking: false
    // Speaking: how far the mouth is open, from 0 to 1
    property real mouth: 0

    implicitWidth: size * 1.3
    implicitHeight: size

    onMoodChanged: if (mood !== "working") eyes.lookX = 0

    // Listening: a halo that swells with the voice
    Rectangle {
        anchors.centerIn: head
        visible: face.mood === "listening"
        width: head.width + Math.round(face.size * (0.12 + face.level * 0.3))
        height: head.height + Math.round(face.size * (0.12 + face.level * 0.3))
        radius: height * 0.38
        color: "transparent"
        border.color: "#ff9f0a"
        border.width: Math.max(1.5, face.size * 0.05)
        opacity: 0.35 + face.level * 0.65
    }

    Rectangle {
        id: head
        anchors.fill: parent
        radius: height * 0.34
        gradient: Gradient {
            GradientStop { position: 0.0; color: face.tint }
            GradientStop { position: 1.0; color: face.shade }
        }
        transform: Translate { id: bob }

        Item {
            id: eyes

            property real lookX: 0

            Behavior on lookX { NumberAnimation { duration: 420; easing.type: Easing.InOutQuad } }

            anchors.centerIn: parent
            // Whole pixels: a small face would otherwise redraw for every fraction of one.
            anchors.horizontalCenterOffset: Math.round(lookX * face.size * 0.13)
            anchors.verticalCenterOffset: face.mood === "speaking" ? -face.size * 0.08
                                        : face.mood === "waiting" || face.mood === "listening" ? -face.size * 0.04 : 0
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
                              : face.mood === "waiting" || face.mood === "listening" ? parent.height
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

    // Speaking: a mouth that opens and closes
    Rectangle {
        visible: face.mood === "speaking"
        anchors.horizontalCenter: head.horizontalCenter
        anchors.verticalCenter: head.verticalCenter
        anchors.verticalCenterOffset: Math.round(face.size * 0.22)
        width: Math.round(face.size * 0.24)
        height: Math.max(2, Math.round(face.size * (0.05 + face.mouth * 0.13)))
        radius: Math.min(width, height) / 2
        color: "#16181d"
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
        running: face.visible && (face.mood === "idle" || face.mood === "waiting" || face.mood === "listening")
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

    // Talking: the mouth moves about eight times a second
    Timer {
        running: face.visible && face.mood === "speaking"
        repeat: true
        interval: 120
        onTriggered: face.mouth = face.mouth > 0.5 ? Math.random() * 0.3 : 0.5 + Math.random() * 0.5
        onRunningChanged: if (!running) face.mouth = 0
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
