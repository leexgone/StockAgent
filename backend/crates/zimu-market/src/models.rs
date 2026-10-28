//! sidecar 传输契约的 serde 模型（与 `datafetch/README.md`、docs/04 附录 B 对齐）。
//!
//! 约定：
//! - 可空字段一律 `Option<f64/String>`——sidecar 的 `_num()` 会把 ''/'-' 归一成
//!   null，停牌/退市行大量字段为 null，这里不许用默认值掩盖「无数据」。
//! - `#[serde(default)]` 只用于列表/新增字段的前向兼容，不用于数字字段。
//! - 这些是**传输模型**不是领域模型：`kind`/`status` 保持 sidecar 的字符串枚举，
//!   转 `zimu_domain::Kind` 在调用方做（本 crate 不依赖 domain，见 lib.rs）。

use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

/// 实时行状态（P4-T3 四态）。未知值不 panic，归 `Unknown`（前向兼容）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum QuoteStatus {
    Ok,
    /// 有行但无有效价（停牌/退市冻结价）
    HaltedOrDelisted,
    /// 有行但字段为空串（深市退市样本形态）
    EmptyOrDelisted,
    /// 源整行缺失（非法代码/覆盖范围外），service 合成的占位行
    NoData,
    #[serde(other)]
    Unknown,
}

/// 统一实时行（/quote `quotes[]`；所有通道同键，04 附录 B）。
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct QuoteRow {
    pub symbol: String,
    pub code: String,
    pub mkt: String,
    pub kind: String,
    pub name: Option<String>,
    pub price: Option<f64>,
    pub prev: Option<f64>,
    pub open: Option<f64>,
    pub high: Option<f64>,
    pub low: Option<f64>,
    pub volume: Option<f64>,
    pub amount: Option<f64>,
    pub chg_pct: Option<f64>,
    /// 行情时点随行显式（上海本地日期 YYYY-MM-DD）——当日 bar 合成的判据
    pub date: Option<String>,
    pub time: Option<String>,
    pub status: QuoteStatus,
    pub source: Option<String>,
}

/// 降级明细：逐源原因（disabled/熔断/异常），04 附录 B「降级可观测」。
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct Attempt {
    pub source: String,
    pub reason: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct QuotePayload {
    pub as_of: String,
    pub count: usize,
    pub degraded: bool,
    #[serde(default)]
    pub attempts: Vec<Attempt>,
    pub quotes: Vec<QuoteRow>,
}

/// 日线 bar（新浪系 amount 可缺省；单位：股票/ETF 股/份，转债 张）。
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct KlineBar {
    pub date: String,
    pub open: Option<f64>,
    pub high: Option<f64>,
    pub low: Option<f64>,
    pub close: Option<f64>,
    pub volume: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub amount: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct KlinePayload {
    pub symbol: String,
    pub kind: String,
    /// explicit | inferred（缺省 kind 时 sidecar 内部号段路由的标注）
    pub kind_routed: String,
    /// qfq | none
    pub adjusted: String,
    pub start: String,
    pub end: String,
    pub count: usize,
    pub source: Option<String>,
    pub degraded: bool,
    #[serde(default)]
    pub attempts: Vec<Attempt>,
    pub kline: Vec<KlineBar>,
}

/// 转债列表现券行 + 集思录强赎字段（redeem 源失败时 13 个 redeem 键全 null，
/// `redeem_source` 为 null——降级不断供，P4-T3 契约）。
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct CbItem {
    pub symbol: String,
    pub code: String,
    pub mkt: String,
    pub kind: String,
    pub name: Option<String>,
    pub price: Option<f64>,
    /// 现券行的昨基准是**结算价**（settlement）
    pub prev: Option<f64>,
    pub open: Option<f64>,
    pub high: Option<f64>,
    pub low: Option<f64>,
    pub volume: Option<f64>,
    pub amount: Option<f64>,
    pub chg_pct: Option<f64>,
    pub ticktime: Option<String>,
    pub status: QuoteStatus,
    pub spot_source: Option<String>,
    // ---- 集思录强赎表（可选源）----
    #[serde(default)]
    pub stock_code: Option<String>,
    #[serde(default)]
    pub stock_name: Option<String>,
    #[serde(default)]
    pub convert_price: Option<f64>,
    #[serde(default)]
    pub trigger_pct: Option<f64>,
    #[serde(default)]
    pub trigger_price: Option<f64>,
    #[serde(default)]
    pub stock_price: Option<f64>,
    #[serde(default)]
    pub redeem_price: Option<f64>,
    #[serde(default)]
    pub last_trade_date: Option<String>,
    #[serde(default)]
    pub maturity_date: Option<String>,
    #[serde(default)]
    pub scale: Option<f64>,
    #[serde(default)]
    pub scale_left: Option<f64>,
    #[serde(default)]
    pub redeem_status: Option<String>,
    #[serde(default)]
    pub redeem_hint: Option<String>,
    /// 本行 redeem 字段来自哪个源；null = 强赎源整体降级
    #[serde(default)]
    pub redeem_source: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct CbListPayload {
    pub as_of: String,
    pub count: usize,
    pub spot_source: Option<String>,
    pub redeem_source: Option<String>,
    pub degraded: bool,
    #[serde(default)]
    pub attempts: Vec<Attempt>,
    pub items: Vec<CbItem>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct CalendarPayload {
    pub start: String,
    pub end: String,
    pub count: usize,
    /// 升序 ISO 日期（YYYY-MM-DD），含未来交易日（时钟「下一交易日」依赖）
    pub trade_dates: Vec<String>,
    pub source: Option<String>,
    pub degraded: bool,
    #[serde(default)]
    pub attempts: Vec<Attempt>,
}

/// sidecar `/healthz`：只取 ok/version 强类型，其余（breakers/cache/
/// disabled_sources）原样透传——运维排查用，不参与业务判断。
/// 内部探测类型，不进 API 契约（status 端点响应是 server 侧的 SidecarStatus）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SidecarHealth {
    pub ok: bool,
    #[serde(default)]
    pub version: Option<String>,
    #[serde(flatten)]
    pub rest: serde_json::Map<String, serde_json::Value>,
}

/// sidecar 错误契约 `{error:{code,message,detail}}`（非 2xx 响应体）。
#[derive(Debug, Clone, Deserialize)]
pub struct ErrorBody {
    pub error: ErrorDetail,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ErrorDetail {
    pub code: String,
    pub message: String,
    #[serde(default)]
    pub detail: serde_json::Value,
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 真实形状样本（P4-T3 联网冒烟所见）：正常行 + 退市行 + no_data 合成行。
    #[test]
    fn deserialize_quote_payload_with_nulls() {
        let raw = r#"{
          "as_of": "2026-10-28T15:40:01+08:00", "count": 3,
          "degraded": true,
          "attempts": [{"source": "sina_hq", "reason": "ConnectTimeout: timed out"}],
          "quotes": [
            {"symbol": "sh600519", "code": "600519", "mkt": "SH", "kind": "stock",
             "name": "贵州茅台", "price": 1437.0, "prev": 1421.0, "open": 1425.0,
             "high": 1440.0, "low": 1420.0, "volume": 3123400.0, "amount": 4480000000.0,
             "chg_pct": 1.126, "date": "2026-10-28", "time": "15:00:03",
             "status": "ok", "source": "em_push2"},
            {"symbol": "sh110074", "code": "110074", "mkt": "SH", "kind": "cb",
             "name": null, "price": null, "prev": null, "open": null, "high": null,
             "low": null, "volume": null, "amount": null, "chg_pct": null,
             "date": null, "time": null, "status": "halted_or_delisted", "source": "sina_hq"},
            {"symbol": "sz127022", "code": "127022", "mkt": "SZ", "kind": "cb",
             "name": null, "price": null, "prev": null, "open": null, "high": null,
             "low": null, "volume": null, "amount": null, "chg_pct": null,
             "date": null, "time": null, "status": "no_data", "source": null}
          ]
        }"#;
        let p: QuotePayload = serde_json::from_str(raw).unwrap();
        assert_eq!(p.count, 3);
        assert!(p.degraded);
        assert_eq!(p.attempts[0].source, "sina_hq");
        assert_eq!(p.quotes[0].status, QuoteStatus::Ok);
        assert_eq!(p.quotes[0].price, Some(1437.0));
        assert_eq!(p.quotes[1].status, QuoteStatus::HaltedOrDelisted);
        assert_eq!(p.quotes[2].status, QuoteStatus::NoData);
        assert!(p.quotes[2].name.is_none());
    }

    #[test]
    fn deserialize_kline_bar_without_amount() {
        // 新浪指数日线无 amount 列：键缺省 → None（不是 0）
        let raw = r#"{"date": "2026-10-27", "open": 3253.24, "high": 3288.88,
                      "low": 3245.0, "close": 3280.5, "volume": 445566.0}"#;
        let b: KlineBar = serde_json::from_str(raw).unwrap();
        assert!(b.amount.is_none());
        assert_eq!(b.close, Some(3280.5));
    }

    #[test]
    fn deserialize_cb_item_redeem_degraded() {
        // 强赎源降级：13 个 redeem 键全 null + redeem_source null，行仍可用
        let raw = r#"{
          "symbol": "sz128136", "code": "128136", "mkt": "SZ", "kind": "cb",
          "name": "立讯转债", "price": 132.5, "prev": 131.0, "open": 131.2,
          "high": 133.0, "low": 130.8, "volume": 812340.0, "amount": 107000000.0,
          "chg_pct": 1.145, "ticktime": "2026-10-28 15:00:00", "status": "ok",
          "spot_source": "sina-cb-spot",
          "stock_code": null, "stock_name": null, "convert_price": null,
          "trigger_pct": null, "trigger_price": null, "stock_price": null,
          "redeem_price": null, "last_trade_date": null, "maturity_date": null,
          "scale": null, "scale_left": null, "redeem_status": null,
          "redeem_hint": null, "redeem_source": null
        }"#;
        let it: CbItem = serde_json::from_str(raw).unwrap();
        assert_eq!(it.status, QuoteStatus::Ok);
        assert!(it.redeem_source.is_none() && it.redeem_hint.is_none());
        assert_eq!(it.ticktime.as_deref(), Some("2026-10-28 15:00:00"));
    }

    #[test]
    fn unknown_status_does_not_panic() {
        let raw = r#"{
          "symbol": "sh600000", "code": "600000", "mkt": "SH", "kind": "stock",
          "name": null, "price": null, "prev": null, "open": null, "high": null,
          "low": null, "volume": null, "amount": null, "chg_pct": null,
          "date": null, "time": null, "status": "something_new", "source": null
        }"#;
        let r: QuoteRow = serde_json::from_str(raw).unwrap();
        assert_eq!(r.status, QuoteStatus::Unknown);
    }

    #[test]
    fn deserialize_health_keeps_extra_fields() {
        let raw = r#"{"ok": true, "version": "0.1.0",
                      "breakers": {"sina_hq": "closed"}, "cache": {"hits": 3},
                      "disabled_sources": []}"#;
        let h: SidecarHealth = serde_json::from_str(raw).unwrap();
        assert!(h.ok && h.version.as_deref() == Some("0.1.0"));
        assert!(h.rest.contains_key("breakers"));
    }
}
