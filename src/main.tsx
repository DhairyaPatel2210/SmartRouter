import React from "react";
import ReactDOM from "react-dom/client";
import App from "./App";
import { boot, useApp } from "./lib/store";
import { ErrorBoundary, reportError } from "./components/ErrorBoundary";
import "./styles.css";

// Apply the theme before first paint to avoid a flash.
if (window.matchMedia("(prefers-color-scheme: dark)").matches) document.documentElement.classList.add("dark");

ReactDOM.createRoot(document.getElementById("root")!).render(
  <React.StrictMode>
    <ErrorBoundary>
      <App />
    </ErrorBoundary>
  </React.StrictMode>,
);

// Uncaught errors go to the core log so they show in the terminal and app.log.
window.addEventListener("error", (e) => reportError(`${e.message} (${e.filename}:${e.lineno})`));
window.addEventListener("unhandledrejection", (e) => reportError(`unhandled: ${String(e.reason)}`, "warn"));

boot().catch((e) => {
  reportError(`boot failed: ${String(e)}`);
  useApp.setState({ bootError: String(e), ready: true });
});
