const GET_URL = "https://httpbin.org/get?source=http-demo";
const POST_URL = "https://httpbin.org/post";
const NOT_FOUND_URL = "https://httpbin.org/status/404";

async function demoGet() {
    const res = await fetch(GET_URL);
    const json = JSON.parse(res.body);
    log("info", `GET ${res.status}: args=${JSON.stringify(json.args)}`);
}

async function demoPost() {
    const payload = { message: "hello from http-demo", sentAt: Date.now() };
    const res = await fetch(POST_URL, {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify(payload),
    });
    const json = JSON.parse(res.body);
    log("info", `POST ${res.status}: server echoed ${JSON.stringify(json.json)}`);
}

async function demoNotFound() {
    // HTTP error statuses resolve with ok: false instead of rejecting
    const res = await fetch(NOT_FOUND_URL);
    log("info", `404 check: status=${res.status} ok=${res.ok}`);
}

async function run(label, fn) {
    try {
        await fn();
    } catch (e) {
        log("error", `${label} failed: ${e}`);
    }
}

async function main() {
    log("info", "http-demo starting");
    await run("GET", demoGet);
    await run("POST", demoPost);
    await run("404", demoNotFound);
    await run("bad method", () => fetch(GET_URL, { method: "PSOT" }));
    log("info", "http-demo done");
}
