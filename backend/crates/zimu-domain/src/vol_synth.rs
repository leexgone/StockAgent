//! 波动合成假设（03 §8）：ρ 为写死假设值（同标的 1 / 同行业 0.62 / 跨行业 0.35），
//! 只用于「调整前后谁更大」的相对比较，不是绝对风险预测。
//! C7 拍板改估计值后，本模块与 `fixtures/golden/vol_synth.json` 要一起改。

use serde::{Deserialize, Serialize};

use crate::TRADING_DAYS;

pub const RHO_SAME: f64 = 0.62;
pub const RHO_CROSS: f64 = 0.35;

#[derive(Debug, Clone, Deserialize)]
pub struct VolItem {
    pub code: String,
    pub sector: String,
    /// 占市值权重（分数）；Σw 不强制为 1——再平衡「调整后」场景权重和小于 1
    pub w: f64,
    /// 日收益标准差（分数，近 30 日）
    pub sigma: f64,
}

#[derive(Debug, Clone, Deserialize)]
pub struct VolSynthInput {
    pub items: Vec<VolItem>,
}

#[derive(Debug, Serialize)]
pub struct VolSynthOutput {
    /// 组合年化波动 %（与原型 diagnosis.html portVol 一致，×100 输出百分数）
    pub ann_vol: f64,
}

/// √(ΣᵢΣⱼ wᵢwⱼσᵢσⱼρᵢⱼ) × √244 × 100（原型 `diagnosis.html:137` portVol，累加顺序一致）。
pub fn vol_synth(inp: &VolSynthInput) -> VolSynthOutput {
    let mut v = 0.0;
    for a in &inp.items {
        for b in &inp.items {
            let rho = if a.code == b.code {
                1.0
            } else if a.sector == b.sector {
                RHO_SAME
            } else {
                RHO_CROSS
            };
            v += a.w * b.w * a.sigma * b.sigma * rho;
        }
    }
    VolSynthOutput {
        ann_vol: v.max(0.0).sqrt() * TRADING_DAYS.sqrt() * 100.0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_items_zero_vol() {
        let out = vol_synth(&VolSynthInput { items: vec![] });
        assert_eq!(out.ann_vol, 0.0);
    }
}
