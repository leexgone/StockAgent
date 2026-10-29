import createClient from "openapi-fetch";
import type { paths } from "./schema";

/**
 * REST 唯一入口（P5-T1，ADR-0008）：openapi-fetch + `npm run gen:api` 生成的类型面
 * （schema.d.ts），path / params / 响应全部由 docs/api/openapi.json 约束——
 * 契约变更后先再生成 openapi.json，再 gen:api，类型漂移由 tsc 暴露。
 *
 * 约定：
 * - 全仓不手拼 URL 字符串、不裸 fetch；唯一例外是 SSE 会话 /api/chat*
 *   （流式读取不在 openapi-fetch 覆盖面内，ChatPanel 保持原生 fetch + ReadableStream）。
 * - baseUrl 缺省 = 同源相对路径：开发态 vite 把 /api 反代到 8080，
 *   生产态 zimu-server 单端口直接托管 dist（SPA 回落已就位，深链可刷新）。
 * - 调用返回 [data, error] 元组：error 分支必须处理（降级/提示），不许裸 `!`。
 */
export const api = createClient<paths>();

/**
 * 从 openapi-fetch 的 error 响应提取可展示消息：后端错误体统一是 `ApiError`
 * （机器码 error + 中文 message），message 可直接给用户看；非 ApiError
 * （网络层失败等）→ fallback。store 的 error 字段一律经这里收敛。
 */
export function apiErrorMessage(err: unknown, fallback: string): string {
  if (err && typeof err === "object" && "message" in err) {
    const m = (err as { message?: unknown }).message;
    if (typeof m === "string" && m.length > 0) return m;
  }
  return fallback;
}
