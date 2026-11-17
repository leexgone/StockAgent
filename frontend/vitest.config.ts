import { defineConfig } from "vitest/config";

// 与 vite.config.ts 分开：应用构建配置保持纯净，测试配置独立演进。
// 黄金向量测试（src/domain/golden.test.ts，P3-T4）是纯计算 + node:fs 读
// 仓库根 fixtures/golden/，用 node 环境即可，无需 jsdom / react 插件。
export default defineConfig({
  test: {
    environment: "node",
    include: ["src/**/*.test.ts"],
    // vmThreads（2026-11-17）：vitest 5.1.0 缺省 forks/threads 池在本机 Windows
    // 项目路径下模块加载异常（worker 状态在而 suite collector undefined，
    // describe 抛 reading 'config'）；vmThreads 合成模块链接绕过，156 例全绿。
    // 上游 tarball 当日有重发布噪声（5.0.5 构建漂移），5.1.0 + vmThreads 为实证稳定组合。
    pool: "vmThreads",
  },
});
