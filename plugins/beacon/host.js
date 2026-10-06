// Beacon -- test fixture for the plugin event bus. Emits only.
//
// Announces "beat" every host tick, carrying a counter so the subscriber can
// prove ordering and that the payload survives the round trip.
//
// Also announces "unheard", which nothing subscribes to on purpose: that is
// the case where a mistyped event name vanishes without a trace.

let count = 0;

async function init() {
    log("info", "beacon starting");
}

async function tick() {
    count += 1;

    await announce("beat", { count, at: Date.now() });
    log("info", `announced beat ${count}`);

    // No subscriber for this one, by design.
    await announce("unheard", { count });
}
