/**
 * 格式化负零防御（2026-10-30 真实数据修复）：-0 与舍入到显示精度后为零的
 * 微负数不得带负号（"-0" / "-0.00"）；正常负数原样保留。
 */

import { describe, expect, it } from "vitest";
import { fixed, fmt, money, pct, wan } from "./format";

describe("format 负零防御", () => {
  it("fmt 剥掉负零与舍零微负数的负号", () => {
    expect(fmt(-0, 0)).toBe("0");
    expect(fmt(-0.004, 0)).toBe("0");
    expect(fmt(-0.001, 2)).toBe("0.00");
    expect(fmt(-0)).toBe("0.00");
  });

  it("fmt 正常负数原样保留", () => {
    expect(fmt(-1234.5)).toBe("-1,234.50");
    expect(fmt(-0.6, 2)).toBe("-0.60");
  });

  it("pct/money/wan/fixed 同规则收口", () => {
    expect(pct(-0.001)).toBe("0.00%");
    expect(pct(0.5)).toBe("+0.50%");
    expect(money(-0.002)).toBe("¥0.00");
    expect(wan(-4)).toBe("0.00 万");
    expect(fixed(-0.04, 1)).toBe("0.0");
    expect(fixed(-1.234, 1)).toBe("-1.2");
  });
});
