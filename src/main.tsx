import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import "./index.css";
import "./lib/i18n";
import { App } from "./App";
import { TooltipProvider } from "@/components/ui/tooltip";

if (import.meta.env.VITE_WDIO === "true") {
  void import("@wdio/tauri-plugin");
}

createRoot(document.getElementById("root")!).render(<StrictMode><TooltipProvider><App /></TooltipProvider></StrictMode>);
