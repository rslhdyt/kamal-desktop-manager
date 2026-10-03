import { createKumoToastManager } from "@cloudflare/kumo";

// Shared with <Toasty> so non-React code (run output handlers) can show toasts.
export const toasts = createKumoToastManager();
