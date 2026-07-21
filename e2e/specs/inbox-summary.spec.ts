describe("AI summary inbox", () => {
  before(async () => {
    let mainFound = false;
    for (const handle of await browser.getWindowHandles()) {
      await browser.switchToWindow(handle);
      if ((await browser.getTitle()) === "myfocus") {
        mainFound = true;
        break;
      }
    }
    expect(mainFound).toBe(true);
    await browser.tauri.execute(({ core }) => core.invoke("seed_e2e_data"));
    await $('[data-testid="summary-inbox"]').waitForDisplayed();
  });

  it("shows persisted summary jobs and clears the new badge when reviewed", async () => {
    const inbox = await $('[data-testid="summary-inbox"]');
    expect(await inbox.getText()).toContain("1件生成中");
    expect(await inbox.getText()).toContain("1");
    await inbox.click();

    await $('[data-testid="article-1001"]').waitForDisplayed();
    await expect($('[data-testid="article-1001"] .summary-job-status')).toHaveText("新着");
    await expect($('[data-testid="article-1002"] .summary-job-status')).toHaveText("生成中");
    await expect($('[data-testid="article-1003"] .summary-job-status')).toHaveText("失敗");

    await $('[data-testid="article-1001"]').click();
    await expect($('.article-ai-summary-text')).toHaveText("Completed E2E summary");
    await browser.waitUntil(async () => !(await inbox.getText()).endsWith("1"), {
      timeoutMsg: "summary inbox badge did not clear after review",
    });
  });
});
