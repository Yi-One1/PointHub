//! 平台适配层。
//!
//! 每个平台一个实现，对外只暴露 [`Provider`] 这一组动作：
//! 刷新令牌 → 查状态 → 领签到 → 查/推进附加活动。
//!
//! 协作约定：
//! - 适配器**不改**账号元数据，只读 `Account`、按需修改 `Credential`（令牌轮换）；
//! - 适配器**不落盘**，持久化交给引擎；
//! - 所有错误向上抛 [`ph_core::Error`]，由引擎决定是否重试。

pub mod client_crypto;
pub mod http;
pub mod identity;
pub mod local_import;
pub mod qoder;
pub mod trae;
pub mod workbuddy;

use async_trait::async_trait;
use ph_core::{
    Account, CheckinOutcome, Credential, Result, TravelSnapshot, Vendor, VendorSnapshot,
};

/// 派猫猫旅行的动作选择。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TravelAction {
    /// 只查状态，不动。
    Status,
    /// 完整闭环：能领就领，空闲就派。
    Auto,
    /// 只领取（已到达时）。
    ClaimOnly,
    /// 只派出（空闲时）。
    DepartOnly,
}

/// 平台能力声明。界面要据此决定显不显示按钮，
/// 不能靠 `if vendor == WorkBuddy` 到处散落判断。
#[derive(Debug, Clone, Copy)]
pub struct Capabilities {
    pub checkin: bool,
    pub travel: bool,
    pub balance: bool,
    pub token_refresh: bool,
    /// 该适配是否经过实机验证。
    pub verified: bool,
}

#[async_trait]
pub trait Provider: Send + Sync {
    fn vendor(&self) -> Vendor;

    fn capabilities(&self) -> Capabilities;

    /// 用长效令牌换新的接口令牌；若无长效令牌则什么都不做。
    /// 实现方负责把轮换后的新令牌写回 `cred`。
    async fn refresh(&self, cred: &mut Credential) -> Result<()>;

    /// 查积分余额与今日签到状态。
    async fn snapshot(&self, account: &Account, cred: &Credential) -> Result<VendorSnapshot>;

    /// 执行签到。
    async fn checkin(&self, account: &Account, cred: &Credential) -> Result<CheckinOutcome>;

    /// 附加活动（目前只有 WorkBuddy 的派猫猫旅行）。
    async fn travel(
        &self,
        _account: &Account,
        _cred: &Credential,
        _action: TravelAction,
        _location_id: Option<u32>,
    ) -> Result<TravelSnapshot> {
        Err(ph_core::Error::Unsupported(
            "该平台没有附加活动".to_string(),
        ))
    }
}

/// 按平台取适配器。返回 `Box<dyn Provider>`，调用方无需知道具体类型。
pub fn provider_for(vendor: Vendor) -> Box<dyn Provider> {
    match vendor {
        Vendor::WorkBuddy => Box::new(workbuddy::WorkBuddyProvider::new()),
        Vendor::TraeCn => Box::new(trae::TraeCnProvider::new()),
        Vendor::QoderCn => Box::new(qoder::QoderCnProvider::new()),
    }
}

/// 全部平台的适配器清单（用于「能力矩阵」展示与自检）。
pub fn all_capabilities() -> Vec<(Vendor, Capabilities)> {
    Vendor::ALL
        .iter()
        .map(|v| (*v, provider_for(*v).capabilities()))
        .collect()
}
