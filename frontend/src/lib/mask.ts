/**
 * chat_id 展示掩码（会话绑定卡：默认脱敏 + 逐行「查看/收起」切换）。
 * 规则：保留 private:/group: 前缀与 @domain 段；纯数字 id（QQ 号）留前 2 后 2，
 * 其余（微信 ilink id）留前 4；中段一律 ****，短 id（≤4 字符）全掩。
 * 掩码只是展示层投影——store/API payload 原样（后端唯一真值，ADR-0008），
 * 绑定/解绑请求仍用完整 chat_id。
 */
export function maskChatId(chatId: string): string {
  if (!chatId) return chatId;
  const m = /^(private:|group:)?([\s\S]*)$/.exec(chatId);
  const prefix = m?.[1] ?? "";
  const rest = m?.[2] ?? chatId;
  const at = rest.indexOf("@");
  const local = at >= 0 ? rest.slice(0, at) : rest;
  const domain = at >= 0 ? rest.slice(at) : "";
  const masked = /^\d+$/.test(local)
    ? local.length <= 4
      ? "****"
      : `${local.slice(0, 2)}****${local.slice(-2)}`
    : local.length <= 4
      ? "****"
      : `${local.slice(0, 4)}****`;
  return prefix + masked + domain;
}