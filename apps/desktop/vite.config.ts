import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";

export default defineConfig({
  plugins: [react()],
  clearScreen: false,
  server: {
    host: "127.0.0.1",
    port: 1420,
    strictPort: true,
    // fs.allow 放开到仓库根：SettingsView 构建期 `?raw` 导入工作区 Cargo.toml
    // 派生版本号（vite 6 起根外文件默认 Denied，开发服务器需显式放行）
    fs: { allow: ["../.."] },
  },
  envPrefix: ["VITE_", "TAURI_"],
  build: {
    target: "esnext",
    minify: "esbuild",
    sourcemap: false,
  },
});
