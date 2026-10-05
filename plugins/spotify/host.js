// Spotify -- host side.
//
// Access tokens last an hour, so this keeps one in memory and refreshes it
// when it expires. The refresh token is long-lived and lives in secrets.json;
// it is never visible here -- config.refreshToken is the literal
// "{{refreshToken}}" and fetch() swaps in the real value on its way out.
//
// No client secret: the token was obtained with PKCE, so refreshing takes the
// client id alone.

const TOKEN_URL = "https://accounts.spotify.com/api/token";
const API = "https://api.spotify.com/v1/me/player";

let token = null;
let expiresAt = 0;
let needsAuth = false;
let current = null;

async function accessToken() {
    if (token && Date.now() < expiresAt) {
        return token;
    }

    const res = await fetch(TOKEN_URL, {
        method: "POST",
        headers: { "Content-Type": "application/x-www-form-urlencoded" },
        body: [
            "grant_type=refresh_token",
            `refresh_token=${config.refreshToken}`,
            `client_id=${encodeURIComponent(config.clientId)}`,
        ].join("&"),
    });

    if (!res.ok) {
        // invalid_grant means the refresh token was revoked or never existed.
        // No amount of retrying fixes that -- someone has to sign in again, so
        // say so on the device instead of failing silently every 3 seconds.
        if (res.body && res.body.indexOf("invalid_grant") !== -1) {
            needsAuth = true;
            throw new Error("refresh token rejected -- tap Connect to sign in again");
        }
        throw new Error(`token refresh returned ${res.status}`);
    }

    const parsed = JSON.parse(res.body);
    token = parsed.access_token;
    needsAuth = false;

    // Spotify may hand back a rotated refresh token; keep the newest one.
    if (parsed.refresh_token) {
        log("info", "refresh token rotated, but only auth.js can store it");
    }

    // 30s of slack so a request never starts against an almost-dead token.
    expiresAt = Date.now() + (parsed.expires_in - 30) * 1000;

    log("info", "refreshed access token");
    return token;
}

async function call(path, method, body) {
    const options = {
        method: method || "GET",
        headers: { Authorization: `Bearer ${await accessToken()}` },
    };

    if (body !== undefined) {
        options.headers["Content-Type"] = "application/json";
        options.body = JSON.stringify(body);
    }

    const res = await fetch(`${API}${path}`, options);

    // 204 means "nothing playing" for GET, and "accepted" for the controls.
    if (res.status === 204) return null;

    if (!res.ok) {
        throw new Error(`${method || "GET"} ${path} returned ${res.status}`);
    }

    return res.body ? JSON.parse(res.body) : null;
}

async function wakeableDevice() {
    const data = await call("/devices");
    const devices = (data && data.devices) || [];

    const chosen = devices.find((d) => d.is_active) || devices[0];
    return chosen ? chosen.id : null;
}

async function targeted(path) {
    if (current && current.item) {
        return path;
    }

    const id = await wakeableDevice();
    if (!id) {
        throw new Error("no Spotify device is available to play on");
    }

    return `${path}?device_id=${encodeURIComponent(id)}`;
}

function clock(ms) {
    const total = Math.max(0, Math.floor(ms / 1000));
    return `${Math.floor(total / 60)}:${String(total % 60).padStart(2, "0")}`;
}

// The shell draws these and sends the name back as an action when one is
// tapped. Only the names and glyphs come from here; the layout is the shell's,
// so a plugin cannot break the page.
function controls(isPlaying) {
    return [
        { name: "previous", glyph: "prev" },
        { name: isPlaying ? "pause" : "play", glyph: isPlaying ? "pause" : "play", active: true },
        { name: "next", glyph: "next" },
    ];
}

async function publish(playing, corrected) {
    current = playing;

    if (!playing || !playing.item) {
        await sendState("onState", {
            corrected: !!corrected,
            value: "--",
            unit: "",
            headline: needsAuth ? "Not connected" : "Nothing playing",
            detail: needsAuth ? "Tap Connect in settings" : "",
            elapsedMs: 0,
            totalMs: 0,
            advancing: false,
            actions: needsAuth ? [] : controls(false),
        });
        return;
    }

    const track = playing.item;
    const artists = (track.artists || []).map((a) => a.name).join(", ");
    const album = track.album ? track.album.name : "";

    // The device advances elapsedMs itself between updates, so this only has
    // to be accurate, not frequent.
    await sendState("onState", {
        corrected: !!corrected,
        value: clock(playing.progress_ms || 0),
        unit: clock(track.duration_ms || 0),
        elapsedMs: playing.progress_ms || 0,
        totalMs: track.duration_ms || 0,
        advancing: !!playing.is_playing,
        headline: track.name || "unknown track",
        detail: album ? `${artists} · ${album}` : artists || "unknown artist",
        meta: playing.is_playing ? "playing" : "paused",
        actions: controls(playing.is_playing),
        art: (track.album && track.album.images && track.album.images[0])
            ? track.album.images[0].url
            : "",
    });
}

async function init() {
    log("info", "spotify starting");

    if (!config.clientId) {
        log("warn", "clientId is not set");
    }

    try {
        await publish(await call("/currently-playing"));
    } catch (e) {
        log("error", `first poll failed: ${e}`);
        await publish(null);
    }
}

async function tick() {
    try {
        let data = await call("/currently-playing");
        await publish(data);
    } catch (e) {
        log("error", `poll failed: ${e}`);
    }
}

const SETTLE_ATTEMPTS = 4;

async function settled(done) {
    let playing = null;

    for (let i = 0; i < SETTLE_ATTEMPTS; i++) {
        playing = await call("/currently-playing");

        if (done(playing)) {
            return playing;
        }
    }

    log("warn", "state did not settle, publishing what we have");
    return playing;
}

async function resume() {
    const path = await targeted("/play");

    try {
        await call(path, "PUT");
        return;
    } catch (e) {
        log("info", `resume failed (${e}), transferring playback instead`);
    }

    const id = await wakeableDevice();
    if (!id) {
        throw new Error("no Spotify device is available to play on");
    }

    await call("", "PUT", { device_ids: [id], play: true });
}

// Actions arrive from the device: the widget or a control is tapped, pi.js
// calls sendAction(), and it lands here.
async function onAction(name) {
    try {
        const previousId = current && current.item ? current.item.id : null;

        const changedTrack = (p) => !!(p && p.item && p.item.id !== previousId);
        const isPlaying = (want) => (p) => !!p && p.is_playing === want;

        let done;

        switch (name) {
            case "next":
                done = changedTrack;
                await call(await targeted("/next"), "POST");
                break;
            case "previous":
                done = changedTrack;
                await call(await targeted("/previous"), "POST");
                break;
            case "play":
                done = isPlaying(true);
                await resume();
                break;
            case "pause":
                done = isPlaying(false);
                await call("/pause", "PUT");
                break;
            default:
                log("warn", `unknown action ${name}`);
                return;
        }

        log("info", `action ${name}`);
        await publish(await settled(done));
    } catch (e) {
        log("error", `action ${name} failed: ${e}`);

        try {
            await publish(await call("/currently-playing"), true);
        } catch (inner) {
            log("error", `could not re-read after a failed ${name}: ${inner}`);
        }
    }
}
