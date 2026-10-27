/**
 * 黄金测试向量 runner（P3-T4，TS 共享层）。
 *
 * 读取仓库根 `fixtures/golden/*.json`——与 Rust 侧 runner
 * （`backend/crates/zimu-domain/tests/golden.rs`）跑**同一份文件**，
 * 两边实现永不互相对拍，只对 fixture。比对规则见 `docs/03-domain-rules.md` 附录：
 * - 数值：|实现 − 期望| ≤ tol（case 级 ?? 文件级 ?? 1e-9）；非有限数（NaN/Inf）直接判失败
 * - 布尔/字符串/枚举：精确相等（阈值压线的浮点噪声由实现内 THRESHOLD_EPS 消化）
 * - 数组：同长按序对齐；`null`：实现必须返回 null；expect 未列字段不校验
 * - `status != "active"` 的组（如 price_limit pending）整组跳过
 */
import { readdirSync, readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";

import { describe, expect, it } from "vitest";

import * as domain from "./index";

const FIXTURES_DIR = fileURLToPath(new URL("../../../fixtures/golden", import.meta.url));

type Json = null | boolean | number | string | Json[] | { [key: string]: Json };

interface FixtureCase {
  id: string;
  fn: string;
  desc: string;
  tags: string[];
  tolerance?: number;
  /** fixture 输入是运行时数据，键与各函数的 Input 类型一一对应（黄金向量本身就是契约） */
  // eslint 不在本工程，显式 any 只为运行时 JSON 直通
  input: any;
  expect: Record<string, Json>;
}

interface FixtureFile {
  group: string;
  title: string;
  spec: string;
  schema_version: number;
  status: string;
  tolerance?: number;
  notes?: string;
  cases: FixtureCase[];
}

/** fixture 的 `fn` 串（snake_case，见 03 附录映射表）→ 本层函数（camelCase） */
function compute(group: string, fn: string, input: any): Json {
  switch (`${group}/${fn}`) {
    case "lot/lot_size":
      return { lot: domain.lotSize(input.kind) };
    case "lot/price_decimals":
      return { decimals: domain.priceDecimals(input.kind) };
    case "lot/validate_qty":
      return domain.validateQty(input.kind, input.side, input.qty, input.held);
    case "lot/quick_tiers":
      return { tiers: domain.quickTiers(input.kind, input.side, input.held) };
    case "fees/fee":
      return domain.fee(input);
    case "dilution/apply_trade":
      return domain.applyTrade(input);
    case "recalc/recalc":
      return domain.recalc(input);
    case "discipline/check":
      return domain.check(input);
    case "discipline/sell_to":
      return domain.sellTo(input);
    case "perf/perf":
      return domain.perf(input);
    case "vol_synth/vol_synth":
      return domain.volSynth(input);
    default:
      throw new Error(`未知 组/函数：${group}/${fn}`);
  }
}

function isRecord(v: Json): v is { [key: string]: Json } {
  return typeof v === "object" && v !== null && !Array.isArray(v);
}

function compare(path: string, expected: Json, computed: Json, tol: number, errs: string[]): void {
  if (isRecord(expected) && isRecord(computed)) {
    for (const [k, ev] of Object.entries(expected)) {
      if (!(k in computed)) {
        errs.push(`${path}.${k}: 实现输出缺少该字段（期望 ${JSON.stringify(ev)}）`);
        continue;
      }
      compare(`${path}.${k}`, ev, computed[k], tol, errs);
    }
    return;
  }
  if (Array.isArray(expected) && Array.isArray(computed)) {
    if (expected.length !== computed.length) {
      errs.push(`${path}: 数组长度不符 期望 ${expected.length} 实际 ${computed.length}`);
      return;
    }
    expected.forEach((ev, i) => compare(`${path}[${i}]`, ev, computed[i], tol, errs));
    return;
  }
  if (typeof expected === "number" && typeof computed === "number") {
    const diff = Math.abs(expected - computed);
    // !(diff ≤ tol)：实现吐出 NaN 时 diff 为 NaN，比较恒 false，取反后判失败
    if (!(diff <= tol)) {
      errs.push(
        `${path}: 期望 ${expected} 实际 ${computed}（差 ${diff.toExponential(3)}，容差 ${tol.toExponential()}）`,
      );
    }
    return;
  }
  if (expected === null && computed === null) return;
  if (expected !== computed) {
    errs.push(`${path}: 期望 ${JSON.stringify(expected)} 实际 ${JSON.stringify(computed)}`);
  }
}

const fixtureFiles = readdirSync(FIXTURES_DIR)
  .filter((f) => f.endsWith(".json"))
  .sort();

if (fixtureFiles.length === 0) {
  throw new Error(`fixtures/golden 下没有 JSON 文件：${FIXTURES_DIR}`);
}

for (const file of fixtureFiles) {
  const doc = JSON.parse(readFileSync(`${FIXTURES_DIR}/${file}`, "utf8")) as FixtureFile;
  const stem = file.replace(/\.json$/, "");

  describe(`${doc.group} · ${doc.title}`, () => {
    it("group 与文件名一致", () => {
      expect(doc.group).toBe(stem);
    });

    if (doc.status !== "active") {
      // pending 组（price_limit 等 B2/P1-T2 定稿）整组跳过，与 Rust runner 行为一致
      it.skip(`status=${doc.status}：整组跳过（定稿条件见 fixture notes）`, () => {});
      return;
    }

    const fileTol = doc.tolerance ?? 1e-9;
    for (const c of doc.cases) {
      it(c.id, () => {
        const tol = c.tolerance ?? fileTol;
        const computed = compute(doc.group, c.fn, c.input);
        const errs: string[] = [];
        compare("expect", c.expect, computed, tol, errs);
        expect(errs, errs.join("\n")).toEqual([]);
      });
    }
  });
}
