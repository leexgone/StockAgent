//! 费用估算（03 §3）：佣金（万 2.5、最低 5 元）、印花税（卖出且股票）、过户费（股票，沪深均收）。
//! 输出保留原始精度不做分位舍入（03 附录约定 5）；界面必须始终带「实际以券商结算为准」。

use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use crate::{Kind, Side};

pub const COMMISSION_RATE: f64 = 0.00025;
pub const MIN_COMMISSION: f64 = 5.0;
pub const STAMP_TAX_RATE: f64 = 0.0005;
pub const TRANSFER_FEE_RATE: f64 = 0.00001;

#[derive(Debug, Clone, Copy, Deserialize)]
pub struct FeeInput {
    pub kind: Kind,
    pub side: Side,
    pub qty: f64,
    pub price: f64,
}

#[derive(Debug, Clone, Copy, Serialize, ToSchema)]
pub struct FeeBreakdown {
    pub amount: f64,
    pub commission: f64,
    pub stamp_tax: f64,
    pub transfer_fee: f64,
    pub total: f64,
}

/// 费用估算（原型 `holdings.html:320`）。ETF 与转债免印花税、免过户费；
/// 转债「转股」的费用税务不同，范围外（03 §3 ⚠️ 留扩展位）。
pub fn fee(input: FeeInput) -> FeeBreakdown {
    let amount = input.qty * input.price;
    let commission = MIN_COMMISSION.max(amount * COMMISSION_RATE);
    let stamp_tax = if input.side == Side::Sell && input.kind == Kind::Stock {
        amount * STAMP_TAX_RATE
    } else {
        0.0
    };
    let transfer_fee = if input.kind == Kind::Stock {
        amount * TRANSFER_FEE_RATE
    } else {
        0.0
    };
    // 累加顺序与原型一致：((佣金 + 印花) + 过户)
    let total = commission + stamp_tax + transfer_fee;
    FeeBreakdown {
        amount,
        commission,
        stamp_tax,
        transfer_fee,
        total,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn min_commission_floor() {
        let f = fee(FeeInput {
            kind: Kind::Stock,
            side: Side::Buy,
            qty: 100.0,
            price: 25.0,
        });
        assert_eq!(f.commission, 5.0);
        assert!((f.total - 5.025).abs() < 1e-9);
    }
}
