import React from "react";
import ReactDOM from "react-dom/client";
import App from "./App";
import { SettingsWindow } from "./components/SettingsWindow";

declare global {
  interface Window {
    __MYFOCUS_WINDOW__?: string;
  }
}

const isSettingsWindow = window.__MYFOCUS_WINDOW__ === "settings";

ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
  <React.StrictMode>
    {isSettingsWindow ? <SettingsWindow /> : <App />}
  </React.StrictMode>,
);
