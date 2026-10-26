import React from "react";
import ReactDOM from "react-dom/client";
import App from "./App";
import Tray from "./Tray";
import "@fontsource-variable/plus-jakarta-sans";
import "./styles.css";

const isTray = location.hash === "#tray";
if (isTray) document.body.classList.add("tray-body");

if (import.meta.env.DEV && !("__TAURI_INTERNALS__" in window)) await import("./dev/mock");

ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
  <React.StrictMode>{isTray ? <Tray /> : <App />}</React.StrictMode>,
);
