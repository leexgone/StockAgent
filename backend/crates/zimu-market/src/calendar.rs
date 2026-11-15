//! 日历缓存：数据小、变化慢（sidecar 侧 TTL 6h，交易日表一天内不变），
//! 适配层再缓 1h——`/api/market/status` 每秒被前端轮也不打 sidecar。
//!
//! 降级语义（03 §9）：拉取失败**不缓存错误**；但过期的旧日历比工作日启发式
//! 更有价值（节假日判断仍然正确），所以调用方失败路径应回落 `stale()`，
//! 只有连旧缓存都没有才用 `CalendarView::unavailable()`。

use std::sync::Mutex;
use std::time::{Duration, Instant};

use crate::client::MarketApi;
use crate::clock::CalendarView;
use crate::error::MarketResult;
use crate::models::CalendarPayload;

pub const DEFAULT_CACHE_TTL: Duration = Duration::from_secs(3600);

pub struct CalendarCache {
    ttl: Duration,
    inner: Mutex<Option<(Instant, CalendarPayload)>>,
}

impl Default for CalendarCache {
    fn default() -> Self {
        Self::new(DEFAULT_CACHE_TTL)
    }
}

impl CalendarCache {
    pub fn new(ttl: Duration) -> Self {
        Self {
            ttl,
            inner: Mutex::new(None),
        }
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Option<(Instant, CalendarPayload)>> {
        self.inner.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// 未过期的缓存（无则 None，不打网络）。
    pub fn fresh(&self) -> Option<CalendarPayload> {
        self.lock()
            .as_ref()
            .filter(|(at, _)| at.elapsed() < self.ttl)
            .map(|(_, p)| p.clone())
    }

    /// 任意缓存（含过期）+ 是否过期。失败路径的降级料。
    pub fn stale(&self) -> Option<(CalendarPayload, bool)> {
        self.lock()
            .as_ref()
            .map(|(at, p)| (p.clone(), at.elapsed() >= self.ttl))
    }

    /// 取日历：fresh 缓存 → sidecar 实拉（成功回填缓存）。
    /// start/end 传 None 用 sidecar 默认窗口（今天 ±180 天，含未来交易日——
    /// 时钟「下一交易日」依赖）。失败原样返回 Err，由调用方决定降级路径。
    pub async fn get(
        &self,
        api: &dyn MarketApi,
        start: Option<&str>,
        end: Option<&str>,
    ) -> MarketResult<CalendarPayload> {
        if start.is_none() && end.is_none() {
            if let Some(p) = self.fresh() {
                return Ok(p);
            }
        }
        let p = api.calendar(start, end).await?;
        if start.is_none() && end.is_none() {
            *self.lock() = Some((Instant::now(), p.clone()));
        }
        Ok(p)
    }

    /// 时钟输入侧的便捷组装：实拉 → 失败退旧缓存（标 degraded）→ 全无退启发式。
    /// 第二返回值 = 是否处于日历降级（03 §9 的 `calendar_degraded` 依据之一
    /// 是「不可用」，旧缓存仍然可用，只是新鲜度未知——按可用处理）。
    pub async fn view_for_clock(&self, api: &dyn MarketApi) -> CalendarView {
        match self.get(api, None, None).await {
            Ok(p) => CalendarView::from_payload(&p),
            Err(e) => {
                tracing::warn!("交易日历不可用，市场时钟降级为工作日启发式：{e}");
                match self.stale() {
                    Some((p, expired)) => {
                        let mut v = CalendarView::from_payload(&p);
                        // 旧日历仍是日历（节假日判断正确）；过期只影响未来边界，
                        // upstream_degraded 如实透传「用的旧缓存」
                        v.upstream_degraded = true;
                        let _ = expired; // 新鲜度已并入 upstream_degraded 语义
                        v
                    }
                    None => CalendarView::unavailable(),
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::MarketError;
    use async_trait::async_trait;
    use std::sync::atomic::{AtomicUsize, Ordering};

    struct FakeCal {
        result: Option<CalendarPayload>,
        calls: AtomicUsize,
    }

    fn payload(dates: &[&str], source: &str) -> CalendarPayload {
        CalendarPayload {
            start: "2026-01-01".into(),
            end: "2026-12-31".into(),
            count: dates.len(),
            trade_dates: dates.iter().map(|s| s.to_string()).collect(),
            source: Some(source.into()),
            degraded: false,
            attempts: vec![],
        }
    }

    #[async_trait]
    impl MarketApi for FakeCal {
        fn base_url(&self) -> &str {
            "fake://cal"
        }
        async fn health(&self) -> MarketResult<crate::models::SidecarHealth> {
            unimplemented!()
        }
        async fn quote(&self, _: &[String]) -> MarketResult<crate::models::QuotePayload> {
            unimplemented!()
        }
        async fn kline(
            &self,
            _: &str,
            _: Option<&str>,
            _: Option<&str>,
            _: Option<&str>,
        ) -> MarketResult<crate::models::KlinePayload> {
            unimplemented!()
        }
        async fn cb_list(&self) -> MarketResult<crate::models::CbListPayload> {
            unimplemented!()
        }
        async fn calendar(
            &self,
            _: Option<&str>,
            _: Option<&str>,
        ) -> MarketResult<CalendarPayload> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            match &self.result {
                Some(p) => Ok(p.clone()),
                None => Err(MarketError::Unavailable("sidecar 挂了".into())),
            }
        }
        async fn sector_map(&self) -> MarketResult<crate::models::SectorMapPayload> {
            unimplemented!()
        }
    }

    #[tokio::test]
    async fn caches_within_ttl() {
        let fake = FakeCal {
            result: Some(payload(&["2026-10-28"], "sina_calendar")),
            calls: AtomicUsize::new(0),
        };
        let c = CalendarCache::new(Duration::from_secs(60));
        let a = c.get(&fake, None, None).await.unwrap();
        let b = c.get(&fake, None, None).await.unwrap();
        assert_eq!(a.trade_dates, b.trade_dates);
        assert_eq!(fake.calls.load(Ordering::SeqCst), 1); // 第二次命中缓存
    }

    #[tokio::test]
    async fn expired_cache_refetches() {
        let fake = FakeCal {
            result: Some(payload(&["2026-10-28"], "sina_calendar")),
            calls: AtomicUsize::new(0),
        };
        let c = CalendarCache::new(Duration::ZERO); // 立即过期
        c.get(&fake, None, None).await.unwrap();
        c.get(&fake, None, None).await.unwrap();
        assert_eq!(fake.calls.load(Ordering::SeqCst), 2);
    }

    #[tokio::test]
    async fn failure_falls_back_to_stale_then_heuristic() {
        // 先成功缓存一次
        let ok = FakeCal {
            result: Some(payload(&["2026-10-28"], "sina_calendar")),
            calls: AtomicUsize::new(0),
        };
        let c = CalendarCache::new(Duration::ZERO); // 过期，但 stale 仍可用
        c.get(&ok, None, None).await.unwrap();

        // sidecar 挂：view_for_clock 退旧缓存（可用，标 upstream_degraded）
        let down = FakeCal {
            result: None,
            calls: AtomicUsize::new(0),
        };
        let v = c.view_for_clock(&down).await;
        assert!(v.trade_dates.is_some() && v.upstream_degraded);

        // 连旧缓存都没有：彻底不可用 → 时钟启发式
        let empty = CalendarCache::default();
        let v = empty.view_for_clock(&down).await;
        assert!(v.trade_dates.is_none() && v.source.is_none());
    }

    #[tokio::test]
    async fn explicit_window_bypasses_cache() {
        let fake = FakeCal {
            result: Some(payload(&["2026-10-28"], "sina_calendar")),
            calls: AtomicUsize::new(0),
        };
        let c = CalendarCache::new(Duration::from_secs(60));
        c.get(&fake, Some("2026-10-01"), Some("2026-10-31"))
            .await
            .unwrap();
        c.get(&fake, Some("2026-10-01"), Some("2026-10-31"))
            .await
            .unwrap();
        assert_eq!(fake.calls.load(Ordering::SeqCst), 2); // 显式窗口直连，不进缓存
    }
}
