// Echo -- test fixture for the plugin event bus. Receives only.
//
// Subscribes to beacon's "beat" and logs what arrives. No tick(), so this
// plugin is idle until an event reaches it -- which makes the log a clean
// record of the bus and nothing else.

let seen = 0;

async function init() {
    log("info", "echo starting, waiting for beacon/beat");
}

// name is the event as announced; payload is whatever the emitter passed.
async function onEvent(name, payload) {
    seen += 1;

    const count = payload && payload.count;
    log("info", `received ${name} #${count} (${seen} so far): ${JSON.stringify(payload)}`);
}
