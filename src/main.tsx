import React from "react";
import ReactDOM from "react-dom/client";
import App from "./App";
import { boot, useApp } from "./lib/store";
import "./styles.css";

// Apply the theme before first paint to avoid a flash.
if (window.matchMedia("(prefers-color-scheme: dark)").matches) document.documentElement.classList.add("dark");

ReactDOM.createRoot(document.getElementById("root")!).render(
  <React.StrictMode>
    <App />
  </React.StrictMode>,
);

boot().catch((e) => useApp.setState({ bootError: String(e), ready: true }));
