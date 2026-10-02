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

    // Paths of the files kept on the shelf; Main persists them.
    property var shelf: []
    // Files being shared: [{ id, file, progress, state }]
    property var uploads: []
    readonly property bool uploading: uploads.length > 0
    // A plain notification is on screen; it takes precedence over the activity card.
    property bool notifying: false
    property string time: Qt.formatTime(new Date(), "HH:mm")

    // Claude asked for permission; Main switches the islands to the activity tab.
    signal permissionRequested(string requestId)

    // True while a new event wants the islands to open on their own.
    readonly property bool alerting: alertTimer.running
    // Like `alerting`, but for live activity (diffs, commands), which users may turn off.
    readonly property bool showingActivity: activityTimer.running

    readonly property string mood: happy ? "happy"
                                  : countPhase("waiting") > 0 ? "waiting"
                                  : countPhase("started") + countPhase("tool") > 0 ? "working"
                                  : "idle"

    readonly property int maxSteps: 5
    // Matches notchctl's default wait before Claude Code falls back to the terminal.
    readonly property int permissionTimeout: 45000

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
            say(ev.project, qsTr("Wants to run %1").arg(ev.tool), permissionTimeout)
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
            d.permission = { request_id: ev.request_id, tool: ev.tool, detail: ev.detail, diff: ev.diff ?? null }
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
