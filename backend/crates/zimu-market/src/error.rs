//! 适配器错误三分类（docs/04 附录 B「适配器边界」）。
//!
//! 分类就是降级策略的路由：
//! - `Unavailable` → sidecar 进程不可达：主程序整体降级（陈旧持仓数据照常、
//!   行情端点 503、时钟退工作日启发式），**不是**上游数据源的问题。
//! - `Upstream` → sidecar 活着但其上游全链失败/参数被拒：原样透传状态码与
//!   错误契约（如 502 `UPSTREAM_ALL_FAILED`、400 `BAD_SYMBOL`）。
//! - `Contract` → 响应与契约不符（反序列化失败）：两边版本漂移，按 502 处理并
//!   记日志——这属于要修代码的 bug，不是运行期降级。

use std::fmt;

#[derive(Debug, Clone)]
pub enum MarketError {
    /// sidecar 不可达（连接拒绝/超时/URL 非法）
    Unavailable(String),
    /// sidecar 返回非 2xx，携带其错误契约
    Upstream {
        status: u16,
        code: String,
        message: String,
    },
    /// 响应体与契约不符（反序列化失败/缺关键字段）
    Contract(String),
}

pub type MarketResult<T> = Result<T, MarketError>;

impl fmt::Display for MarketError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            MarketError::Unavailable(m) => write!(f, "行情 sidecar 不可达：{m}"),
            MarketError::Upstream {
                status,
                code,
                message,
            } => write!(f, "行情上游错误（HTTP {status} {code}）：{message}"),
            MarketError::Contract(m) => write!(f, "行情契约不符：{m}"),
        }
    }
}

impl std::error::Error for MarketError {}
