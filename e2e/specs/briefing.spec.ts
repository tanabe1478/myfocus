describe("daily briefing", () => {
  it("opens a cached recommendation directly from the sidebar", async () => {
    await browser.tauri.execute(({ core }) =>
      core.invoke("set_setting", {
        key: "ai_recommendation_cache",
        value: JSON.stringify({
          createdAt: Date.now(),
          text: "ARTICLE: 999 | Cached recommendation\nA reason to read it.",
        }),
      })
    );

    const briefing = await $('[data-testid="open-briefing"]');
    await briefing.waitForDisplayed();
    await briefing.click();

    await expect($('.article-suggestion-title')).toHaveText("Cached recommendation");
    await expect($('.ai-cache-badge')).toHaveText("保存済みブリーフィング");
  });
});
