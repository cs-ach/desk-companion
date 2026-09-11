// Earthquake Watch -- host side.
//
// A reference for the plugin lifecycle:
//
//   init()  runs once, after host.js is evaluated. It establishes the baseline
//           that every later tick compares against.
//   tick()  runs every `tickIntervalMs`. It reports only what CHANGED since the
//           last run, which is the whole reason init() exists separately.
//
// Both are `async`. The host awaits the returned promise before continuing, so
// work that happens after an `await` still counts as part of the hook, and a
// rejection surfaces as a host-side error instead of disappearing.

// `config` is a host-provided global holding this plugin's resolved settings:
// manifest defaults layered under whatever is in config.json. Secrets appear as
// a "{{key}}" placeholder, never as plaintext.
const FEED = config.feedUrl;
const MIN_MAGNITUDE = config.minMagnitude;

// Module-level state survives from init() through every tick(): the host keeps
// one JS context alive per plugin for its entire lifetime, and calls into the
// same globals each cycle. This is where a plugin's memory lives.
let seen = new Set();
let startedAt = 0;
let ticks = 0;

async function fetchQuakes() {
    const res = await fetch(FEED, { headers: { Accept: "application/json" } });

    // fetch() resolves for any HTTP status -- it only rejects on a transport
    // failure -- so a 4xx/5xx has to be checked explicitly.
    if (!res.ok) {
        throw new Error(`feed returned ${res.status}`);
    }

    const feed = JSON.parse(res.body);
    return (feed.features || [])
        .map((f) => ({
            id: f.id,
            mag: f.properties.mag ?? 0,
            place: f.properties.place ?? "unknown location",
            time: f.properties.time ?? 0,
        }))
        .filter((q) => q.mag >= MIN_MAGNITUDE)
        .sort((a, b) => b.time - a.time); // newest first
}

function describe(q) {
    return `M${q.mag.toFixed(1)} ${q.place}`;
}

async function init() {
    startedAt = Date.now();
    log("info", `watching for M${MIN_MAGNITUDE.toFixed(1)}+ quakes`);

    try {
        const quakes = await fetchQuakes();
        seen = new Set(quakes.map((q) => q.id));

        log("info", `baseline: ${seen.size} event(s) in the last hour`);
        if (quakes.length > 0) {
            log("info", `most recent: ${describe(quakes[0])}`);
        }
    } catch (e) {
        // Deliberately NOT fatal. The host treats a rejected init() as fatal and
        // stops the plugin, so only throw from here for setup that cannot be
        // retried -- a bad config value, a missing capability. A cold network is
        // not that: the next tick establishes the baseline instead.
        log("warn", `no baseline (${e}); the first tick will establish one`);
    }
}

async function tick() {
    ticks += 1;
    const upMinutes = Math.floor((Date.now() - startedAt) / 60000);
    const prefix = `tick ${ticks} (up ${upMinutes}m)`;

    try {
        const quakes = await fetchQuakes();
        const fresh = quakes.filter((q) => !seen.has(q.id));

        // Rebuild rather than union: the feed is a rolling one-hour window, so
        // this drops ids that have aged out and keeps `seen` bounded by the feed
        // size instead of growing for as long as the plugin runs.
        seen = new Set(quakes.map((q) => q.id));

        if (fresh.length === 0) {
            log("info", `${prefix}: nothing new, ${seen.size} tracked`);
        } else {
            log("info", `${prefix}: ${fresh.length} new of ${seen.size} tracked`);
            for (const q of fresh) {
                log("info", `  ${describe(q)}`);
            }
        }
    } catch (e) {
        // A flaky network should cost one cycle, not the plugin. Catching here
        // keeps that true regardless of how the host treats a rejected tick();
        // once the host logs-and-continues on its own, this can just throw.
        log("error", `${prefix} failed: ${e}`);
    }
}
