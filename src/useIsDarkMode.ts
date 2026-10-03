import { useSyncExternalStore } from "react";

// main.tsx sets Kumo's data-mode from the OS appearance, so follow the same media query.
const dark = window.matchMedia("(prefers-color-scheme: dark)");
const subscribe = (cb: () => void) => {
  dark.addEventListener("change", cb);
  return () => dark.removeEventListener("change", cb);
};
export const useIsDarkMode = () => useSyncExternalStore(subscribe, () => dark.matches);
