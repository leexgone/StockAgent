//! 行业映射缓存（P7-T6/ADR-0016）：数据静态（板成员变化慢，sidecar 日级 TTL），
//! 适配层再缓 24h——刷新链需求驱动补全下，永久未映射代码（新股/退市）会令每轮
//! 刷新都打 sidecar，Rust 缓存把频率压到每 TTL 一次；sidecar 重启缓存清零时
//! stale 回落避免干等 30–90s 重建。
//!
//! 降级语义：拉取失败**不缓存错误**；调用方失败路径回落 `stale()`（旧映射仍可用，
//! 行业是静态属性），连旧缓存都没有才跳过本轮股票补全（service 补全 pass 承接）。

use std::sync::Mutex;
use std::time::{Duration, Instant};

use crate::client::MarketApi;
use crate::error::MarketResult;
use crate::models::SectorMapPayload;

/// 24h，与 sidecar 日级 TTL 对齐
pub const DEFAULT_CACHE_TTL: Duration = Duration::from_secs(86400);

pub struct SectorMapCache {
    ttl: Duration,
    inner: Mutex<Option<(Instant, SectorMapPayload)>>,
}

impl Default for SectorMapCache {
    fn default() -> Self {
        Self::new(DEFAULT_CACHE_TTL)
    }
}

impl SectorMapCache {
    pub fn new(ttl: Duration) -> Self {
        Self {
            ttl,
            inner: Mutex::new(None),
        }
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Option<(Instant, SectorMapPayload)>> {
        self.inner.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// 未过期的缓存（无则 None，不打网络）。
    pub fn fresh(&self) -> Option<SectorMapPayload> {
        self.lock()
            .as_ref()
            .filter(|(at, _)| at.elapsed() < self.ttl)
            .map(|(_, p)| p.clone())
    }

    /// 任意缓存（含过期）+ 是否过期。失败路径的降级料。
    pub fn stale(&self) -> Option<(SectorMapPayload, bool)> {
        self.lock()
            .as_ref()
            .map(|(at, p)| (p.clone(), at.elapsed() >= self.ttl))
    }

    /// 取映射：fresh 缓存 → sidecar 实拉（成功回填）。失败原样 Err，
    /// 由调用方（service 补全 pass）决定 stale 回落或跳过本轮股票补全。
    pub async fn get(&self, api: &dyn MarketApi) -> MarketResult<SectorMapPayload> {
        if let Some(p) = self.fresh() {
            return Ok(p);
        }
        let p = api.sector_map().await?;
        *self.lock() = Some((Instant::now(), p.clone()));
        Ok(p)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::MarketError;
    use async_trait::async_trait;
    use std::sync::atomic::{AtomicUsize, Ordering};

    struct FakeSector {
        result: Option<SectorMapPayload>,
        calls: AtomicUsize,
    }

    fn payload() -> SectorMapPayload {
        SectorMapPayload {
            as_of: "2026-11-15T10:00:00+08:00".into(),
            count: 1,
            source: Some("sina_sector".into()),
            degraded: false,
            attempts: vec![],
            sectors: [("600519".to_string(), "酿酒行业".to_string())]
                .into_iter()
                .collect(),
        }
    }

    #[async_trait]
    impl MarketApi for FakeSector {
        fn base_url(&self) -> &str {
            "fake://sector"
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
        ) -> MarketResult<crate::models::CalendarPayload> {
            unimplemented!()
        }
        async fn sector_map(&self) -> MarketResult<SectorMapPayload> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            match &self.result {
                Some(p) => Ok(p.clone()),
                None => Err(MarketError::Unavailable("sidecar 挂了".into())),
            }
        }
    }

    #[tokio::test]
    async fn caches_within_ttl() {
        let fake = FakeSector { result: Some(payload()), calls: AtomicUsize::new(0) };
        let c = SectorMapCache::new(Duration::from_secs(60));
        let a = c.get(&fake).await.unwrap();
        let b = c.get(&fake).await.unwrap();
        assert_eq!(a.sectors, b.sectors);
        assert_eq!(fake.calls.load(Ordering::SeqCst), 1); // 第二次命中缓存
    }

    #[tokio::test]
    async fn expired_cache_refetches() {
        let fake = FakeSector { result: Some(payload()), calls: AtomicUsize::new(0) };
        let c = SectorMapCache::new(Duration::ZERO); // 立即过期
        c.get(&fake).await.unwrap();
        c.get(&fake).await.unwrap();
        assert_eq!(fake.calls.load(Ordering::SeqCst), 2);
    }

    #[tokio::test]
    async fn failure_returns_err_stale_available() {
        let ok = FakeSector { result: Some(payload()), calls: AtomicUsize::new(0) };
        let c = SectorMapCache::new(Duration::ZERO); // 过期但 stale 可用
        c.get(&ok).await.unwrap();
        let down = FakeSector { result: None, calls: AtomicUsize::new(0) };
        assert!(c.get(&down).await.is_err());
        assert!(c.stale().is_some()); // 调用方据此回落旧映射
    }

    #[tokio::test]
    async fn no_cache_failure_err_and_no_stale() {
        let down = FakeSector { result: None, calls: AtomicUsize::new(0) };
        let c = SectorMapCache::default();
        assert!(c.get(&down).await.is_err());
        assert!(c.stale().is_none());
    }
}