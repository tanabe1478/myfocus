describe("feed categories", () => {
  before(async () => {
    await $('[data-testid="all-articles"]').waitForDisplayed();
    await browser.tauri.execute(({ core }) => core.invoke("seed_e2e_data"));
    await $('[data-testid="feed-101"]').waitForDisplayed();
  });

  it("moves a feed, renames its category, and returns it to uncategorized", async () => {
    await browser.execute(() => {
      const handle = document.querySelector<HTMLElement>('[data-testid="feed-101-drag"]')!;
      const target = document.querySelector<HTMLElement>('[data-testid="category-Tech"]')!;
      const box = target.getBoundingClientRect();
      handle.dispatchEvent(new MouseEvent("mousedown", { bubbles: true, button: 0 }));
      target.dispatchEvent(
        new MouseEvent("mousemove", {
          bubbles: true,
          buttons: 1,
          clientX: box.left + box.width / 2,
          clientY: box.top + box.height / 2,
        })
      );
      target.dispatchEvent(
        new MouseEvent("mouseup", {
          bubbles: true,
          button: 0,
          clientX: box.left + box.width / 2,
          clientY: box.top + box.height / 2,
        })
      );
    });
    await browser.waitUntil(async () => {
      const feeds = await browser.tauri.execute(({ core }) => core.invoke<Array<{ id: number; category: string | null }>>("list_feeds"));
      return feeds.find((feed) => feed.id === 101)?.category === "Tech";
    });

    await browser.execute(() => {
      window.prompt = () => "Engineering";
    });
    await $('[data-testid="category-Tech"]').moveTo();
    await $('[data-testid="rename-category-Tech"]').click();
    await $('[data-testid="category-Engineering"]').waitForDisplayed();

    await browser.execute(() => {
      window.prompt = () => "";
    });
    await $('[data-testid="feed-101"]').moveTo();
    await $('[data-testid="feed-101-category"]').click();
    await browser.waitUntil(async () => {
      const feeds = await browser.tauri.execute(({ core }) => core.invoke<Array<{ id: number; category: string | null }>>("list_feeds"));
      return feeds.find((feed) => feed.id === 101)?.category == null;
    });
  });
});
