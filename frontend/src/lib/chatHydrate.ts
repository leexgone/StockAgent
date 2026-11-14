/**
 * 会话档案水合（T4/ADR-0011）：服务端 chat_messages 行 → ChatPanel 展示条目。
 *
 * 纯函数（vitest 直测）。真值纪律：档案是会话唯一真值，本函数只做形态转换——
 * - user/assistant 原样；assistant 一律 done:true（档案里没有"进行中"的轮）；
 * - tool 行解析 tool_json（recorder 落库形态 {name,arguments,result,is_error}）；
 *   **解析失败降级为错误 notice，不崩、不静默丢**（与全站"降级如实"纪律一致）；
 * - result=null 的兜底行（轮末未收到结果）如实展示"无结果"；
 * - 未知 role（词汇 CHECK 理论上不会漏进来）同样 notice 兜底。
 */

import type { components } from "../api/schema";
import type { ChatEntry } from "../components/ChatPanel";
import { parseFileAnnotations } from "./fileAnnotations";

export type ChatMessageRow = components["schemas"]["ChatMessageRow"];

/** 档案 tool 行的落库形态（backend chat_record.rs RowSpec::Tool） */
interface ToolSnapshot {
  name?: unknown;
  arguments?: unknown;
  result?: unknown;
  is_error?: unknown;
}

export function hydrateEntries(rows: ChatMessageRow[]): ChatEntry[] {
  const out: ChatEntry[] = [];
  for (const r of rows) {
    switch (r.role) {
      case "user": {
        // ADR-0015：档案 user 行含附件标注行 → 剥离正文、还原 chips（与 live 同形态）
        const { body, files } = parseFileAnnotations(r.text);
        out.push({ kind: "user", text: body, ...(files.length ? { files } : {}) });
        break;
      }
      case "assistant":
        out.push({ kind: "assistant", text: r.text, done: true });
        break;
      case "tool":
        out.push(hydrateToolRow(r));
        break;
      default:
        out.push({
          kind: "notice",
          level: "error",
          text: `档案行 #${r.id} 角色未知（${String(r.role)}），已按原文跳过`,
        });
    }
  }
  return out;
}

function hydrateToolRow(r: ChatMessageRow): ChatEntry {
  let snap: ToolSnapshot;
  try {
    snap = JSON.parse(r.tool_json ?? "") as ToolSnapshot;
  } catch {
    return {
      kind: "notice",
      level: "error",
      text: `档案工具行 #${r.id} 解析失败（内容已跳过）`,
    };
  }
  const name = typeof snap.name === "string" && snap.name ? snap.name : "工具";
  const args = typeof snap.arguments === "string" ? snap.arguments : "";
  const noResult = snap.result == null;
  return {
    kind: "tool",
    id: `hist-${r.id}`,
    name,
    args,
    result: noResult
      ? { ok: false, content: "（无结果：轮次在收到工具结果前结束）" }
      : { ok: snap.is_error !== true, content: String(snap.result) },
  };
}
