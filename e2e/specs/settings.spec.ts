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
    const theme = await $('[data-testid="theme-select"]');
    await browser.execute(() => {
      const select = document.querySelector<HTMLSelectElement>('[data-testid="theme-select"]')!;
      select.value = "warm-dark";
      select.dispatchEvent(new Event("change", { bubbles: true }));
    });
    await expect(theme).toHaveValue("warm-dark");
    await $('[data-testid="save-settings"]').click();
    await expect($('[data-testid="settings-note"]')).toHaveText("保存しました");
    await expect(
      browser.tauri.execute(({ core }) => core.invoke<string>("get_setting", { key: "theme_id" }))
    ).resolves.toBe("warm-dark");

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
    await expect(theme).toHaveValue("warm-dark");
    await expect(browser.execute(() => document.documentElement.dataset.theme)).resolves.toBe(
      "warm-dark"
    );

    await browser.switchToWindow(mainHandle);
    await expect(browser.execute(() => document.documentElement.dataset.theme)).resolves.toBe(
      "warm-dark"
    );
    await browser.switchToWindow(settingsHandle!);

    await $('button=複製して編集').click();
    const themeName = await $('[data-testid="theme-name"]');
    await themeName.waitForDisplayed();
    await themeName.setValue("E2E custom theme");
    await $('[data-testid="save-custom-theme"]').click();
    await expect($('[data-testid="settings-note"]')).toHaveText(
      "カスタムテーマを保存しました"
    );
    expect(await theme.getValue()).toMatch(/^user:/);
    await expect(
      browser.tauri.execute(({ core }) =>
        core.invoke<string>("get_setting", { key: "custom_themes" })
      )
    ).resolves.toContain("E2E custom theme");
    await browser.switchToWindow(mainHandle);
    await expect(browser.execute(() => document.documentElement.dataset.theme)).resolves.toMatch(
      /^user:/
    );
    await browser.switchToWindow(settingsHandle!);

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

  it("toggles diagnostic logging and writes a local record", async () => {
    const toggle = await $('[data-testid="diagnostic-logging-toggle"]');
    await toggle.waitForDisplayed();
    if (await toggle.isSelected()) await toggle.click();
    await toggle.click();
    await expect(toggle).toBeSelected();
    await expect(
      browser.tauri.execute(({ core }) =>
        core.invoke<string>("get_setting", { key: "diagnostic_logging_enabled" })
      )
    ).resolves.toBe("true");
    await browser.tauri.execute(({ core }) =>
      core.invoke("diagnostic_log", {
        level: "info",
        event: "e2e_diagnostic_record",
        details: { source: "settings_spec" },
      })
    );
    const info = await browser.tauri.execute(({ core }) =>
      core.invoke<{ sizeBytes: number }>("get_diagnostic_info")
    );
    expect(info.sizeBytes).toBeGreaterThan(0);
    await toggle.click();
    await expect(toggle).not.toBeSelected();
  });

  it("exposes both native window labels", async () => {
    await expect(browser.tauri.listWindows()).resolves.toEqual(
      expect.arrayContaining(["main", "settings"])
    );
  });
});
