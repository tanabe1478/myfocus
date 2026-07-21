describe("daily briefing", () => {
  it("opens a cached recommendation directly from the sidebar", async () => {
    // Seed in Rust so the WebKit embedded driver does not need to serialize
    // a nested multiline payload through direct JavaScript evaluation.
    await browser.tauri.execute(({ core }) => core.invoke("seed_e2e_data"));

    const briefing = await $('[data-testid="open-briefing"]');
    await briefing.waitForDisplayed();
    await briefing.click();

    await expect($('.ai-message strong')).toHaveText("Daily picks");
    await expect($('.article-suggestion-title')).toHaveText("Cached recommendation");
    await expect($('.ai-cache-badge')).toHaveText("保存済みブリーフィング");
  });
});
