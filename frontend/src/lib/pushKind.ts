/**
 * push_log 展示词汇（ADR-0017 F2）：kind → 标签、channel → 标签。
 * 自 HomeScreen/ChatView 的本地 KIND_LABEL 抽出（两处本就同映射），并补齐
 * 缺失的 post_market_review_push（QQ/微信盘后摘要推送实际写入的 kind）。
 * 注意与 lib/kind.ts 的持仓 KIND_LABEL 是**两套词汇**，不混用。
 * kind 是封闭词汇（随渠道落地扩展，03 §12）；未知值原样回落，不伪造。
 */

export const PUSH_KIND_LABEL: Record<string, string> = {
  post_market_review: "盘后复盘",
  alert_push: "预警推送",
  post_market_review_push: "盘后摘要推送",
  chat_push: "问答推送",
};

export function pushKindLabel(kind: string): string {
  return PUSH_KIND_LABEL[kind] ?? kind;
}

const CHANNEL_LABEL: Record<string, string> = {
  agent: "定时任务",
  qq: "QQ",
  wechat: "微信",
};

export function channelLabel(channel: string): string {
  return CHANNEL_LABEL[channel] ?? channel;
}