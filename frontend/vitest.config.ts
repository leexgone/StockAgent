import { defineConfig } from "vitest/config";

// 与 vite.config.ts 分开：应用构建配置保持纯净，测试配置独立演进。
// 黄金向量测试（src/domain/golden.test.ts，P3-T4）是纯计算 + node:fs 读
// 仓库根 fixtures/golden/，用 node 环境即可，无需 jsdom / react 插件。
export default defineConfig({
  test: {
    environment: "node",
    include: ["src/**/*.test.ts"],
  },
});
