import QtQuick

// Claude asks for more than a yes or no: a plan to approve (ExitPlanMode) or questions
// with options to pick from (AskUserQuestion). Answers go back as a reply object:
// { decision: "allow" | "deny" | "ask", mode, answers: { question: label }, message }.
Column {
    id: prompt

    // ActivityCard.permission: { request_id, tool, plan, questions }
    required property var permission

    readonly property bool isPlan: permission.plan != null
    readonly property var questions: permission.questions ?? []
    // A text field is open: the island takes the keyboard only then.
    readonly property bool typing: noteOpen || customIndex >= 0

    signal replied(var reply)

    // Plan: a note for Claude when it should keep planning
    property bool noteOpen: false
    // Questions: chosen labels per question index, and custom answers
    property var chosen: ({})
    property var custom: ({})
    // The question whose own answer is being typed, -1 when none
    property int customIndex: -1
    // Bumped on every change of `chosen`/`custom`, which bindings can't see into
    property int revision: 0

    readonly property bool allAnswered: {
        revision // dependency
        return questions.every((q, i) => (custom[i] ?? "") !== "" || (chosen[i] ?? []).length > 0)
    }

    function toggle(index, label, multi) {
        const now = chosen[index] ?? []
        chosen[index] = multi ? (now.includes(label) ? now.filter(l => l !== label) : now.concat([label]))
                              : [label]
        delete custom[index]
        revision++
    }

    function sendAnswers() {
        const answers = {}
        questions.forEach((q, i) => answers[q.question] = (custom[i] ?? "") || (chosen[i] ?? []).join(", "))
        replied({ decision: "allow", answers: answers })
    }

    spacing: 8
    width: parent ? parent.width : 0

    // ---- Plan ----

    Rectangle {
        visible: prompt.isPlan
        width: parent.width
        height: Math.min(planText.implicitHeight + 16, 220)
        radius: 8
        color: "#0dffffff"

        Flickable {
            id: planView
            anchors.fill: parent
            anchors.margins: 8
            clip: true
            contentHeight: planText.implicitHeight
            boundsBehavior: Flickable.StopAtBounds

            TextEdit {
                id: planText
                width: planView.width - 6
                readOnly: true
                selectByMouse: true
                wrapMode: TextEdit.Wrap
                textFormat: TextEdit.MarkdownText
                text: prompt.isPlan ? prompt.permission.plan : ""
                color: "#e6e7ea"
                selectionColor: "#4c8dff"
                font.pixelSize: 12
                onLinkActivated: link => Qt.openUrlExternally(link)
            }
        }

        Rectangle {
            anchors.right: parent.right
            anchors.rightMargin: 3
            y: 8 + planView.visibleArea.yPosition * (parent.height - 16)
            width: 3
            height: planView.visibleArea.heightRatio * (parent.height - 16)
            radius: 1.5
            color: "#40ffffff"
            visible: planView.contentHeight > planView.height
        }
    }

    Flow {
        visible: prompt.isPlan && !prompt.noteOpen
        width: parent.width
        spacing: 6

        PillButton {
            text: qsTr("Approve, auto-accept edits")
            tint: "#34c759"
            filled: true
            onClicked: prompt.replied({ decision: "allow", mode: "acceptEdits" })
        }
        PillButton {
            text: qsTr("Approve, I'll review each edit")
            tint: "#34c759"
            onClicked: prompt.replied({ decision: "allow", mode: "default" })
        }
        PillButton {
            text: qsTr("Keep planning…")
            tint: "#ffb020"
            onClicked: prompt.noteOpen = true
        }
        PillButton {
            text: qsTr("In terminal")
            tint: "#8b909a"
            onClicked: prompt.replied({ decision: "ask" })
        }
    }

    SendField {
        width: parent.width
        visible: prompt.isPlan && prompt.noteOpen
        monospace: false
        placeholder: qsTr("What should change? (Enter sends, Esc goes back)")
        onVisibleChanged: if (visible) focusInput()
        onSubmitted: text => prompt.replied({ decision: "deny", message: text })
        onCancelled: prompt.noteOpen = false
    }

    // ---- Questions ----

    Repeater {
        model: prompt.isPlan ? [] : prompt.questions

        Column {
            id: question

            required property var modelData
            required property int index
            readonly property bool multi: modelData.multiSelect === true

            width: prompt.width
            spacing: 4

            Row {
                spacing: 8

                Rectangle {
                    visible: !!question.modelData.header
                    anchors.verticalCenter: parent.verticalCenter
                    width: headerText.implicitWidth + 12
                    height: 18
                    radius: 9
                    color: "#264c8dff"

                    Text {
                        id: headerText
                        anchors.centerIn: parent
                        text: question.modelData.header ?? ""
                        color: "#8fb4ff"
                        font.pixelSize: 10
                        font.weight: Font.DemiBold
                    }
                }
                Text {
                    anchors.verticalCenter: parent.verticalCenter
                    width: prompt.width - 90
                    wrapMode: Text.Wrap
                    text: question.modelData.question
                    color: "#f2f3f5"
                    font.pixelSize: 12
                    font.weight: Font.DemiBold
                }
            }

            Repeater {
                model: question.modelData.options ?? []

                Rectangle {
                    id: option

                    required property var modelData
                    readonly property bool picked: {
                        prompt.revision // dependency
                        return (prompt.chosen[question.index] ?? []).includes(modelData.label)
                               && !(prompt.custom[question.index] ?? "")
                    }

                    width: question.width
                    height: optionText.implicitHeight + 12
                    radius: 8
                    color: picked ? "#264c8dff" : optionArea.containsMouse ? "#1affffff" : "#0dffffff"
                    border.color: picked ? "#4c8dff" : "transparent"
                    border.width: 1

                    // ○ ● for one choice, ☐ ☑ for several
                    Text {
                        id: mark
                        anchors.left: parent.left
                        anchors.leftMargin: 10
                        anchors.top: parent.top
                        anchors.topMargin: 6
                        text: question.multi ? (option.picked ? "☑" : "☐") : (option.picked ? "●" : "○")
                        color: option.picked ? "#4c8dff" : "#8b909a"
                        font.pixelSize: 12
                    }
                    Column {
                        id: optionText
                        anchors.left: mark.right
                        anchors.leftMargin: 8
                        anchors.right: parent.right
                        anchors.rightMargin: 10
                        anchors.verticalCenter: parent.verticalCenter

                        Text {
                            width: parent.width
                            wrapMode: Text.Wrap
                            text: option.modelData.label
                            color: "#f2f3f5"
                            font.pixelSize: 12
                        }
                        Text {
                            width: parent.width
                            visible: !!option.modelData.description
                            wrapMode: Text.Wrap
                            text: option.modelData.description ?? ""
                            color: "#8b909a"
                            font.pixelSize: 10
                        }
                    }

                    MouseArea {
                        id: optionArea
                        anchors.fill: parent
                        hoverEnabled: true
                        cursorShape: Qt.PointingHandCursor
                        onClicked: prompt.toggle(question.index, option.modelData.label, question.multi)
                    }
                }
            }

            // Their own answer instead of the options
            Text {
                visible: prompt.customIndex !== question.index
                text: {
                    prompt.revision // dependency
                    const own = prompt.custom[question.index] ?? ""
                    return own ? qsTr("Your answer: %1  (change)").arg(own) : qsTr("Other answer…")
                }
                color: "#8fb4ff"
                font.pixelSize: 11

                MouseArea {
                    anchors.fill: parent
                    cursorShape: Qt.PointingHandCursor
                    onClicked: prompt.customIndex = question.index
                }
            }
            SendField {
                width: question.width
                visible: prompt.customIndex === question.index
                monospace: false
                placeholder: qsTr("Your answer (Enter keeps it, Esc goes back)")
                onVisibleChanged: if (visible) focusInput()
                onSubmitted: text => {
                    prompt.custom[question.index] = text
                    prompt.customIndex = -1
                    prompt.revision++
                }
                onCancelled: prompt.customIndex = -1
            }
        }
    }

    Row {
        visible: !prompt.isPlan
        spacing: 6

        PillButton {
            text: qsTr("Send")
            tint: "#34c759"
            filled: true
            opacity: prompt.allAnswered ? 1 : 0.4
            onClicked: if (prompt.allAnswered) prompt.sendAnswers()
        }
        PillButton {
            text: qsTr("In terminal")
            tint: "#8b909a"
            onClicked: prompt.replied({ decision: "ask" })
        }
    }
}
