//! `MarketApi` trait（测试注入点）与 `HttpMarketClient`（sidecar HTTP 实现）。
//!
//! 错误映射（`error.rs` 三分类的落地处）：
//! - 连接拒绝/超时/URL 非法 → `Unavailable`（降级信号）
//! - 非 2xx → 解析 sidecar 错误契约 `{error:{code,message}}` → `Upstream`
//!   （解析不了也不丢状态码，归 `HTTP_<n>`）
//! - 2xx 但反序列化失败 → `Contract`（两边版本漂移，要修代码）
//!
//! 唯一消费入口（04 附录 B）：全代码库只有这里对 sidecar 发 HTTP。

use std::time::Duration;

use async_trait::async_trait;
use serde::de::DeserializeOwned;

use crate::error::{MarketError, MarketResult};
use crate::models::{
    CalendarPayload, CbListPayload, ErrorBody, KlinePayload, QuotePayload, SectorMapPayload,
    SidecarHealth,
};

/// sidecar 缺省地址（datafetch/README：`uv run uvicorn ... --port 8300`）
pub const DEFAULT_BASE_URL: &str = "http://127.0.0.1:8300";
/// 缺省整请求超时：quote 合并请求 ~0.3s、kline 单标的 ~1s，10s 足够宽又不至于
/// 把 /api/market/* 挂死；sidecar 侧还有自己的限频排队。
pub const DEFAULT_TIMEOUT: Duration = Duration::from_secs(10);
/// `/sector_map` 单请求超时（P7-T6）：冷建映射 1+49 次新浪外呼可达 30–90s，
/// 默认 10s 必超时——单请求覆盖客户端超时（reqwest per-request timeout）。
pub const SECTOR_MAP_TIMEOUT: Duration = Duration::from_secs(180);

#[async_trait]
pub trait MarketApi: Send + Sync {
    /// sidecar 基地址（诊断透出用）
    fn base_url(&self) -> &str;
    async fn health(&self) -> MarketResult<SidecarHealth>;
    /// symbols 必须是完整前缀形态（sh600519，B7）；拼装责任在调用方
    async fn quote(&self, symbols: &[String]) -> MarketResult<QuotePayload>;
    async fn kline(
        &self,
        symbol: &str,
        kind: Option<&str>,
        start: Option<&str>,
        end: Option<&str>,
    ) -> MarketResult<KlinePayload>;
    async fn cb_list(&self) -> MarketResult<CbListPayload>;
    /// start/end None = sidecar 默认窗口（今天 ±180 天，含未来交易日）
    async fn calendar(
        &self,
        start: Option<&str>,
        end: Option<&str>,
    ) -> MarketResult<CalendarPayload>;
    /// 全量「6 位代码→新浪行业板名」映射（P7-T6）。冷建可达分钟级，
    /// 实现侧自带长超时（`SECTOR_MAP_TIMEOUT`）；调用方宜缓存（SectorMapCache）。
    async fn sector_map(&self) -> MarketResult<SectorMapPayload>;
}

pub struct HttpMarketClient {
    base: String,
    http: reqwest::Client,
}

impl HttpMarketClient {
    pub fn new(base_url: &str, timeout: Duration) -> MarketResult<Self> {
        let http = reqwest::Client::builder()
            .timeout(timeout)
            .build()
            .map_err(|e| MarketError::Unavailable(format!("HTTP 客户端构建失败：{e}")))?;
        Ok(Self {
            base: base_url.trim_end_matches('/').to_string(),
            http,
        })
    }

    /// 环境变量装配：`ZIMU_MARKET_URL`（默认 127.0.0.1:8300）、
    /// `ZIMU_MARKET_TIMEOUT_MS`（默认 10000）。
    pub fn from_env() -> MarketResult<Self> {
        let url = std::env::var("ZIMU_MARKET_URL").unwrap_or_else(|_| DEFAULT_BASE_URL.into());
        let timeout_ms = std::env::var("ZIMU_MARKET_TIMEOUT_MS")
            .ok()
            .and_then(|v| v.parse::<u64>().ok())
            .unwrap_or(DEFAULT_TIMEOUT.as_millis() as u64);
        Self::new(&url, Duration::from_millis(timeout_ms))
    }

    async fn get_json<T: DeserializeOwned>(
        &self,
        path: &str,
        query: &[(&str, Option<&str>)],
    ) -> MarketResult<T> {
        self.get_json_with_timeout(path, query, None).await
    }

    /// `timeout` Some = 单请求覆盖客户端缺省超时（sector_map 冷建用）；
    /// None = 用客户端缺省。错误三分类逻辑两条路径共用。
    async fn get_json_with_timeout<T: DeserializeOwned>(
        &self,
        path: &str,
        query: &[(&str, Option<&str>)],
        timeout: Option<Duration>,
    ) -> MarketResult<T> {
        let url = format!("{}{}", self.base, path);
        let pairs: Vec<(&str, &str)> = query
            .iter()
            .filter_map(|(k, v)| v.map(|v| (*k, v)))
            .collect();
        let mut req = self.http.get(&url).query(&pairs);
        if let Some(t) = timeout {
            req = req.timeout(t);
        }
        let resp = req
            .send()
            .await
            .map_err(|e| {
                MarketError::Unavailable(if e.is_timeout() {
                    format!("请求 {path} 超时")
                } else {
                    format!("请求 {path} 失败：{e}")
                })
            })?;
        let status = resp.status();
        if !status.is_success() {
            // sidecar 错误契约 {error:{code,message,detail}}；解析不了归 HTTP_<n>
            let parsed = resp.json::<ErrorBody>().await.ok();
            let (code, message) = match parsed {
                Some(b) => (b.error.code, b.error.message),
                None => (
                    format!("HTTP_{}", status.as_u16()),
                    format!("sidecar 返回 {status}（非契约响应体）"),
                ),
            };
            return Err(MarketError::Upstream {
                status: status.as_u16(),
                code,
                message,
            });
        }
        resp.json::<T>()
            .await
            .map_err(|e| MarketError::Contract(format!("{path} 响应反序列化失败：{e}")))
    }
}

#[async_trait]
impl MarketApi for HttpMarketClient {
    fn base_url(&self) -> &str {
        &self.base
    }

    async fn health(&self) -> MarketResult<SidecarHealth> {
        self.get_json("/healthz", &[]).await
    }

    async fn quote(&self, symbols: &[String]) -> MarketResult<QuotePayload> {
        let codes = symbols.join(",");
        self.get_json("/quote", &[("codes", Some(codes.as_str()))])
            .await
    }

    async fn kline(
        &self,
        symbol: &str,
        kind: Option<&str>,
        start: Option<&str>,
        end: Option<&str>,
    ) -> MarketResult<KlinePayload> {
        // symbol 只含 [a-z0-9]（symbols.rs 拼装保证），路径拼接安全
        let path = format!("/kline/{symbol}");
        self.get_json(&path, &[("kind", kind), ("start", start), ("end", end)])
            .await
    }

    async fn cb_list(&self) -> MarketResult<CbListPayload> {
        self.get_json("/cb_list", &[]).await
    }

    async fn calendar(
        &self,
        start: Option<&str>,
        end: Option<&str>,
    ) -> MarketResult<CalendarPayload> {
        self.get_json(
            "/calendar",
            &[("start", start), ("end", end)],
        )
        .await
    }

    async fn sector_map(&self) -> MarketResult<SectorMapPayload> {
        self.get_json_with_timeout("/sector_map", &[], Some(SECTOR_MAP_TIMEOUT))
            .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;

    /// 一次性裸 HTTP 假 sidecar：读完请求头后回固定响应（测错误映射，不引框架）。
    async fn spawn_fake(raw_response: String) -> String {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            if let Ok((mut sock, _)) = listener.accept().await {
                let mut buf = [0u8; 2048];
                let _ = sock.read(&mut buf).await; // 读掉请求（内容不校验）
                let _ = sock.write_all(raw_response.as_bytes()).await;
                let _ = sock.shutdown().await;
            }
        });
        format!("http://{addr}")
    }

    fn http_response(status: &str, body: &str) -> String {
        format!(
            "HTTP/1.1 {status}\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
            body.len()
        )
    }

    #[tokio::test]
    async fn connection_refused_maps_to_unavailable() {
        // 127.0.0.1:1 无监听 → 立即拒绝；这是「sidecar 挂了」的降级信号
        let c = HttpMarketClient::new("http://127.0.0.1:1", Duration::from_secs(2)).unwrap();
        let err = c.quote(&["sh600519".into()]).await.unwrap_err();
        assert!(matches!(err, MarketError::Unavailable(_)), "{err:?}");
    }

    #[tokio::test]
    async fn error_contract_maps_to_upstream() {
        let base = spawn_fake(http_response(
            "502 Bad Gateway",
            r#"{"error":{"code":"UPSTREAM_ALL_FAILED","message":"全部行情源失败","detail":{"attempts":[]}}}"#,
        ))
        .await;
        let c = HttpMarketClient::new(&base, Duration::from_secs(5)).unwrap();
        let err = c.quote(&["sh600519".into()]).await.unwrap_err();
        match err {
            MarketError::Upstream {
                status,
                code,
                message,
            } => {
                assert_eq!(status, 502);
                assert_eq!(code, "UPSTREAM_ALL_FAILED");
                assert_eq!(message, "全部行情源失败");
            }
            other => panic!("期望 Upstream，得到 {other:?}"),
        }
    }

    #[tokio::test]
    async fn bad_symbol_400_passes_through() {
        let base = spawn_fake(http_response(
            "400 Bad Request",
            r#"{"error":{"code":"BAD_SYMBOL","message":"必须带显式市场前缀","detail":{"input":"600519"}}}"#,
        ))
        .await;
        let c = HttpMarketClient::new(&base, Duration::from_secs(5)).unwrap();
        let err = c.quote(&["600519".into()]).await.unwrap_err();
        match err {
            MarketError::Upstream { status, code, .. } => {
                assert_eq!((status, code.as_str()), (400, "BAD_SYMBOL"))
            }
            other => panic!("期望 Upstream，得到 {other:?}"),
        }
    }

    #[tokio::test]
    async fn non_contract_error_body_keeps_status() {
        let base = spawn_fake(http_response("500 Internal Server Error", "plain text oops"))
            .await;
        let c = HttpMarketClient::new(&base, Duration::from_secs(5)).unwrap();
        let err = c.calendar(None, None).await.unwrap_err();
        match err {
            MarketError::Upstream { status, code, .. } => {
                assert_eq!(status, 500);
                assert_eq!(code, "HTTP_500");
            }
            other => panic!("期望 Upstream，得到 {other:?}"),
        }
    }

    #[tokio::test]
    async fn garbage_200_maps_to_contract() {
        let base = spawn_fake(http_response("200 OK", r#"{"nope": true}"#)).await;
        let c = HttpMarketClient::new(&base, Duration::from_secs(5)).unwrap();
        let err = c.quote(&["sh600519".into()]).await.unwrap_err();
        assert!(matches!(err, MarketError::Contract(_)), "{err:?}");
    }

    #[tokio::test]
    async fn sector_map_deserializes_typed_payload() {
        let body = r#"{"as_of":"2026-11-15T10:00:00+08:00","count":2,"source":"sina_sector",
          "degraded":false,"attempts":[],
          "sectors":{"600519":"酿酒行业","600011":"电力行业"}}"#;
        let base = spawn_fake(http_response("200 OK", body)).await;
        let c = HttpMarketClient::new(&base, Duration::from_secs(5)).unwrap();
        let p = c.sector_map().await.unwrap();
        assert_eq!(p.count, 2);
        assert_eq!(p.sectors.get("600519").map(String::as_str), Some("酿酒行业"));
        assert!(!p.degraded);
    }

    #[tokio::test]
    async fn success_deserializes_typed_payload() {
        let body = r#"{"as_of":"2026-10-28T15:40:01+08:00","count":1,"degraded":false,"attempts":[],
          "quotes":[{"symbol":"sh600519","code":"600519","mkt":"SH","kind":"stock","name":"贵州茅台",
            "price":1437.0,"prev":1421.0,"open":1425.0,"high":1440.0,"low":1420.0,
            "volume":3123400.0,"amount":4480000000.0,"chg_pct":1.126,
            "date":"2026-10-28","time":"15:00:03","status":"ok","source":"sina_hq"}]}"#;
        let base = spawn_fake(http_response("200 OK", body)).await;
        let c = HttpMarketClient::new(&base, Duration::from_secs(5)).unwrap();
        let p = c.quote(&["sh600519".into()]).await.unwrap();
        assert_eq!(p.count, 1);
        assert_eq!(p.quotes[0].price, Some(1437.0));
        assert_eq!(p.quotes[0].status, crate::models::QuoteStatus::Ok);
        assert!(!p.degraded);
    }
}
