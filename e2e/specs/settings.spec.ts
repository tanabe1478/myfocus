describe("settings window", () => {
  it("opens, saves, closes, and reopens with the saved value", async () => {
    await browser.pause(500);

    const openButton = await $('[data-testid="open-settings"]');
    await openButton.waitForDisplayed();
    await openButton.click();

    const handles = await browser.getWindowHandles();
    expect(handles.length).toBeGreaterThanOrEqual(2);

    const mainHandle = await browser.getWindowHandle();
    let settingsHandle: string | undefined;
    for (const handle of handles) {
      await browser.switchToWindow(handle);
      if ((await browser.getTitle()).includes("設定")) {
        settingsHandle = handle;
        break;
      }
    }
    expect(settingsHandle).toBeDefined();

    const settings = await $('[data-testid="settings-window"]');
    await settings.waitForDisplayed();

    const retention = await $('[data-testid="retention-days"]');
    await retention.setValue("91");
    await $('[data-testid="save-settings"]').click();
    await expect($('[data-testid="settings-note"]')).toHaveText("保存しました");

    await $('[data-testid="close-settings"]').click();
    await browser.waitUntil(
      async () =>
        !(await browser.tauri.execute(({ core }) =>
          core.invoke<boolean>("is_settings_visible")
        )),
      { timeoutMsg: "settings window did not become hidden" }
    );

    await browser.switchToWindow(mainHandle);
    await $('[data-testid="open-settings"]').click();
    await browser.switchToWindow(settingsHandle!);
    await settings.waitForDisplayed();
    await expect(retention).toHaveValue("91");

    // Exercise the same native close request emitted by the title-bar ×.
    await browser.tauri.execute(({ core }) =>
      core.invoke("request_settings_native_close")
    );
    await browser.waitUntil(
      async () =>
        !(await browser.tauri.execute(({ core }) =>
          core.invoke<boolean>("is_settings_visible")
        )),
      { timeoutMsg: "native close did not hide the settings window" }
    );

    // The close request must hide, not destroy, the reusable window.
    await browser.switchToWindow(mainHandle);
    await $('[data-testid="open-settings"]').click();
    await browser.switchToWindow(settingsHandle!);
    await settings.waitForDisplayed();
    await expect(retention).toHaveValue("91");
  });

  it("exposes both native window labels", async () => {
    await expect(browser.tauri.listWindows()).resolves.toEqual(
      expect.arrayContaining(["main", "settings"])
    );
  });
});
