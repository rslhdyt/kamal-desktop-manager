import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";
import tailwindcss from "@tailwindcss/vite";
// @ts-expect-error type error without @types/node package
import process from "node:process";
const host = process.env.TAURI_DEV_HOST;

// https://vite.dev/config/
export default defineConfig(() => ({
  plugins: [react(), tailwindcss()],

  // Vite options tailored for Tauri development and only applied in `tauri dev` or `tauri build`
  //
  // 1. prevent Vite from obscuring rust errors
  clearScreen: false,
  // 2. tauri expects a fixed port, fail if that port is not available
  server: {
    port: 1420,
    strictPort: true,
    host: host || false,
    hmr: host
      ? {
          protocol: "ws",
          host,
          port: 1421,
        }
      : undefined,
    watch: {
      // 3. tell Vite to ignore watching `src-tauri`
      ignored: ["**/src-tauri/**"],
    },
  },
  build: {
    rolldownOptions: {
      output: {
        // Split vendors so no chunk exceeds Vite's 500 kB warning. echarts/zrender stay lazy (UsageChart).
        codeSplitting: {
          groups: [
            { name: "react", test: /node_modules[\\/](react|react-dom|scheduler)[\\/]/ },
            { name: "kumo", test: /node_modules[\\/]@cloudflare[\\/]kumo[\\/]/ },
            { name: "xterm", test: /node_modules[\\/]@xterm[\\/]/ },
            { name: "echarts", test: /node_modules[\\/]echarts[\\/]/ },
            { name: "zrender", test: /node_modules[\\/]zrender[\\/]/, priority: 1 },
          ],
        },
      },
    },
  },
}));
