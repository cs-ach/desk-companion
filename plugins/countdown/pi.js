const DAY_MS = 86_400_000;

function daysLeft() {
  const target = Date.parse(getConfig("target"));
  return Math.ceil((target - Date.now()) / DAY_MS);
}

register("main", () => {
  const view = { label: getConfig("label"), days: daysLeft() };
  renderPage(view, false);
  renderWidget("days_left", view, false);
}, 3600);
