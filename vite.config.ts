import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";
import { resolve } from "node:path";

// Tauri 需要固定端口，且不能在端口被占用时切换
export default defineConfig({
  plugins: [react()],
  clearScreen: false,
  server: {
    port: 1420,
    strictPort: true,
    host: "127.0.0.1",
    watch: {
      // 监听 src-tauri 会触发无意义的重启
      ignored: ["**/src-tauri/**"],
    },
  },
  build: {
    target: "chrome110",
    minify: "esbuild",
    sourcemap: false,
    rollupOptions: {
      // 多入口：主窗口 + 便签窗口
      input: {
        main: resolve(__dirname, "index.html"),
        note: resolve(__dirname, "note.html"),
      },
      output: {
        manualChunks: undefined,
      },
    },
  },
});

