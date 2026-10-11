import QtQuick

// A plan in detail: how much of each window is used and when it resets, or the tokens
// used where there are no limits.
//   ● Codex plan  Plus · updated just now
//   5 hours  ━━━──────  22%  ⟳ in 3h 05m
//   Week     ━━━━━────  47%  ⟳ Sun 18:00
Row {
    id: card

    // { key, label, name, windows: [{ minutes, used_percent, resets_at }], tokens, updated }
    // (updated in ms since the epoch)
    required property var plan
    required property var notch
    // Milliseconds since the epoch, ticking once a minute
    required property real now

    spacing: 14

    function windowLabel(minutes) {
        if (minutes >= 10000 && minutes <= 10200)
            return qsTr("Week")
        if (minutes === 300)
            return qsTr("5 hours")
        return minutes >= 1440 ? qsTr("%1 days").arg(Math.round(minutes / 1440))
                               : qsTr("%1 h").arg(Math.round(minutes / 60))
    }

    function resetText(resetsAt) {
        if (!resetsAt)
            return ""
        const left = resetsAt * 1000 - now
        if (left <= 0)
            return qsTr("now")
        if (left < 24 * 3600 * 1000) {
            const minutes = Math.round(left / 60000)
            const hours = Math.floor(minutes / 60)
            return hours > 0 ? qsTr("in %1h %2m").arg(hours).arg(String(minutes % 60).padStart(2, "0"))
                             : qsTr("in %1m").arg(minutes)
        }
        return new Date(resetsAt * 1000).toLocaleString(Qt.locale(), "ddd HH:mm")
    }

    function updatedText(updated) {
        if (!updated)
            return ""
        const minutes = Math.floor((now - updated) / 60000)
        if (minutes < 1)
            return qsTr("updated just now")
        if (minutes < 60)
            return qsTr("updated %1 min ago").arg(minutes)
        if (minutes < 24 * 60)
            return qsTr("updated %1 h ago").arg(Math.floor(minutes / 60))
        return qsTr("updated %1").arg(new Date(updated).toLocaleDateString(Qt.locale(), "d MMM"))
    }

    function tokens(count) {
        return count >= 1e6 ? (count / 1e6).toFixed(1) + "M" : count >= 1000 ? Math.round(count / 1000) + "k" : String(count)
    }

    function barColor(used) {
        return used >= 90 ? "#ff5f57" : used >= 70 ? "#ffb020" : "#34c759"
    }

    readonly property color tint: notch.agentColor(plan.key)

    Face {
        anchors.verticalCenter: parent.verticalCenter
        size: 44
        tint: Qt.lighter(card.tint, 1.25)
        shade: card.tint
    }

    Column {
        anchors.verticalCenter: parent.verticalCenter
        width: card.width - 72
        spacing: 6

        Row {
            spacing: 8

            Rectangle {
                anchors.verticalCenter: parent.verticalCenter
                width: 8
                height: 8
                radius: 4
                color: card.tint
            }
            Text {
                text: qsTr("%1 plan").arg(card.plan.label)
                color: "#f2f3f5"
                font.pixelSize: 14
                font.weight: Font.DemiBold
            }
            Text {
                anchors.baseline: parent.children[1].baseline
                text: [card.plan.name, card.updatedText(card.plan.updated)].filter(t => t).join(" · ")
                color: "#8b909a"
                font.pixelSize: 11
            }
        }

        Repeater {
            model: card.plan.windows ?? []

            Row {
                id: windowRow

                required property var modelData

                spacing: 10

                Text {
                    width: 52
                    anchors.verticalCenter: parent.verticalCenter
                    text: card.windowLabel(windowRow.modelData.minutes)
                    color: "#8b909a"
                    font.pixelSize: 12
                }
                Rectangle {
                    anchors.verticalCenter: parent.verticalCenter
                    width: 90
                    height: 5
                    radius: 2.5
                    color: "#1affffff"

                    Rectangle {
                        width: parent.width * Math.min(1, windowRow.modelData.used_percent / 100)
                        height: parent.height
                        radius: parent.radius
                        color: card.barColor(windowRow.modelData.used_percent)
                    }
                }
                Text {
                    width: 38
                    anchors.verticalCenter: parent.verticalCenter
                    horizontalAlignment: Text.AlignRight
                    text: Math.round(windowRow.modelData.used_percent) + "%"
                    color: "#f2f3f5"
                    font.pixelSize: 12
                    font.weight: Font.DemiBold
                    font.features: { "tnum": 1 }
                }
                Text {
                    anchors.verticalCenter: parent.verticalCenter
                    visible: text !== ""
                    text: windowRow.modelData.resets_at ? "⟳ " + card.resetText(windowRow.modelData.resets_at) : ""
                    color: "#8b909a"
                    font.pixelSize: 12
                    font.features: { "tnum": 1 }
                }
            }
        }

        Repeater {
            model: card.plan.tokens ? [
                { label: qsTr("Today"), count: card.plan.tokens.today, cost: card.plan.tokens.cost_today_usd },
                { label: qsTr("7 days"), count: card.plan.tokens.week, cost: card.plan.tokens.cost_week_usd }
            ] : []

            Row {
                id: tokenRow

                required property var modelData

                spacing: 10

                Text {
                    width: 52
                    text: tokenRow.modelData.label
                    color: "#8b909a"
                    font.pixelSize: 12
                }
                Text {
                    text: qsTr("%1 tokens").arg(card.tokens(tokenRow.modelData.count))
                          + (tokenRow.modelData.cost > 0 ? "  ·  $" + tokenRow.modelData.cost.toFixed(2) : "")
                    color: "#f2f3f5"
                    font.pixelSize: 12
                    font.features: { "tnum": 1 }
                }
            }
        }
    }
}
