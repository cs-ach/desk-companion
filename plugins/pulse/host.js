// Pulse -- a heartbeat for the state pipeline.
//
// It fetches nothing and depends on nothing, so whatever reaches the device is
// what this file sent. That makes it the thing to reach for when state stops
// arriving and you need to know which half is at fault.

let count = 0;
let startedAt = 0;

function uptime() {
    const seconds = Math.floor((Date.now() - startedAt) / 1000);
    if (seconds < 60) return `${seconds}s`;
    const minutes = Math.floor(seconds / 60);
    if (minutes < 60) return `${minutes}m`;
    return `${Math.floor(minutes / 60)}h`;
}

// The keys match the `in` properties of src/widgets/counter.slint. The payload
// crosses the link as JSON, so numbers stay numbers -- `count` is sent as one to
// prove that survives the trip.
async function publish() {
    await sendState({
        count: count,
        label: `tick ${count}`,
        uptime: uptime(),
    });
}

async function init() {
    startedAt = Date.now();
    log("info", "pulse started");
    await publish();
}

async function tick() {
    count += 1;
    log("info", `tick ${count} (up ${uptime()})`);
    await publish();
}
