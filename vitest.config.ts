// 前端单测配置（T3）：独立于 vite.config.ts——构建配置不动它，避免连带踩坑。
// node 环境为主；被测的都是纯逻辑（工具函数/状态机），浏览器 API 一律 mock。

import { defineConfig } from "vitest/config";

export default defineConfig({
  test: {
    environment: "node",
    include: ["src/**/*.test.ts", "src/**/*.test.tsx"],
  },
});
