import { defineConfig } from "vite";
import { resolve } from "path";

// Tauri 需要固定端口，且端口被占用时不能自动切换
export default defineConfig({
  clearScreen: false,
  server: {
    port: 1420,
    strictPort: true,
    watch: {
      ignored: ["**/src-tauri/**"],
    },
  },
  build: {
    target: "chrome105",
    minify: "esbuild",
    sourcemap: false,
    rollupOptions: {
      input: {
        // 主窗口
        main: resolve(__dirname, "index.html"),
        // 全屏框选遮罩
        overlay: resolve(__dirname, "overlay.html"),
      },
    },
  },
});
