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

    // The running timer: { label, endsAt, total } in milliseconds, or null. Main keeps it
    // in the settings, so it survives a restart. While focusing (Pomodoro) it also has
    // focus: "work" | "break" and round: 1, 2, ...
    property var timer: null
    readonly property bool focusing: timer !== null && timer.focus === "work"
    readonly property string timerIcon: !timer ? "" : timer.focus === "work" ? "🎯" : timer.focus === "break" ? "☕" : "⏱"
    readonly property color timerColor: !timer || !timer.focus ? "#ffb020" : timer.focus === "work" ? "#ff6b5c" : "#34c759"
    // Main binds these to the settings: minutes of focus, of a break, and of the long
    // break after every fourth round.
    property int focusMinutes: 25
    property int breakMinutes: 5
    property int longBreakMinutes: 15
    // Main binds this to the setting: hold back notifications that can wait while focusing.
    property bool quietFocus: true
    readonly property bool quiet: focusing && quietFocus
    // Titles of the notifications held back while focusing, shown when the break starts
    property var missed: []

    // Downloads and transfers in progress: [{ id, title, file, percent, speed,
    // processed_bytes, total_bytes, suspended }]
    property var jobs: []
    // When each job was first seen, by id
    property var jobStarts: ({})
    // Scripts' progress (arnotchctl progress): [{ id, title, detail, percent (null: busy),
    // state: "running" | "done" | "failed", updated }]
    property var scripts: []
    // Transfers and scripts that just ended, shown for a moment (with Open buttons):
    // activities as below, with ended: Date.now()
    property var ended: []
    // Everything in progress, as the island shows it, then what just ended: [{ key,
    // kind: "download" | "copy" | "script", name, status, percent (0..100, -1: busy),
    // speed, processed, total, state: "running" | "paused" | "done" | "failed", path }]
    readonly property var activities: {
        const running = jobs.map(jobActivity).concat(scripts.filter(s => s.state === "running").map(scriptActivity))
        const keys = running.map(a => a.key)
        return running.concat(ended.filter(a => !keys.includes(a.key)))
    }
    readonly property var runningActivities: activities.filter(a => a.state === "running" || a.state === "paused")
    // All of them together, from 0 to 1 (transfers by their bytes); -1 when none can tell
    readonly property real activitiesProgress: {
        const known = runningActivities.filter(a => a.percent >= 0)
        if (known.length === 0)
            return -1
        const total = known.reduce((sum, a) => sum + a.total, 0)
        if (known.every(a => a.total > 0))
            return known.reduce((sum, a) => sum + a.processed, 0) / total
        return known.reduce((sum, a) => sum + a.percent, 0) / known.length / 100
    }
    // Apps using the microphone, the camera or recording the screen, by name
    property var privacy: ({ microphone: [], camera: [], screen: [] })
    readonly property bool privacyInUse: privacy.microphone.length + privacy.camera.length + privacy.screen.length > 0
    // The microphones there are: [{ name, description, external, filter }]
    property var microphones: []
    // Asking Claude by voice: { state: "listening" | "transcribing", level } or null
    property var voice: null
    readonly property bool listening: voice !== null && voice.state === "listening"
    // Claude's answer is being read aloud
    property bool speaking: false
    // Talking with Claude by voice: after each answer, it listens again, until the user
    // says nothing (or "that's all"), or ends it
    property bool conversing: false
    // The latest loudness levels while listening, oldest first
    property var voiceLevels: []
    // Where the conversation is: "listening", "transcribing", "thinking", "speaking" or ""
    readonly property string conversation: !conversing ? ""
                                         : voice !== null ? voice.state
                                         : speaking ? "speaking"
                                         : chatBusy ? "thinking" : ""
    // How the faces look: listening and speaking come before the agents' mood
    readonly property string faceMood: listening ? "listening" : speaking ? "speaking"
                                     : conversation === "thinking" || conversation === "transcribing" ? "working" : mood

    // { place, temperature, high, low, code, is_day } or null
    property var weather: null
    // Milliseconds since the epoch, every second while a timer runs
    property real timerNow: Date.now()
    // A timer ran out: Main plays a sound.
    signal timerFinished(string label)

    // The phone (KDE Connect) in reach: { device, percent, charging }, null when none
    property var phone: null
    // Its latest calls and messages since arnotch started, newest first:
    // [{ kind: "call" | "message", state: "ringing" | "talking" | "missed_call", who, body, at }]
    property var phoneLog: []
    // Main binds this to the setting.
    property bool phoneAlerts: true

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
    // per line, tool: the tool Claude is using, changes: the files Claude changed, a JSON
    // array of diffs ({ file, lines, truncated }) }. A failed answer's body is the error.
    // A ListModel, so a new piece of the answer updates one row instead of rebuilding them all.
    property ListModel chat: ListModel {
        // ListModel is not reactive inside functions, so bindings depend on `revision`.
        property int revision: 0
    }
    // The chat is about mail: Claude reads Gmail and saves drafts, until a new chat
    property bool chatGmail: false
    // The Gmail inbox, newest first (see MailView), and what went wrong checking it
    property var mails: []
    property string mailError: ""
    readonly property int unreadMails: mails.filter(m => m.unread).length
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
    signal permissionRequested(string requestId, string sessionId)
    // It was answered, here or elsewhere, or timed out.
    signal permissionClosed(string requestId)
    // Claude finished answering in the chat ("done" or "failed"), with the answer or error.
    signal chatFinished(string status, string body)
    // A question was asked by voice: Main sends it to Claude.
    signal voiceAsked(string text)
    // The answer to a chat turn has been read aloud (or couldn't be).
    signal speechEnded(int turn)

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
            if (say(ev.title, ev.body, 5000, true))
                notifying = true
        } else if (ev.type === "upload") {
            handleUpload(ev)
        } else if (ev.type === "level") {
            if (indicators)
                showLevel(ev.kind, ev.value, ev.muted)
        } else if (ev.type === "battery") {
            handleBattery(ev.battery)
        } else if (ev.type === "timer") {
            startTimer(ev.seconds, ev.label)
        } else if (ev.type === "timer_cancel") {
            stopTimer()
        } else if (ev.type === "focus") {
            startFocus(ev.minutes ?? focusMinutes)
        } else if (ev.type === "jobs") {
            handleJobs(ev.jobs)
        } else if (ev.type === "progress") {
            handleProgress(ev)
        } else if (ev.type === "privacy") {
            privacy = { microphone: ev.microphone, camera: ev.camera, screen: ev.screen }
        } else if (ev.type === "microphones") {
            microphones = ev.inputs
        } else if (ev.type === "voice") {
            handleVoice(ev)
        } else if (ev.type === "speech") {
            if (ev.turn !== chatTurn)
                return
            speaking = ev.speaking
            if (!ev.speaking)
                speechEnded(ev.turn)
        } else if (ev.type === "weather") {
            weather = ev.weather ?? null
        } else if (ev.type === "phone") {
            handlePhone(ev)
        } else if (ev.type === "usage") {
            handleUsage(ev)
        } else if (ev.type === "calendar") {
            meetings = ev.meetings
            remindMeetings()
        } else if (ev.type === "clipboard") {
            clipboard = ev.items
        } else if (ev.type === "mail") {
            handleMail(ev)
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
            say(ev.project, qsTr("Finished"), 4000, true)
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

    // New mail opens the island, unless focusing holds it back.
    function handleMail(ev) {
        mails = ev.mails
        mailError = ev.error ?? ""
        const arrived = ev.arrived ?? []
        if (arrived.length === 0)
            return
        const bounced = arrived.filter(m => m.bounce)
        let said
        if (arrived.length === 1) {
            const m = arrived[0]
            said = say(m.bounce ? qsTr("Mail returned") : m.from, m.subject || m.snippet, 6000, true)
        } else {
            said = say(qsTr("%n new mail(s)", "", arrived.length),
                       bounced.length ? qsTr("%n returned", "", bounced.length) : arrived.map(m => m.from).join(", "),
                       6000, true)
        }
        if (said)
            notifying = true
    }

    // Shows the user's message and an empty answer that `chat` events fill in; `mode` is
    // the permission mode it is answered in.
    function startChatTurn(turn, prompt, files, mode) {
        // A new message replaces an answer still being written.
        for (let i = 0; i < chat.count; i++) {
            if (chat.get(i).status === "streaming")
                chat.setProperty(i, "status", "stopped")
        }
        chatTurn = turn
        const names = (files ?? []).map(path => path.split("/").pop()).join("\n")
        chat.append({ who: "user", body: prompt, status: "done", files: names, tool: "", model: "", changes: "[]", mode: mode })
        chat.append({ who: "claude", body: "", status: "streaming", files: "", tool: "", model: "", changes: "[]", mode: mode })
        chat.revision++
    }

    function addChatAttachments(paths) {
        chatAttachments = chatAttachments.concat(paths.filter(p => !chatAttachments.includes(p)))
    }

    function clearChat() {
        chatGmail = false
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
        } else if (ev.state === "model") {
            chat.setProperty(last, "model", ev.text)
        } else if (ev.state === "change") {
            // Kept as JSON: a ListModel role holds one type for every row.
            const changes = JSON.parse(chat.get(last).changes)
            changes.push(JSON.parse(ev.text))
            chat.setProperty(last, "changes", JSON.stringify(changes))
            chat.setProperty(last, "tool", "")
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

    function startTimer(seconds, label) {
        timerNow = Date.now()
        timer = { label: label ?? "", endsAt: timerNow + seconds * 1000, total: seconds * 1000 }
    }

    function stopTimer() {
        const wasFocusing = focusing
        timer = null
        if (wasFocusing)
            sayMissed(qsTr("Focus ended"))
    }

    // Pomodoro: `minutes` of focus, a break, focus again, ... until stopped.
    function startFocus(minutes) {
        missed = []
        startFocusPhase("work", 1, minutes)
    }

    function startFocusPhase(phase, round, workMinutes) {
        const longBreak = phase === "break" && round % 4 === 0
        const minutes = phase === "work" ? workMinutes : longBreak ? longBreakMinutes : breakMinutes
        timerNow = Date.now()
        timer = {
            label: phase === "work" ? qsTr("Focus · round %1").arg(round) : longBreak ? qsTr("Long break") : qsTr("Break"),
            endsAt: timerNow + minutes * 60000,
            total: minutes * 60000,
            focus: phase,
            round: round,
            workMinutes: workMinutes
        }
    }

    // On to the next phase: the work's end (or a skip) starts a break, a break's end the next round.
    function nextFocusPhase() {
        if (!timer || !timer.focus)
            return
        const t = timer
        if (t.focus === "work") {
            startFocusPhase("break", t.round, t.workMinutes)
            sayMissed("☕ " + timer.label + " · " + qsTr("%1 min").arg(Math.round(timer.total / 60000)))
        } else {
            startFocusPhase("work", t.round + 1, t.workMinutes)
            say("🎯 " + timer.label, qsTr("until %1").arg(Qt.formatTime(new Date(timer.endsAt), "HH:mm")), 6000)
            notifying = true
        }
    }

    // The end of a focus: what was held back meanwhile.
    function sayMissed(title) {
        const n = missed.length
        const body = n === 0 ? qsTr("Nothing came in meanwhile")
                   : n === 1 ? qsTr("Meanwhile: %1").arg(missed[0])
                   : qsTr("%1 notifications meanwhile: %2").arg(n).arg(missed.slice(-3).reverse().join(", "))
        missed = []
        say(title, body, n > 0 ? 12000 : 6000)
        notifying = true
    }

    // The weather's symbol and description, from its WMO code.
    function weatherIcon(w) {
        if (!w)
            return ""
        const c = w.code
        return c === 0 ? (w.is_day ? "☀️" : "🌙") : c === 1 ? (w.is_day ? "🌤️" : "🌙") : c === 2 ? "⛅"
             : c === 3 ? "☁️" : c === 45 || c === 48 ? "🌫️" : c >= 95 ? "⛈️"
             : (c >= 71 && c <= 77) || c === 85 || c === 86 ? "🌨️"
             : c >= 51 && c <= 57 || c >= 80 ? "🌦️" : "🌧️"
    }

    function weatherText(code) {
        return code === 0 ? qsTr("Clear") : code === 1 ? qsTr("Mostly clear") : code === 2 ? qsTr("Partly cloudy")
             : code === 3 ? qsTr("Overcast") : code === 45 || code === 48 ? qsTr("Fog")
             : code >= 51 && code <= 57 ? qsTr("Drizzle") : code >= 61 && code <= 67 ? qsTr("Rain")
             : code >= 71 && code <= 77 ? qsTr("Snow") : code >= 80 && code <= 82 ? qsTr("Showers")
             : code === 85 || code === 86 ? qsTr("Snow showers") : code >= 95 ? qsTr("Thunderstorm") : ""
    }

    function handleJobs(now) {
        const at = Date.now()
        const starts = {}
        for (const job of now)
            starts[job.id] = jobStarts[job.id] ?? at
        // A transfer that reached the end stays a moment, to open it; one that ran a while
        // also says so. Cancelled ones just go.
        for (const job of jobs) {
            if (job.id in starts || job.percent < 99)
                continue
            const activity = Object.assign(jobActivity(job), { state: "done", percent: 100, speed: 0 })
            addEnded(activity)
            if (at - jobStarts[job.id] > 5000 && say(activity.kind === "download" ? qsTr("Downloaded") : qsTr("Copied"),
                                                     activity.name, 5000, true))
                notifying = true
        }
        jobStarts = starts
        jobs = now
    }

    function handleProgress(ev) {
        const others = scripts.filter(s => s.id !== ev.id)
        const before = scripts.find(s => s.id === ev.id)
        const script = { id: ev.id, title: ev.title || (before ? before.title : ev.id), detail: ev.detail,
                         percent: ev.percent ?? null, state: ev.state, updated: Date.now(),
                         started: before ? before.started : Date.now() }
        if (ev.state === "running") {
            scripts = others.concat([script])
            return
        }
        scripts = others
        addEnded(Object.assign(scriptActivity(script), { state: ev.state, percent: ev.state === "done" ? 100 : script.percent ?? 0 }))
        if (say(ev.state === "done" ? qsTr("✓ %1").arg(script.title) : qsTr("✗ %1").arg(script.title),
                script.detail || (ev.state === "done" ? qsTr("Done") : qsTr("Failed")), ev.state === "done" ? 4000 : 8000,
                ev.state === "done"))
            notifying = true
    }

    function addEnded(activity) {
        ended = ended.filter(a => a.key !== activity.key).concat([Object.assign(activity, { ended: Date.now() })])
    }

    // "Downloading", "İndiriliyor", "Copying", "Kopyalanıyor", ... as KDE names its jobs
    function jobActivity(job) {
        // ("İ" doesn't match "i" case-insensitively.)
        const download = /download|[iİ]ndir|herunterlad|télécharg|descarg/i.test(job.title)
        return {
            key: "job:" + job.id,
            kind: download ? "download" : "copy",
            name: job.file || job.title,
            status: job.title,
            percent: job.percent,
            speed: job.speed,
            processed: job.processed_bytes,
            total: job.total_bytes,
            state: job.suspended ? "paused" : "running",
            path: job.path ?? ""
        }
    }

    function scriptActivity(script) {
        return {
            key: "script:" + script.id,
            kind: "script",
            name: script.title,
            status: script.detail,
            percent: script.percent === null ? -1 : script.percent,
            speed: 0,
            processed: 0,
            total: 0,
            state: script.state,
            path: ""
        }
    }

    function handleVoice(ev) {
        if (ev.state === "listening" || ev.state === "transcribing") {
            if (!voice || voice.state !== ev.state)
                voiceLevels = []
            voice = { state: ev.state, level: ev.level ?? 0 }
            if (ev.state === "listening")
                voiceLevels = voiceLevels.concat([ev.level ?? 0]).slice(-24)
            return
        }
        voice = null
        voiceLevels = []
        if (ev.state === "done") {
            voiceAsked(ev.text)
        } else {
            // Nothing said: the conversation is over.
            conversing = false
            if (ev.state === "failed") {
                say(qsTr("Couldn't hear you"), ev.text, 10000)
                notifying = true
            }
        }
    }

    // The answer to a permission request said aloud: "allow" for "evet", "olur, çalıştır",
    // "deny" for "hayır", "yok, dur"; "" when it is neither.
    function permissionAnswer(text) {
        const said = text.toLocaleLowerCase(Qt.locale("tr_TR")).replace(/[^\p{L}\s']/gu, " ").trim().replace(/\s+/g, " ")
        const starts = words => words.some(w => said === w || said.startsWith(w + " "))
        // "Hayır" first: "hayır, çalıştırma" is no, whatever follows.
        if (starts(["hayır", "yok", "olmaz", "istemiyorum", "reddet", "iptal", "dur", "vazgeç", "çalıştırma",
                    "yapma", "no", "nope", "deny", "cancel", "don't", "dont", "stop"]))
            return "deny"
        if (starts(["evet", "olur", "tamam", "onayla", "onaylıyorum", "izin ver", "çalıştır", "yap", "devam",
                    "peki", "tabii", "tabi", "he", "aynen", "yes", "yeah", "yep", "ok", "okay", "sure",
                    "go ahead", "allow", "run it", "do it"]))
            return "allow"
        return ""
    }

    // "That's all", "teşekkürler", ...: words that end a conversation instead of asking.
    function endsConversation(text) {
        const said = text.toLocaleLowerCase(Qt.locale("tr_TR")).replace(/[^\p{L}\s']/gu, "").trim().replace(/\s+/g, " ")
        return ["bu kadar", "tamam bu kadar", "yeter", "sus", "kapat", "bitti", "görüşürüz", "teşekkürler",
                "teşekkür ederim", "sağ ol", "sağol", "tamam teşekkürler", "that's all", "thats all", "thanks",
                "thank you", "stop", "bye", "goodbye"].includes(said)
    }

    function extendTimer(seconds) {
        if (timer)
            timer = Object.assign({}, timer, { endsAt: timer.endsAt + seconds * 1000, total: timer.total + seconds * 1000 })
    }

    function handlePhone(ev) {
        if (ev.kind === "battery") {
            const before = phone
            phone = { device: ev.device, percent: ev.percent, charging: ev.charging }
            if (phoneAlerts && before && !ev.charging && ev.percent <= 15 && before.percent > 15) {
                say(qsTr("%1: battery low").arg(ev.device), qsTr("%1% left").arg(ev.percent), 8000)
                notifying = true
            }
            return
        }
        if (ev.kind === "gone") {
            phone = null
            return
        }
        logPhone(ev)
        if (!phoneAlerts)
            return
        if (ev.kind === "call") {
            const who = ev.contact || ev.number || qsTr("Unknown number")
            const ringing = qsTr("📞 Calling: %1").arg(who)
            if (ev.state === "talking") {
                // Answered: the ringing note goes away.
                if (headline === ringing && alertTimer.running) {
                    alertTimer.stop()
                    notifying = false
                }
                return
            }
            if (ev.state === "ringing")
                say(ringing, ev.device, 30000)
            else if (ev.state !== "missed_call" || !say(qsTr("Missed call: %1").arg(who), ev.device, 15000, true))
                return
            notifying = true
        } else if (ev.kind === "message") {
            if (say(qsTr("💬 %1").arg(ev.from || qsTr("Message")), ev.body, 10000, true))
                notifying = true
        }
    }

    // A ringing call is listed once; answering or missing it updates that entry.
    function logPhone(ev) {
        const at = Date.now()
        if (ev.kind === "message") {
            phoneLog = [{ kind: "message", state: "", who: ev.from || qsTr("Message"), body: ev.body, at: at }]
                       .concat(phoneLog).slice(0, 30)
            return
        }
        const who = ev.contact || ev.number || qsTr("Unknown number")
        const last = phoneLog.find(e => e.kind === "call")
        if (ev.state !== "ringing" && last && last.who === who && last.state === "ringing" && at - last.at < 600000) {
            phoneLog = phoneLog.map(e => e === last ? Object.assign({}, e, { state: ev.state }) : e)
            return
        }
        phoneLog = [{ kind: "call", state: ev.state, who: who, body: "", at: at }].concat(phoneLog).slice(0, 30)
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
                if (say(qsTr("%1% of your 5-hour Claude limit used").arg(Math.round(w.used_percent)),
                        qsTr("Resets at %1").arg(Qt.formatTime(new Date(w.resets_at * 1000), "HH:mm")), 10000, true))
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
        permissionClosed(requestId)
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

    // Shows a message for `duration` ms, opening the island. One that `canWait` is held
    // back while focusing (false is returned then) and listed when the break starts.
    function say(title, body, duration, canWait) {
        if (canWait && quiet) {
            missed = missed.concat([title])
            return false
        }
        headline = title
        subline = body
        alertTimer.interval = duration
        alertTimer.restart()
        return true
    }

    property Timer alertTimer: Timer {
        onTriggered: notch.notifying = false
    }
    property Timer activityTimer: Timer { interval: 3500 }
    property Timer levelTimer: Timer { interval: 1600 }
    property Timer pauseGrace: Timer { interval: 30000 }

    // Ended transfers go after a few seconds; scripts that stopped reporting after 10 minutes.
    property Timer activitySweeper: Timer {
        interval: 1000
        repeat: true
        running: notch.ended.length > 0 || notch.scripts.length > 0
        onTriggered: {
            const now = Date.now()
            const ended = notch.ended.filter(a => now - a.ended < 8000)
            if (ended.length !== notch.ended.length)
                notch.ended = ended
            const scripts = notch.scripts.filter(s => now - s.updated < 600000)
            if (scripts.length !== notch.scripts.length)
                notch.scripts = scripts
        }
    }

    // Ticks the countdown, and rings when the timer runs out.
    property Timer timerTick: Timer {
        interval: 1000
        repeat: true
        running: notch.timer !== null
        triggeredOnStart: true
        onTriggered: {
            notch.timerNow = Date.now()
            if (notch.timer && notch.timerNow >= notch.timer.endsAt) {
                const label = notch.timer.label
                notch.timerFinished(label)
                if (notch.timer.focus) {
                    notch.nextFocusPhase()
                    return
                }
                notch.timer = null
                notch.say(qsTr("⏰ Time's up"), label || qsTr("Timer"), 20000)
                notch.notifying = true
            }
        }
    }

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
