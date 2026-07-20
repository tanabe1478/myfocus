import React from "react";
import ReactDOM from "react-dom/client";
import { SettingsWindow } from "./components/SettingsWindow";
import { initializeThemeSync } from "./theme";

initializeThemeSync();

if (import.meta.env.MODE === "e2e") void import("@wdio/tauri-plugin");

ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
  <React.StrictMode>
    <SettingsWindow />
  </React.StrictMode>,
);
