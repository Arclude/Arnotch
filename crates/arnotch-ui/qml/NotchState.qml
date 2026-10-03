import QtQuick

// What the notch knows, shared by every island window.
QtObject {
    id: notch

    property ListModel sessions: ListModel {
        // ListModel is not reactive inside functions, so bindings depend on `revision`.
        property int revision: 0
    }

    // Per-session activity that does not fit a ListModel row:
    // { prompt, steps: [{ tool, detail, status }], view: "" | "diff" | "terminal",
    //   diff, command, output: [], result: "running" | "ok" | "fail" }
    property var details: ({})

    // The session shown in detail: the one that changed last.
    property string focusId: ""
    property int focusRevision: 0
    readonly property var focus: {
        focusRevision // dependency
        const row = sessions.get(indexOf(focusId))
        const detail = details[focusId]
        return row && detail ? Object.assign({ project: row.project, phase: row.phase, message: row.detail }, detail) : null
    }

    // Latest thing worth saying, shown next to the big face.
    property string headline: qsTr("All quiet")
    property string subline: ""
    property bool happy: false
    // [{ id, app, app_pid, shell_pid, shell, cwd, command: { pgid, name, args, started_at, cpu } | null, can_send }]
    property var terminals: []
    // Terminals running Claude Code, best candidates first: the session shown in the
    // island, then terminals that can be typed into (others get the path via the clipboard).
    readonly property var claudeTerminals: {
        const focusCwd = focus ? focus.cwd : ""
        const rank = t => (t.cwd === focusCwd ? 0 : 2) + (t.can_send ? 0 : 1)
        return terminals.filter(t => t.command && t.command.name === "claude")
                        .sort((a, b) => rank(a) - rank(b))
    }

    // A level that just changed, shown in the closed island for a moment:
    // { kind: "volume" | "brightness" | "battery", value: 0..1, muted }
    property var level: null
    readonly property bool showingLevel: levelTimer.running
    // { percent, plugged }, null without a battery
    property var battery: null
    // Main binds this to the setting.
    property bool indicators: true

    // The Claude plan's rate limits, from the latest status line: { five_hour, seven_day },
    // each { used_percent, resets_at } or absent (API key users have none).
    property var limits: null
    // Per session: { model, cost_usd, context_percent }
    property var usage: ({})
    property int usageRevision: 0
    // Limit warnings already given, by window reset time and threshold
    property var limitWarned: ({})

    // Meetings in the next 24 hours: [{ title, start, end, link }] (Unix seconds)
    property var meetings: []
    // Milliseconds since the epoch, updated once a minute with the clock
    property real now: Date.now()
    // The meeting under way or coming next, null when there is none
    readonly property var nextMeeting: meetings.find(m => m.end * 1000 > now) ?? null
    // Reminders already given, by start and title
    property var reminded: ({})

    // Latest text entries of the clipboard history, newest first
    property var clipboard: []

    // What's playing (see MediaCard), null when no player has a track.
    property var media: null
    // Playing, or paused less than half a minute ago (so it can be resumed from the
    // closed island).
    readonly property bool mediaActive: media !== null && (media.playing || pauseGrace.running)

    // Paths of the files kept on the shelf; Main persists them.
    property var shelf: []
    // Files being shared: [{ id, file, progress, state }]
    property var uploads: []
    readonly property bool uploading: uploads.length > 0
    // The island's chat with Claude: rows of { who: "user" | "claude", body,
    // status: "streaming" | "done" | "failed" | "stopped", files: attached file names, one
    // per line, tool: the tool Claude is using }. A failed answer's body is the error.
    // A ListModel, so a new piece of the answer updates one row instead of rebuilding them all.
    property ListModel chat: ListModel {
        // ListModel is not reactive inside functions, so bindings depend on `revision`.
        property int revision: 0
    }
    // Files to send with the next chat message (absolute paths)
    property var chatAttachments: []
    // The message being answered; chat events of other turns are leftovers of stopped answers.
    property int chatTurn: 0
    readonly property bool chatBusy: {
        chat.revision // dependency
        return chat.count > 0 && chat.get(chat.count - 1).status === "streaming"
    }

    // A plain notification is on screen; it takes precedence over the activity card.
    property bool notifying: false
    property string time: Qt.formatTime(new Date(), "HH:mm")

    // Claude asked for permission; Main switches the islands to the activity tab.
    signal permissionRequested(string requestId)
    // Claude finished answering in the chat ("done" or "failed"), with the answer or error.
    signal chatFinished(string status, string body)

    // True while a new event wants the islands to open on their own.
    readonly property bool alerting: alertTimer.running
    // Like `alerting`, but for live activity (diffs, commands), which users may turn off.
    readonly property bool showingActivity: activityTimer.running

    readonly property string mood: happy ? "happy"
                                  : countPhase("waiting") > 0 ? "waiting"
                                  : countPhase("started") + countPhase("tool") > 0 ? "working"
                                  : "idle"

    readonly property int maxSteps: 5
    // Matches arnotchctl's default wait before Claude Code falls back to the terminal.
    readonly property int permissionTimeout: 45000
    // ...and for plan approvals and questions (ARNOTCH_PROMPT_TIMEOUT)
    readonly property int promptTimeout: 300000

    function indexOf(sessionId) {
        for (let i = 0; i < sessions.count; i++) {
            if (sessions.get(i).sessionId === sessionId)
                return i
        }
        return -1
    }

    function countPhase(phase) {
        sessions.revision // dependency
        let n = 0
        for (let i = 0; i < sessions.count; i++) {
            if (sessions.get(i).phase === phase)
                n++
        }
        return n
    }

    function handleEvent(ev) {
        if (ev.type === "notify") {
            say(ev.title, ev.body, 5000)
            notifying = true
        } else if (ev.type === "upload") {
            handleUpload(ev)
        } else if (ev.type === "level") {
            if (indicators)
                showLevel(ev.kind, ev.value, ev.muted)
        } else if (ev.type === "battery") {
            handleBattery(ev.battery)
        } else if (ev.type === "usage") {
            handleUsage(ev)
        } else if (ev.type === "calendar") {
            meetings = ev.meetings
            remindMeetings()
        } else if (ev.type === "clipboard") {
            clipboard = ev.items
        } else if (ev.type === "media") {
            const wasPlaying = media !== null && media.playing
            media = ev.player ?? null
            if (media && media.playing)
                pauseGrace.stop()
            else if (wasPlaying && media)
                pauseGrace.restart()
        } else if (ev.type === "chat") {
            handleChat(ev)
        } else if (ev.type === "permission_closed") {
            closePermission(ev.request_id)
        } else if (ev.type === "terminals") {
            terminals = ev.terminals
        } else if (ev.type === "agent") {
            notifying = false
            handleAgent(ev)
        }
    }

    function handleAgent(ev) {
        const id = ev.session_id
        const index = indexOf(id)

        if (ev.phase === "ended") {
            if (index >= 0)
                sessions.remove(index)
            delete details[id]
            if (focusId === id)
                focusId = sessions.count > 0 ? sessions.get(0).sessionId : ""
            sessions.revision++
            focusRevision++
            return
        }

        // A tool result keeps the session in its "tool" phase: the agent carries on.
        // A permission request is a way of waiting for the user.
        const phase = ev.phase === "tool_result" ? "tool" : ev.phase === "permission" ? "waiting" : ev.phase
        const row = {
            sessionId: id,
            project: ev.project || "?",
            phase: phase,
            tool: ev.tool || "",
            detail: ev.phase === "tool_result" && index >= 0 ? sessions.get(index).detail
                                                              : ev.detail || ev.message || ev.prompt || "",
            updated: Date.now()
        }
        if (index >= 0)
            sessions.set(index, row)
        else
            sessions.insert(0, row)
        sessions.revision++

        updateDetails(id, ev)
        focusId = id
        focusRevision++

        if (ev.phase === "done") {
            happy = true
            happyTimer.restart()
            say(ev.project, qsTr("Finished"), 4000)
        } else if (ev.phase === "waiting") {
            say(ev.project, ev.message || qsTr("Needs your attention"), 6000)
        } else if (ev.phase === "permission") {
            // Plans and questions take reading: they stay up as long as arnotchctl waits.
            const needsInput = ev.plan != null || ev.questions != null
            say(ev.project, ev.plan != null ? qsTr("Plan ready for review") : ev.questions != null ? qsTr("Claude has a question")
                            : qsTr("Wants to run %1").arg(ev.tool), needsInput ? promptTimeout : permissionTimeout)
            permissionRequested(ev.request_id)
        } else if (ev.phase === "started") {
            headline = ev.project
            subline = ev.prompt || qsTr("Thinking…")
        } else if (ev.phase === "tool") {
            headline = ev.project
            subline = ev.detail ? `${ev.tool} · ${ev.detail}` : ev.tool
        }
    }

    function updateDetails(id, ev) {
        const d = details[id] ?? { prompt: "", steps: [], view: "", diff: null, command: "", output: [], result: "", cwd: "" }
        if (ev.cwd)
            d.cwd = ev.cwd

        if (ev.phase === "permission") {
            d.permission = { request_id: ev.request_id, tool: ev.tool, detail: ev.detail, diff: ev.diff ?? null,
                             plan: ev.plan ?? null, questions: ev.questions ?? null }
        } else if (ev.phase === "started") {
            d.prompt = ev.prompt || ""
            d.steps = []
            d.view = ""
        } else if (ev.phase === "tool") {
            d.steps = d.steps.concat([{ tool: ev.tool, detail: ev.detail, status: "running" }]).slice(-maxSteps)
            if (ev.diff) {
                d.view = "diff"
                d.diff = ev.diff
                activityTimer.restart()
            } else if (ev.tool === "Bash") {
                d.view = "terminal"
                d.command = ev.detail
                d.output = []
                d.result = "running"
                activityTimer.restart()
            }
        } else if (ev.phase === "tool_result") {
            const status = ev.ok ? "ok" : "fail"
            for (let i = d.steps.length - 1; i >= 0; i--) {
                if (d.steps[i].tool === ev.tool && d.steps[i].status === "running") {
                    d.steps[i] = Object.assign({}, d.steps[i], { status: status })
                    break
                }
            }
            d.steps = d.steps.slice() // new array so bindings see the change
            if (ev.tool === "Bash" && d.view === "terminal") {
                d.output = ev.output ?? []
                d.result = status
                activityTimer.restart()
            }
        } else if (ev.phase === "done") {
            // Anything still "running" finished with the turn.
            d.steps = d.steps.map(s => s.status === "running" ? Object.assign({}, s, { status: "ok" }) : s)
        }

        details[id] = d
    }

    // The user closed the island: forget pending alerts so it stays closed.
    function dismiss() {
        alertTimer.stop()
        activityTimer.stop()
        notifying = false
    }

    function handleUpload(ev) {
        const others = uploads.filter(u => u.id !== ev.id)
        if (ev.state === "uploading") {
            uploads = others.concat([ev])
            return
        }
        uploads = others
        if (ev.state === "done")
            say(qsTr("Link copied"), ev.url, 6000)
        else
            say(qsTr("Couldn't share %1").arg(ev.file), ev.error, 8000)
    }

    // Shows the user's message and an empty answer that `chat` events fill in.
    function startChatTurn(turn, prompt, files) {
        // A new message replaces an answer still being written.
        for (let i = 0; i < chat.count; i++) {
            if (chat.get(i).status === "streaming")
                chat.setProperty(i, "status", "stopped")
        }
        chatTurn = turn
        const names = (files ?? []).map(path => path.split("/").pop()).join("\n")
        chat.append({ who: "user", body: prompt, status: "done", files: names, tool: "" })
        chat.append({ who: "claude", body: "", status: "streaming", files: "", tool: "" })
        chat.revision++
    }

    function addChatAttachments(paths) {
        chatAttachments = chatAttachments.concat(paths.filter(p => !chatAttachments.includes(p)))
    }

    function clearChat() {
        chat.clear()
        chatTurn = 0
        chat.revision++
    }

    function handleChat(ev) {
        const last = chat.count - 1
        if (ev.turn !== chatTurn || last < 0)
            return
        if (ev.state === "streaming") {
            chat.setProperty(last, "body", chat.get(last).body + ev.text)
            chat.setProperty(last, "tool", "")
        } else if (ev.state === "tool") {
            chat.setProperty(last, "tool", ev.text)
        } else {
            chat.setProperty(last, "tool", "")
            chat.setProperty(last, "status", ev.state)
            if (ev.state === "failed")
                chat.setProperty(last, "body", ev.text)
        }
        chat.revision++
        if (ev.state === "done" || ev.state === "failed")
            chatFinished(ev.state, chat.get(last).body)
    }

    function handleUsage(ev) {
        usage[ev.session_id] = { model: ev.model, cost_usd: ev.cost_usd, context_percent: ev.context_percent ?? null }
        usageRevision++
        if (!ev.five_hour && !ev.seven_day)
            return
        limits = { five_hour: ev.five_hour ?? null, seven_day: ev.seven_day ?? null }
        // Crossing 80% and 95% of the 5-hour window opens the island once per window.
        const w = ev.five_hour
        if (!w)
            return
        for (const threshold of [95, 80]) {
            const key = w.resets_at + ":" + threshold
            if (w.used_percent >= threshold && !limitWarned[key]) {
                limitWarned[w.resets_at + ":80"] = true
                limitWarned[key] = true
                say(qsTr("%1% of your 5-hour Claude limit used").arg(Math.round(w.used_percent)),
                    qsTr("Resets at %1").arg(Qt.formatTime(new Date(w.resets_at * 1000), "HH:mm")), 10000)
                notifying = true
                break
            }
        }
    }

    // Ten minutes before a meeting, and when it starts, the island opens to say so.
    function remindMeetings() {
        for (const m of meetings) {
            const minutes = (m.start * 1000 - now) / 60000
            const key = m.start + m.title
            const stage = minutes > 0 && minutes <= 10 ? "soon" : minutes <= 0 && minutes > -2 ? "now" : ""
            if (!stage || reminded[key] === stage || (stage === "soon" && reminded[key] === "now"))
                continue
            reminded[key] = stage
            if (stage === "soon")
                say(qsTr("In %1 min: %2").arg(Math.ceil(minutes)).arg(m.title), Qt.formatTime(new Date(m.start * 1000), "HH:mm"), 15000)
            else
                say(qsTr("Starting now: %1").arg(m.title), Qt.formatTime(new Date(m.start * 1000), "HH:mm"), 15000)
            notifying = true
        }
    }

    function showLevel(kind, value, muted) {
        level = { kind: kind, value: value, muted: muted }
        levelTimer.restart()
    }

    // Plugging in shows the charge; a low battery opens the island.
    function handleBattery(now) {
        const before = battery
        battery = now
        if (!before || !indicators)
            return
        if (now.plugged && !before.plugged) {
            showLevel("battery", now.percent / 100, false)
        } else if (!now.plugged) {
            for (const limit of [20, 10, 5]) {
                if (now.percent <= limit && before.percent > limit) {
                    say(qsTr("Battery low"), qsTr("%1% left").arg(Math.round(now.percent)), 8000)
                    notifying = true
                    break
                }
            }
        }
    }

    function addToShelf(paths) {
        shelf = shelf.concat(paths.filter(p => !shelf.includes(p)))
    }

    function removeFromShelf(path) {
        shelf = shelf.filter(p => p !== path)
    }

    // Local paths from dropped urls; anything that isn't a local file is skipped.
    function localPaths(urls) {
        const paths = []
        for (const url of urls) {
            const text = String(url)
            if (text.startsWith("file://"))
                paths.push(decodeURIComponent(text.slice("file://".length)))
        }
        return paths
    }

    // The request was answered (here or by timing out); drop its buttons.
    function closePermission(requestId) {
        for (const id in details) {
            const d = details[id]
            if (d.permission && d.permission.request_id === requestId) {
                d.permission = null
                const index = indexOf(id)
                if (index >= 0 && sessions.get(index).phase === "waiting")
                    sessions.setProperty(index, "phase", "tool")
                alertTimer.stop()
                sessions.revision++
                focusRevision++
            }
        }
    }

    function say(title, body, duration) {
        headline = title
        subline = body
        alertTimer.interval = duration
        alertTimer.restart()
    }

    property Timer alertTimer: Timer {
        onTriggered: notch.notifying = false
    }
    property Timer activityTimer: Timer { interval: 3500 }
    property Timer levelTimer: Timer { interval: 1600 }
    property Timer pauseGrace: Timer { interval: 30000 }

    property Timer happyTimer: Timer {
        interval: 3000
        onTriggered: notch.happy = false
    }

    // Wakes up once per minute, right after the minute changes.
    property Timer clock: Timer {
        interval: 60000 - Date.now() % 60000 + 50
        running: true
        onTriggered: {
            notch.time = Qt.formatTime(new Date(), "HH:mm")
            notch.now = Date.now()
            notch.remindMeetings()
            interval = 60000 - Date.now() % 60000 + 50
            restart()
        }
    }

    // Forget finished sessions after a while.
    property Timer sweeper: Timer {
        interval: 10000
        repeat: true
        running: notch.sessions.count > 0
        onTriggered: {
            const now = Date.now()
            for (let i = notch.sessions.count - 1; i >= 0; i--) {
                const s = notch.sessions.get(i)
                const age = now - s.updated
                if ((s.phase === "done" && age > 120000) || age > 30 * 60000) {
                    delete notch.details[s.sessionId]
                    notch.sessions.remove(i)
                }
            }
            if (notch.indexOf(notch.focusId) < 0)
                notch.focusId = notch.sessions.count > 0 ? notch.sessions.get(0).sessionId : ""
            notch.sessions.revision++
            notch.focusRevision++
        }
    }
}
