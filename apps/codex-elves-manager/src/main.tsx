import { createRoot } from "react-dom/client";
import { App } from "./App";
import { TaskBoardApp } from "./TaskBoardApp";
import "./styles.css";

/* ── Bundled fonts (offline, no Google Fonts request) ──
     Fontsource packages ship woff2 files that Vite bundles into dist/.
     CSS @font-face declarations are injected at build time.              */
import "@fontsource/inter";
import "@fontsource/jetbrains-mono";

const app = document.getElementById("app");

if (app instanceof HTMLElement) {
  const taskBoardMode = new URLSearchParams(window.location.search).get("taskBoard") === "1";
  createRoot(app).render(taskBoardMode ? <TaskBoardApp /> : <App />);
}
