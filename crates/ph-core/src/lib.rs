//! PointHub 核心领域模型。
//!
//! 这一层只放「是什么」，不放「怎么做」：
//! 平台枚举、账号、凭据、快照、签到结果、调度规则、错误类型。
//! 任何网络请求、文件读写、加密都不得出现在这里。

pub mod error;
pub mod model;
pub mod schedule;
pub mod settings;

pub use error::{Error, Result};
pub use model::{
    Account, CheckinOutcome, CheckinStatus, Credential, TravelSnapshot, TravelState, Vendor,
    VendorSnapshot,
};
pub use schedule::AccountSchedule;
pub use settings::Settings;

/// 当前程序版本，取自 Cargo.toml，避免手写漂移。
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// 数据目录下的固定文件名，集中声明，避免各处硬编码漂移。
pub mod files {
    pub const ACCOUNTS: &str = "accounts.json";
    pub const SETTINGS: &str = "settings.json";
    pub const VAULT: &str = "vault.bin";
    pub const HISTORY: &str = "history.jsonl";
    pub const LOGS_DIR: &str = "logs";
}
