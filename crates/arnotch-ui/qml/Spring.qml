import QtQuick

// Qt's SpringAnimation, shown at the display's refresh rate. SpringAnimation steps its
// physics every 16 ms and does nothing in between, so on a 144 or 180 Hz display the value
// moves only on every second or third frame. This takes the same steps (so the spring feels
// the same) and shows each frame the value between the two steps it falls between.
// `value` follows `target`; a change of target mid-way keeps the momentum. `extent` is the
// furthest `value` goes in the current motion, overshoot included: a window sized by it
// changes size when a motion starts and ends, rather than on every frame.
//
// The time comes from the animation clock (`ticker` animates `clock`), which moves one
// refresh per frame (see app.cpp). FrameAnimation's frameTime would be the wall clock,
// which varies by a few ms from frame to frame, and the motion with it.
QtObject {
    id: motion

    property real target
    property real spring: 6
    property real damping: 0.38
    property real epsilon: 0.5
    readonly property real value: shown
    readonly property real extent: running ? reach : target
    readonly property bool running: ticker.running

    property real shown: 0
    property real position: 0
    property real previous: 0
    property real velocity: 0
    property real clock: 0 // ms since the motion started
    property real lastClock: 0
    // The animation clock counts whole ms, so 5.56 ms frames come as 5 and 6 ms. The
    // frames' average evens them out: { first: clock at the first frame, frames: since }
    property var pace: ({ first: -1, frames: 0 })
    property real pending: 0 // ms not yet stepped
    property real reach: 0
    property bool ready: false

    property NumberAnimation ticker: NumberAnimation {
        target: motion
        property: "clock"
        from: 0
        to: 3600000
        duration: 3600000
    }

    Component.onCompleted: {
        shown = position = previous = target
        ready = true
    }

    onTargetChanged: {
        if (!ready)
            return
        // The spring overshoots by about 5% of the distance; leave twice that.
        reach = Math.max(running ? reach : shown, target + Math.abs(target - shown) * 0.1)
        if (!running) {
            lastClock = 0
            pace = { first: -1, frames: 0 }
            ticker.start()
        }
    }

    onClockChanged: {
        if (!running)
            return
        let dt = clock - lastClock
        lastClock = clock
        if (pace.first < 0) {
            pace = { first: clock, frames: 0 }
        } else {
            pace.frames++
            const frame = (clock - pace.first) / pace.frames
            if (pace.frames >= 3 && frame > 0)
                dt = Math.max(1, Math.round(dt / frame)) * frame
        }
        // After a stall (a hidden window), carry on from where it was rather than jump.
        pending = Math.min(pending + dt, 80)
        while (pending >= 16) {
            pending -= 16
            previous = position
            velocity += motion.spring * (target - position) - damping * velocity
            position += velocity * 16 / 1000
            if (Math.abs(velocity) < epsilon && Math.abs(target - position) < epsilon) {
                shown = position = previous = target
                velocity = pending = 0
                ticker.stop()
                return
            }
        }
        shown = previous + (position - previous) * pending / 16
        if (shown > reach)
            reach = shown
    }
}
