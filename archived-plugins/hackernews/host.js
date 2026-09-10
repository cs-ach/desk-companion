const API = "https://hacker-news.firebaseio.com/v0";

register("main", async () => {
  const count = getConfig("count") ?? 10;

  try {
    const ids = await httpGet(`${API}/topstories.json`);
    const stories = await Promise.all(
      ids.slice(0, count).map((id) => httpGet(`${API}/item/${id}.json`)),
    );

    sendState({
      stories: stories.filter(Boolean).map((s) => ({
        title: s.title ?? "",
        score: s.score ?? 0,
        by: s.by ?? "",
        comments: s.descendants ?? 0,
      })),
      error: null,
    });
  } catch (e) {
    sendState({ error: String(e) });
  }
}, 300);
