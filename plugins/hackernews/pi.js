register("state", (state) => {
  const view = {
    stories: state.stories ?? [],
    error: state.error ?? "",
  };

  renderPage(view);
  renderWidget("top_story", {
    title: view.stories[0]?.title ?? "",
    score: view.stories[0]?.score ?? 0,
    error: view.error,
  });
});
