// Pulse -- device side.
//
// onState() is called with whatever host.js last sent. Logging it here is the
// cheapest way to confirm the link end to end: if this line appears, the whole
// path from host.js through the serial link into the sandbox is working.

async function init() {
    log("info", "pulse ready");
}

async function onState(state) {
    log("info", `state: count=${state.count} label=${state.label} up=${state.uptime}`);
}
