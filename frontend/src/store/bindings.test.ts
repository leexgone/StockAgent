import { describe, expect, it } from "vitest";
import { kindForChatId } from "./bindings";

describe("kindForChatId（待绑定一键绑定的类型推断，ADR-0017 D3 补记）", () => {
  it("group: 前缀 → 群聊（QQ）", () => {
    expect(kindForChatId("group:ABC123")).toBe("group");
  });

  it("private: 前缀 → 私聊", () => {
    expect(kindForChatId("private:ABC123")).toBe("private");
  });

  it("微信 ilink id（private:…@im.wechat）恒私聊", () => {
    expect(kindForChatId("private:wx_demo_user@im.wechat")).toBe("private");
  });

  it("无前缀兜底私聊（不臆造群聊）", () => {
    expect(kindForChatId("bare-id")).toBe("private");
    expect(kindForChatId("")).toBe("private");
  });
});