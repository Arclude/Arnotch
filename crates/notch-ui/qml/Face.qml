import QtQuick
import QtQuick.Shapes

// The notch mascot. `mood` drives the eyes: idle | working | waiting | happy.
// Animations only run while the face is visible, so hidden faces cost nothing.
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

            anchors.centerIn: parent
            anchors.horizontalCenterOffset: lookX * face.size * 0.13
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

                        Behavior on height { NumberAnimation { duration: 70 } }
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
    SequentialAnimation {
        running: face.visible && face.mood === "working"
        loops: Animation.Infinite
        NumberAnimation { target: eyes; property: "lookX"; to: -1; duration: 380; easing.type: Easing.InOutQuad }
        PauseAnimation { duration: 450 }
        NumberAnimation { target: eyes; property: "lookX"; to: 1; duration: 520; easing.type: Easing.InOutQuad }
        PauseAnimation { duration: 450 }
    }

    // Hop to get attention while waiting for the user
    SequentialAnimation {
        running: face.visible && face.mood === "waiting"
        loops: Animation.Infinite
        onStopped: bob.y = 0
        NumberAnimation { target: bob; property: "y"; to: -face.size * 0.12; duration: 180; easing.type: Easing.OutQuad }
        NumberAnimation { target: bob; property: "y"; to: 0; duration: 260; easing.type: Easing.OutBounce }
        PauseAnimation { duration: 1400 }
    }
}
