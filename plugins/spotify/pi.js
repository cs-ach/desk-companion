// Spotify -- device side.

const PREDICTION_MS = 4000;

let state = null;
let elapsed = 0;
let pending = null;
let artUrl = null;
let artData = null;

async function init() {
    log("info", "spotify ready");
}

async function loadArt(url) {
    if (!url || url === artUrl) {
        return;
    }

    try {
        const res = await fetch(url, { binary: true });

        if (!res.ok) {
            log("warn", `cover returned ${res.status}`);
            return;
        }

        artUrl = url;
        artData = res.body;
    } catch (e) {
        log("warn", `cover failed: ${e}`);
    }
}

function view(extra) {
    return {
        headline: state.headline,
        detail: state.detail,
        elapsedMs: elapsed,
        totalMs: state.totalMs,
        advancing: state.advancing,
        playing: state.meta === "playing",
        hasImage: !!artData,
        ...extra,
    };
}

function stale(next) {
    if (!pending || Date.now() > pending.until) {
        pending = null;
        return false;
    }

    if (next.corrected) {
        pending = null;
        return false;
    }

    if (pending.playing !== undefined) {
        return (next.meta === "playing") !== pending.playing;
    }

    return next.headline === pending.headline;
}

async function onState(name, next) {
    if (stale(next)) {
        return;
    }

    pending = null;

    const changed = next.art !== artUrl;

    state = next;
    elapsed = next.elapsedMs || 0;

    await loadArt(next.art);

    await render(view(changed && artData ? { image: artData } : null));
}

async function tick() {
    if (!state || !state.advancing || !state.totalMs) {
        return;
    }

    elapsed = Math.min(elapsed + 1000, state.totalMs);
    await render(view());
}

async function predict(name) {
    if (!state) {
        return;
    }

    const until = Date.now() + PREDICTION_MS;

    switch (name) {
        case "pause":
            state = { ...state, advancing: false, meta: "paused" };
            pending = { playing: false, until };
            break;
        case "play":
            state = { ...state, advancing: true, meta: "playing" };
            pending = { playing: true, until };
            break;
        case "next":
        case "previous":
            elapsed = 0;
            state = { ...state, advancing: false, elapsedMs: 0 };
            pending = { headline: state.headline, until };
            break;
        default:
            return;
    }

    await render(view());
}

async function onAction(name) {
    const action = (name === "nowplaying" || name === "tap")
        ? (state && state.meta === "playing" ? "pause" : "play")
        : name;

    await predict(action);
    await sendAction(action);
}
