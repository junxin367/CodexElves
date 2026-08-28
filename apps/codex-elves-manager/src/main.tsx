import { createRoot } from "react-dom/client";
import { App } from "./App";
import { TaskBoardApp } from "./TaskBoardApp";

/* ── Bundled fonts (offline, no Google Fonts request) ──
     Fontsource packages ship woff2 files that Vite bundles into dist/.
     CSS @font-face declarations are injected at build time.              */
import "@fontsource/inter";
import "@fontsource/jetbrains-mono";

const app = document.getElementById("app");

if (app instanceof HTMLElement) {
  const params = new URLSearchParams(window.location.search);
  const openInDebugMode = params.get("debug") === "open-in";
  const taskBoardMode = params.get("taskBoard") === "1";
  if (openInDebugMode) {
    void Promise.all([import("./open-in-debug.css"), import("./OpenInDebugApp")]).then(
      ([, { OpenInDebugApp }]) => {
        createRoot(app).render(<OpenInDebugApp />);
      },
    );
  } else if (taskBoardMode) {
    void import("./task-board-standalone.css").then(() => {
      createRoot(app).render(<TaskBoardApp />);
    });
  } else {
    void import("./styles.css").then(() => {
      createRoot(app).render(<App />);
    });
  }
}
