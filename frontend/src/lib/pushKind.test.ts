import { describe, expect, it } from "vitest";
import { channelLabel, PUSH_KIND_LABEL, pushKindLabel } from "./pushKind";

describe("pushKind 展示词汇（ADR-0017 F2）", () => {
  it("三种 kind 全映射（含补齐的 post_market_review_push）", () => {
    expect(pushKindLabel("post_market_review")).toBe("盘后复盘");
    expect(pushKindLabel("alert_push")).toBe("预警推送");
    expect(pushKindLabel("post_market_review_push")).toBe("盘后摘要推送");
    expect(Object.keys(PUSH_KIND_LABEL)).toHaveLength(3);
  });

  it("未知 kind 原样回落，不伪造", () => {
    expect(pushKindLabel("future_kind")).toBe("future_kind");
    expect(pushKindLabel("")).toBe("");
  });

  it("channel 标签：agent/qq/wechat", () => {
    expect(channelLabel("agent")).toBe("定时任务");
    expect(channelLabel("qq")).toBe("QQ");
    expect(channelLabel("wechat")).toBe("微信");
  });

  it("未知 channel 原样回落", () => {
    expect(channelLabel("telegram")).toBe("telegram");
  });
});