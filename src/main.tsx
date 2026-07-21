import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import "./index.css";
import { App } from "./App";

if (import.meta.env.VITE_WDIO === "true") {
  void import("@wdio/tauri-plugin");
}

createRoot(document.getElementById("root")!).render(<StrictMode><App /></StrictMode>);
