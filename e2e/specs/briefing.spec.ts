describe("daily briefing", () => {
  it("opens a cached recommendation directly from the sidebar", async () => {
    const briefing = await $('[data-testid="open-briefing"]');
    await briefing.waitForDisplayed();
    // Seed in Rust so the WebKit embedded driver does not need to serialize a
    // nested payload. WebKit may expose the page before core.invoke is ready.
    await browser.waitUntil(
      async () => {
        try {
          await browser.tauri.execute(({ core }) => core.invoke("seed_e2e_data"));
          return true;
        } catch {
          return false;
        }
      },
      { timeout: 15000, timeoutMsg: "Tauri invoke bridge did not become ready" }
    );
    await briefing.click();

    await expect($('.ai-message strong')).toHaveText("Daily picks");
    await expect($('.article-suggestion-title')).toHaveText("Cached recommendation");
    await expect($('.ai-cache-badge')).toHaveText("保存済みブリーフィング");
  });
});
