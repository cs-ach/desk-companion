// Spotify -- authorization.
//
// Runs once, when someone taps Connect on the device. Only this entry is given
// authorize(), setSecret(), randomString() and sha256Base64Url(); the
// long-running host.js has none of them and so cannot open a browser window or
// rewrite the token it runs on.
//
// Authorization Code with PKCE, so there is no client secret anywhere: the
// verifier proves we are the same client that started the flow. A desktop app
// cannot keep a secret, so this is the flow Spotify and RFC 8252 both point
// native apps at.

const AUTH_URL = "https://accounts.spotify.com/authorize";
const TOKEN_URL = "https://accounts.spotify.com/api/token";
const REDIRECT = "http://127.0.0.1:8888/callback";
const SCOPES = "user-read-playback-state user-modify-playback-state";

async function login() {
    if (!config.clientId) {
        throw new Error("clientId is not set -- add it in config.json first");
    }

    // The verifier never leaves this process until the exchange; only its
    // hash travels in the URL the browser opens.
    const verifier = randomString(64);
    const challenge = sha256Base64Url(verifier);
    const state = randomString(43);

    const url = AUTH_URL
        + "?response_type=code"
        + `&client_id=${encodeURIComponent(config.clientId)}`
        + `&redirect_uri=${encodeURIComponent(REDIRECT)}`
        + `&scope=${encodeURIComponent(SCOPES)}`
        + `&code_challenge_method=S256`
        + `&code_challenge=${encodeURIComponent(challenge)}`
        + `&state=${encodeURIComponent(state)}`;

    log("info", "waiting for the browser");
    const code = await authorize(url);

    log("info", "exchanging the code");
    const res = await fetch(TOKEN_URL, {
        method: "POST",
        headers: { "Content-Type": "application/x-www-form-urlencoded" },
        body: [
            "grant_type=authorization_code",
            `code=${encodeURIComponent(code)}`,
            `redirect_uri=${encodeURIComponent(REDIRECT)}`,
            `client_id=${encodeURIComponent(config.clientId)}`,
            `code_verifier=${encodeURIComponent(verifier)}`,
        ].join("&"),
    });

    if (!res.ok) {
        throw new Error(`token exchange returned ${res.status}: ${res.body}`);
    }

    const parsed = JSON.parse(res.body);
    if (!parsed.refresh_token) {
        throw new Error(`no refresh_token in the response: ${res.body}`);
    }

    await setSecret("refreshToken", parsed.refresh_token);
    log("info", "saved refreshToken -- Spotify is connected");
}
