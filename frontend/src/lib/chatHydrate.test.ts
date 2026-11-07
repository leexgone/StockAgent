import { describe, expect, it } from "vitest";
import { hydrateEntries, type ChatMessageRow } from "./chatHydrate";

function row(over: Partial<ChatMessageRow>): ChatMessageRow {
  return { id: 1, session: "web", role: "user", text: "", tool_json: null, ts: "2026-11-07 10:00", ...over };
}

describe("hydrateEntries（服务端档案 → 展示条目，T4）", () => {
  it("user/assistant 原样映射，assistant 一律视为完成态", () => {
    const out = hydrateEntries([
      row({ id: 1, role: "user", text: "茅台什么价" }),
      row({ id: 2, role: "assistant", text: "结论：1437.00" }),
    ]);
    expect(out).toEqual([
      { kind: "user", text: "茅台什么价" },
      { kind: "assistant", text: "结论：1437.00", done: true },
    ]);
  });

  it("tool 行解析 recorder 落库形态（arguments/result 为 JSON 串原文）", () => {
    const snap = JSON.stringify({
      name: "get_quote",
      arguments: '{"codes":["sh600519"]}',
      result: '{"sh600519":{"price":1437.0}}',
      is_error: false,
    });
    const out = hydrateEntries([row({ id: 7, role: "tool", tool_json: snap })]);
    expect(out).toEqual([
      {
        kind: "tool",
        id: "hist-7",
        name: "get_quote",
        args: '{"codes":["sh600519"]}',
        result: { ok: true, content: '{"sh600519":{"price":1437.0}}' },
      },
    ]);
  });

  it("轮末兜底行（result=null）如实展示「无结果」，错误行 ok:false", () => {
    const out = hydrateEntries([
      row({ id: 8, role: "tool", tool_json: JSON.stringify({ name: "refresh_quotes", arguments: "{}", result: null, is_error: true }) }),
      row({ id: 9, role: "tool", tool_json: JSON.stringify({ name: "get_quote", arguments: "{}", result: "上游失败", is_error: true }) }),
    ]);
    expect(out[0]).toMatchObject({ result: { ok: false, content: "（无结果：轮次在收到工具结果前结束）" } });
    expect(out[1]).toMatchObject({ result: { ok: false, content: "上游失败" } });
  });

  it("tool_json 坏数据降级错误 notice，不抛不崩", () => {
    const out = hydrateEntries([row({ id: 3, role: "tool", tool_json: "{坏的" })]);
    expect(out[0]).toMatchObject({ kind: "notice", level: "error" });
    expect((out[0] as { text: string }).text).toContain("#3");
  });

  it("未知 role 走 notice 兜底（词汇 CHECK 之外的保险丝）", () => {
    const out = hydrateEntries([row({ id: 4, role: "thinking" as ChatMessageRow["role"], text: "不该在档案里" })]);
    expect(out[0]).toMatchObject({ kind: "notice", level: "error" });
  });

  it("顺序保持服务端升序原样", () => {
    const out = hydrateEntries([
      row({ id: 1, role: "user", text: "a" }),
      row({ id: 2, role: "tool", tool_json: '{"name":"echo","arguments":"","result":"ok","is_error":false}' }),
      row({ id: 3, role: "assistant", text: "b" }),
    ]);
    expect(out.map((e) => e.kind)).toEqual(["user", "tool", "assistant"]);
  });
});
