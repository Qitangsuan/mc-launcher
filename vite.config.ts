import { defineConfig } from "vite";
import vue from "@vitejs/plugin-vue";

// Tauri 要求固定端口，且不能清屏，否则前端日志会盖住 Rust 的报错
export default defineConfig({
  plugins: [vue()],
  clearScreen: false,
  server: {
    host: "127.0.0.1",
    port: 1420,
    strictPort: true,
    watch: {
      // src-tauri 由 cargo 管；临时写入的 .tmpdir / *.tmp 被锁时会让 watcher 崩（EBUSY）
      ignored: ["**/src-tauri/**", "**/.*.tmpdir/**", "**/*.tmp"],
    },
  },
});
