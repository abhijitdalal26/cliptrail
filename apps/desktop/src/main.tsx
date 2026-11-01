import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import "@fontsource-variable/inter";
import "@fontsource-variable/jetbrains-mono";
import "./index.css";
import { initTheme } from "./lib/theme";
import { Dashboard } from "./dashboard/Dashboard";
import { Picker } from "./picker/Picker";

initTheme();

const isPicker = location.hash.startsWith("#/picker");
document.documentElement.classList.toggle("picker", isPicker);

// No browser context menu in the desktop app (text fields keep theirs).
window.addEventListener("contextmenu", (e) => {
  const t = e.target as HTMLElement;
  if (!t.closest("input, textarea, .selectable")) e.preventDefault();
});

createRoot(document.getElementById("root")!).render(
  <StrictMode>{isPicker ? <Picker /> : <Dashboard />}</StrictMode>,
);
