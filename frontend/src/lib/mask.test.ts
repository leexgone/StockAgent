import { describe, expect, it } from "vitest";
import { maskChatId } from "./mask";

describe("maskChatId（会话绑定展示脱敏）", () => {
  it("微信 ilink id：留前 4 + ****，保留 @domain", () => {
    expect(maskChatId("private:abcd1234efgh5678@im.wechat")).toBe("private:abcd****@im.wechat");
  });

  it("QQ 群号：纯数字留前 2 后 2", () => {
    expect(maskChatId("group:123456789")).toBe("group:12****89");
  });

  it("QQ 私聊号同规则", () => {
    expect(maskChatId("private:123456789")).toBe("private:12****89");
  });

  it("短 id（≤4 字符）全掩", () => {
    expect(maskChatId("group:1234")).toBe("group:****");
    expect(maskChatId("private:ab12")).toBe("private:****");
  });

  it("无前缀兜底同规则掩码", () => {
    expect(maskChatId("bare-id-long")).toBe("bare****");
  });

  it("空串原样返回", () => {
    expect(maskChatId("")).toBe("");
  });
});