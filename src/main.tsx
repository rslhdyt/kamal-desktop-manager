import React from "react";
import ReactDOM from "react-dom/client";
import { Toasty } from "@cloudflare/kumo";
import App from "./App";
import { toasts } from "./toasts";

// Kumo switches theme via data-mode; follow the OS appearance.
const dark = window.matchMedia("(prefers-color-scheme: dark)");
const applyMode = () => (document.documentElement.dataset.mode = dark.matches ? "dark" : "light");
applyMode();
dark.addEventListener("change", applyMode);

ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
  <React.StrictMode>
    <Toasty toastManager={toasts}>
      <App />
    </Toasty>
  </React.StrictMode>,
);
