const modifier = process.platform === "darwin" ? "Meta" : "Control";

async function press(...keys: string[]) {
  await browser.keys(keys);
}

describe("main window keyboard and search flows", () => {
  before(async () => {
    await browser.tauri.execute(({ core }) => core.invoke("seed_e2e_data"));
    await $('[data-testid="article-1001"]').waitForDisplayed();
  });

  it("navigates articles with j and k", async () => {
    await press("j");
    await expect($('[data-testid="article-1001"]')).toHaveElementClass(
      expect.stringContaining("selected")
    );

    await press("j");
    await expect($('[data-testid="article-1002"]')).toHaveElementClass(
      expect.stringContaining("selected")
    );

    await press("k");
    await expect($('[data-testid="article-1001"]')).toHaveElementClass(
      expect.stringContaining("selected")
    );
  });

  it("navigates feeds and categories with Shift+J/K", async () => {
    await press("Shift", "j");
    await expect($('[data-testid="feed-101"]')).toHaveElementClass(
      expect.stringContaining("selected")
    );

    await press("Shift", "j");
    await expect($('[data-testid="category-Tech"]')).toHaveElementClass(
      expect.stringContaining("selected")
    );

    await press("Shift", "k");
    await expect($('[data-testid="feed-101"]')).toHaveElementClass(
      expect.stringContaining("selected")
    );
  });

  it("marks the current feed read with Shift+A", async () => {
    // Embedded WebDriver waits for the key action to finish, while a native
    // confirm dialog blocks that action. Auto-confirm in page context here.
    await browser.execute(() => {
      window.confirm = () => true;
    });
    await press("Shift", "a");
    await browser.waitUntil(async () => {
      const articles = await browser.tauri.execute(({ core }) =>
        core.invoke<Array<{ read: boolean }>>("list_articles", {
          feedId: 101,
          category: null,
          unreadOnly: false,
          starredOnly: false,
        })
      );
      return articles.length === 2 && articles.every((article) => article.read);
    });
  });

  it("opens search, saves a query, and reuses it from the sidebar", async () => {
    await press(modifier, "k");
    const input = await $('[data-testid="search-input"]');
    await input.waitForDisplayed();
    await input.setValue("searchable");
    await $('[data-testid="search-result-1001"]').waitForDisplayed();
    await $('[data-testid="save-search"]').click();
    await expect($('[data-testid="save-search"]')).toHaveText("保存済み");
    await input.click();
    await press("Escape");
    await expect($('[data-testid="search-overlay"]')).not.toBeDisplayed();

    const saved = await $('.saved-search-list .sidebar-row');
    await saved.waitForDisplayed();
    expect(await saved.getText()).toContain("searchable");
    await saved.click();
    await expect($('.article-list .pane-title')).toHaveText("検索: searchable");
  });

  it("opens and closes the AI panel", async () => {
    const panel = await $('[data-testid="ai-panel"]');
    if (await panel.isExisting()) {
      await $('[data-testid="ai-panel"] button[title="閉じる"]').click();
    }
    const open = await $('[data-testid="open-ai"]');
    await open.waitForDisplayed();
    await open.click();
    await expect($('[data-testid="ai-panel"]')).toBeDisplayed();
    await $('[data-testid="ai-panel"] button[title="閉じる"]').click();
    await expect($('[data-testid="ai-panel"]')).not.toExist();
  });
});
