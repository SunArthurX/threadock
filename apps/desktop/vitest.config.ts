import { defineConfig } from "vitest/config";
import react from "@vitejs/plugin-react";

export default defineConfig({
  plugins: [react()],
  // fs.allow 放开到仓库根：SettingsView / round5 测试 `?raw` 导入工作区 Cargo.toml
  // 派生版本号（vite 6 起根外文件默认 Denied ID，vite 5 的行为被收紧）
  server: { host: "127.0.0.1", allowedHosts: true, fs: { allow: ["../.."] } },
  test: {
    environment: "jsdom",
    globals: true,
    include: ["src/**/*.test.{ts,tsx}"],
    setupFiles: ["src/__tests__/setup.ts"],
    host: false, // 关闭 vitest 内置 dev server 的 hostname 校验（沙箱环境会触发 DNS 解析）
  },
});
