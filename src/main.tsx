import React from "react";
import ReactDOM from "react-dom/client";
import App from "./App";
import { initializeThemeSync } from "./theme";
import { initializeDiagnostics } from "./diagnostics";

initializeThemeSync();
initializeDiagnostics("main");

if (import.meta.env.MODE === "e2e") void import("@wdio/tauri-plugin");

ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
  <React.StrictMode>
    <App />
  </React.StrictMode>,
);
