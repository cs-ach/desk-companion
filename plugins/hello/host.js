async function init() { log("info", "init ok"); }
async function tick() {
    n += 1;
    log("info", `tick ${n}`);
    if (n >= 3) throw new Error("tick blew up");
}