import { rmSync } from "node:fs";
import { resolve } from "node:path";

const executable = process.platform === "win32" ? "myfocus.exe" : "myfocus";
const appBinaryPath = resolve("src-tauri", "target", "debug", executable);
const dataDir = resolve("e2e", ".data");

export const config: WebdriverIO.Config = {
  runner: "local",
  specs: ["./e2e/specs/**/*.spec.ts"],
  maxInstances: 1,
  services: [
    [
      "tauri",
      {
        appBinaryPath,
        driverProvider: "embedded",
        embeddedPort: 4445,
        windowLabel: "main",
        startTimeout: 60_000,
        captureBackendLogs: true,
        captureFrontendLogs: true,
        env: {
          MYFOCUS_E2E: "1",
          MYFOCUS_DATA_DIR: dataDir,
        },
      },
    ],
  ],
  capabilities: [
    {
      browserName: "tauri",
      "tauri:options": {
        application: appBinaryPath,
      },
    },
  ],
  logLevel: "warn",
  bail: 0,
  waitforTimeout: 10_000,
  connectionRetryTimeout: 90_000,
  connectionRetryCount: 2,
  framework: "mocha",
  reporters: ["spec"],
  mochaOpts: {
    ui: "bdd",
    timeout: 60_000,
  },
  onPrepare: () => {
    rmSync(dataDir, { recursive: true, force: true });
  },
};
