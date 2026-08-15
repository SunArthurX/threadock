import { defineConfig } from "vitest/config";
import react from "@vitejs/plugin-react";

export default defineConfig({
  plugins: [react()],
  server: { host: "127.0.0.1", allowedHosts: true },
  test: {
    environment: "jsdom",
    globals: true,
    include: ["src/**/*.test.{ts,tsx}"],
    setupFiles: ["src/__tests__/setup.ts"],
    host: false, // 关闭 vitest 内置 dev server 的 hostname 校验（沙箱环境会触发 DNS 解析）
  },
});
