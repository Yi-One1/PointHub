use crate::schedule::AccountSchedule;
use serde::{Deserialize, Serialize};
use std::fmt;

/// 支持的 AI Agent 平台。
///
/// 序列化用稳定的短 id（`workbuddy` / `trae-cn` / `qoder-cn`），
/// 落盘后不要改名，否则历史数据无法反序列化。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Vendor {
    #[serde(rename = "workbuddy")]
    WorkBuddy,
    #[serde(rename = "trae-cn")]
    TraeCn,
    #[serde(rename = "qoder-cn")]
    QoderCn,
}

impl Vendor {
    pub const ALL: [Vendor; 3] = [Vendor::WorkBuddy, Vendor::TraeCn, Vendor::QoderCn];

    /// 落盘 id，稳定不可变。
    pub fn id(self) -> &'static str {
        match self {
            Vendor::WorkBuddy => "workbuddy",
            Vendor::TraeCn => "trae-cn",
            Vendor::QoderCn => "qoder-cn",
        }
    }

    /// 界面显示名。
    pub fn display(self) -> &'static str {
        match self {
            Vendor::WorkBuddy => "WorkBuddy",
            Vendor::TraeCn => "Trae CN",
            Vendor::QoderCn => "Qoder CN",
        }
    }

    /// 该平台的签到刷新时刻（本地时间，时, 分）。
    ///
    /// 依据：WorkBuddy 与 Trae 的每日额度在 00:00 重置；
    /// Qoder 的每日 100 Credits 于每天 10:00 刷新。
    /// 默认值故意留 10 分钟余量，避免踩在刷新瞬间。
    pub fn default_checkin_time(self) -> (u8, u8) {
        match self {
            Vendor::WorkBuddy => (0, 10),
            Vendor::TraeCn => (0, 10),
            Vendor::QoderCn => (10, 10),
        }
    }

    /// 该平台是否具备「派猫猫旅行」这类附加活动。
    pub fn has_travel(self) -> bool {
        matches!(self, Vendor::WorkBuddy)
    }

    pub fn parse(s: &str) -> Option<Vendor> {
        let key = s.trim().to_ascii_lowercase().replace('_', "-");
        match key.as_str() {
            "workbuddy" | "work-buddy" | "wb" | "codebuddy" => Some(Vendor::WorkBuddy),
            "trae-cn" | "trae" | "traecn" => Some(Vendor::TraeCn),
            "qoder-cn" | "qoder" | "qodercn" => Some(Vendor::QoderCn),
            _ => None,
        }
    }
}

impl fmt::Display for Vendor {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.display())
    }
}

/// 一个被纳管的账号。
///
/// 注意：这里**不含任何令牌**。令牌只在 `ph-vault` 的加密仓里，
/// 且永远不随 `Account` 一起序列化到明文文件，也不进日志。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Account {
    pub id: String,
    pub vendor: Vendor,
    /// 用户自己起的名字，例如「主号」「小号A」。
    pub label: String,
    /// 脱敏后的账号标识，仅用于人眼区分，例如 `eyJhbG...9f2c` 或邮箱。
    pub hint: String,
    /// 总开关：false 时既不签到也不查询。
    pub enabled: bool,
    pub checkin_enabled: bool,
    /// 仅 WorkBuddy 有效（派猫猫旅行）。
    pub travel_enabled: bool,
    pub schedule: AccountSchedule,
    pub created_at: i64,
    pub updated_at: i64,
    pub last_checkin_at: Option<i64>,
    pub last_error: Option<String>,
    /// 平台侧稳定用户 id（能拿到就填），用于双端认出「同一个账号」。
    #[serde(default)]
    pub platform_uid: Option<String>,
    /// 唯一执行端设备 id。凭据只存在于执行端，签到也只由它执行。
    /// 老数据没有这个字段，读取后会由引擎补成本机 id。
    #[serde(default)]
    pub executor: Option<String>,
    /// 首次导入该账号的设备 id，仅用于展示来源。
    #[serde(default)]
    pub origin_device: Option<String>,
    /// 失败后允许自动重试的最早时间（epoch 秒）。None 表示不限制。
    ///
    /// 签到失败时写入（普通失败 30 分钟、凭据失效到次日 0 点），成功或
    /// 明确「无资格」时清除。手动签到不读这个字段。
    #[serde(default)]
    pub retry_after: Option<i64>,
    /// 最近一次失败通知所属的本地日期（当天 0 点的 epoch 秒）。
    ///
    /// 同一账号同一天只发一条失败系统通知，避免持续失败把通知栏刷屏。
    #[serde(default)]
    pub last_failure_notice_at: Option<i64>,
}

impl Account {
    pub fn new(vendor: Vendor, label: impl Into<String>, hint: impl Into<String>) -> Self {
        let (hour, minute) = vendor.default_checkin_time();
        let now = chrono::Local::now().timestamp();
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            vendor,
            label: label.into(),
            hint: hint.into(),
            enabled: true,
            checkin_enabled: true,
            travel_enabled: vendor.has_travel(),
            schedule: AccountSchedule::new(hour, minute),
            created_at: now,
            updated_at: now,
            last_checkin_at: None,
            last_error: None,
            platform_uid: None,
            executor: None,
            origin_device: None,
            retry_after: None,
            last_failure_notice_at: None,
        }
    }
}

/// 一个账号在某平台的凭据。
///
/// 全部字段都是敏感信息，只能存在于 `ph-vault` 的加密容器里。
/// `Debug` 故意手写成脱敏版本，防止 `{:?}` 把令牌打进日志。
#[derive(Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct Credential {
    /// 短期接口令牌（Bearer）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub access_token: Option<String>,
    /// 长效令牌（refresh token），等于账号钥匙，优先使用它自续期。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub refresh_token: Option<String>,
    /// 设备标识（Trae 的 x-device-id 等）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub device_id: Option<String>,
    /// Cookie 形态的凭据（部分平台走网页会话）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cookie: Option<String>,
    /// 鉴权域名（WorkBuddy 以登录态里的 domain 为准，不可硬编码）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub domain: Option<String>,
    /// 会话绑定的 User-Agent，部分网关会校验，必须原样回放。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub user_agent: Option<String>,
    /// access_token 过期时间（epoch 秒）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expires_at: Option<i64>,
    /// 平台侧用户 id：令牌不是 JWT 时（Qoder / Trae 浏览器授权）用它去重。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub platform_uid: Option<String>,
    /// 设备机器码（Qoder 风控头、Trae 刷新签名体里要用）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub machine_id: Option<String>,
    /// 设备私钥（PEM）：Trae 浏览器授权的令牌绑定这把钥匙，刷新时要签名字。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub device_key: Option<String>,
    /// Qoder 设备续期的 refresh_token_id（部分响应会下发）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub refresh_token_id: Option<String>,
}

impl fmt::Debug for Credential {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fn mask(v: &Option<String>) -> String {
            match v {
                None => "None".into(),
                Some(s) if s.chars().count() <= 10 => "Some(***)".into(),
                Some(s) => {
                    let head: String = s.chars().take(6).collect();
                    let tail: String = s.chars().rev().take(4).collect::<String>().chars().rev().collect();
                    format!("Some({head}...{tail})")
                }
            }
        }
        f.debug_struct("Credential")
            .field("access_token", &mask(&self.access_token))
            .field("refresh_token", &mask(&self.refresh_token))
            .field("device_id", &mask(&self.device_id))
            .field("cookie", &mask(&self.cookie))
            .field("domain", &self.domain)
            .field("user_agent", &self.user_agent)
            .field("expires_at", &self.expires_at)
            .field("platform_uid", &self.platform_uid)
            .field("machine_id", &mask(&self.machine_id))
            .field("device_key", &mask(&self.device_key))
            .field("refresh_token_id", &mask(&self.refresh_token_id))
            .finish()
    }
}

impl Credential {
    pub fn from_refresh_token(token: impl Into<String>) -> Self {
        Self {
            refresh_token: Some(token.into().trim().to_string()),
            ..Default::default()
        }
    }

    pub fn from_access_token(token: impl Into<String>) -> Self {
        Self {
            access_token: Some(token.into().trim().to_string()),
            ..Default::default()
        }
    }

    /// 生成给人看的脱敏标识。
    pub fn hint(&self) -> String {
        let raw = self
            .refresh_token
            .as_deref()
            .or(self.access_token.as_deref())
            .unwrap_or("");
        if raw.is_empty() {
            return "未知".into();
        }
        let chars: Vec<char> = raw.chars().collect();
        if chars.len() <= 12 {
            return "***".into();
        }
        let head: String = chars.iter().take(6).collect();
        let tail: String = chars.iter().skip(chars.len() - 4).collect();
        format!("{head}...{tail}")
    }
}

/// 派猫猫旅行的状态机。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TravelState {
    /// 空闲，可以派遣。
    Idle,
    /// 旅行中，等待到达。
    Traveling,
    /// 已到达，等待领取奖励。
    Arrived,
    /// 服务端给了不认识的值，或账号未开通该活动。
    Unknown,
}

impl TravelState {
    pub fn parse(s: &str) -> TravelState {
        match s.trim().to_ascii_lowercase().as_str() {
            "idle" => TravelState::Idle,
            "traveling" => TravelState::Traveling,
            "arrived" => TravelState::Arrived,
            _ => TravelState::Unknown,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            TravelState::Idle => "空闲",
            TravelState::Traveling => "旅行中",
            TravelState::Arrived => "已到达（可领取）",
            TravelState::Unknown => "未开通/未知",
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TravelSnapshot {
    pub state: TravelState,
    #[serde(default)]
    pub location: Option<String>,
    /// 到达时间（epoch 秒）。
    #[serde(default)]
    pub arrive_at: Option<i64>,
    #[serde(default)]
    pub reward_credit: Option<i64>,
    #[serde(default)]
    pub daily_limit_reached: bool,
    /// 本次动作的说明，例如「已派出」「已补领 7 分」。
    #[serde(default)]
    pub note: Option<String>,
}

impl Default for TravelState {
    fn default() -> Self {
        TravelState::Unknown
    }
}

/// 某账号某时刻的积分快照。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VendorSnapshot {
    pub account_id: String,
    pub vendor: Vendor,
    /// 采样时间（epoch 秒）。
    pub captured_at: i64,
    /// 积分余额；拿不到就是 None，**不要填 0**，0 和「未知」是两件事。
    #[serde(default)]
    pub balance: Option<f64>,
    #[serde(default)]
    pub checked_in_today: Option<bool>,
    #[serde(default)]
    pub streak_days: Option<i64>,
    #[serde(default)]
    pub daily_credit: Option<i64>,
    /// 今日新得积分，由引擎按前后两次余额差推算。
    #[serde(default)]
    pub today_gain: Option<f64>,
    #[serde(default)]
    pub travel: Option<TravelSnapshot>,
    /// 平台返回的原话或异常说明，便于排查。
    #[serde(default)]
    pub note: Option<String>,
}

impl VendorSnapshot {
    pub fn empty(account: &Account, at: i64) -> Self {
        Self {
            account_id: account.id.clone(),
            vendor: account.vendor,
            captured_at: at,
            balance: None,
            checked_in_today: None,
            streak_days: None,
            daily_credit: None,
            today_gain: None,
            travel: None,
            note: None,
        }
    }
}

/// 签到结果的状态分类。
///
/// 单独区分 `NotEligible`（账号没有活动资格）与 `Failed`（请求失败），
/// 因为二者的处理方式完全不同：前者不该重试，后者要重试。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CheckinStatus {
    /// 本次领到了。
    Claimed,
    /// 今天已经领过（幂等，不是错误）。
    AlreadyDone,
    /// 账号不参与该活动（例如 Qoder 免费号）。
    NotEligible,
    /// 凭据失效，需要重新导入。
    AuthFailed,
    /// 网络或服务端异常。
    Failed,
}

impl CheckinStatus {
    pub fn is_ok(self) -> bool {
        matches!(
            self,
            CheckinStatus::Claimed | CheckinStatus::AlreadyDone | CheckinStatus::NotEligible
        )
    }

    pub fn label(self) -> &'static str {
        match self {
            CheckinStatus::Claimed => "已领取",
            CheckinStatus::AlreadyDone => "今天已签",
            CheckinStatus::NotEligible => "无活动资格",
            CheckinStatus::AuthFailed => "凭据失效",
            CheckinStatus::Failed => "失败",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CheckinOutcome {
    pub account_id: String,
    pub vendor: Vendor,
    pub status: CheckinStatus,
    #[serde(default)]
    pub credit: Option<i64>,
    #[serde(default)]
    pub streak_days: Option<i64>,
    pub message: String,
    pub at: i64,
}

/// 一条用户可见的活动记录。
///
/// 只记「发生了什么事」，不存令牌、也不存上游原始响应体——
/// 这个文件是明文，任何敏感内容都不该进来。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActivityEvent {
    /// 发生时间（epoch 秒）。
    pub at: i64,
    pub account_id: String,
    /// 记录当时的账号别名，便于账号被删后仍能读懂历史。
    pub label: String,
    pub vendor: Vendor,
    /// `checkin` | `travel`
    pub kind: String,
    /// 本次影响的积分（领取为正，失败为空）。
    #[serde(default)]
    pub delta: Option<i64>,
    /// 人话描述，例如「已领取」「派猫猫 已到达」。
    pub text: String,
}

impl ActivityEvent {
    pub fn checkin(account: &Account, at: i64, text: impl Into<String>, delta: Option<i64>) -> Self {
        Self {
            at,
            account_id: account.id.clone(),
            label: account.label.clone(),
            vendor: account.vendor,
            kind: "checkin".into(),
            delta,
            text: text.into(),
        }
    }

    pub fn travel(account: &Account, at: i64, text: impl Into<String>, delta: Option<i64>) -> Self {
        Self {
            at,
            account_id: account.id.clone(),
            label: account.label.clone(),
            vendor: account.vendor,
            kind: "travel".into(),
            delta,
            text: text.into(),
        }
    }
}

/// 双端同步协议的版本号。
///
/// 载荷结构一旦不兼容就加一，对端看到不认识的版本必须整体拒绝，
/// 而不是「尽量解析」——半解析的同步结果比不同步更糟。
pub const SYNC_PROTOCOL: u32 = 1;

/// 一台参与同步的设备。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeviceInfo {
    /// 设备本地生成的稳定 id（uuid）。
    pub id: String,
    /// 显示名，例如「书房电脑」「Pixel 8」。
    pub name: String,
    /// `pc` | `android`。
    pub kind: String,
    /// 配对令牌的 SHA-256（hex）。本机自己的记录不带令牌。
    #[serde(default)]
    pub token_hash: Option<String>,
    pub paired_at: i64,
    #[serde(default)]
    pub last_seen_at: Option<i64>,
}

impl DeviceInfo {
    pub fn new(name: impl Into<String>, kind: impl Into<String>) -> Self {
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            name: name.into(),
            kind: kind.into(),
            token_hash: None,
            paired_at: chrono::Local::now().timestamp(),
            last_seen_at: None,
        }
    }

    /// 令牌哈希：落盘只存它，令牌原文只在配对时返回一次。
    pub fn hash_token(token: &str) -> String {
        use sha2::{Digest, Sha256};
        let mut h = Sha256::new();
        h.update(token.as_bytes());
        h.finalize().iter().map(|b| format!("{b:02x}")).collect()
    }
}

/// 删除标记：防止已删除的账号被对端同步回生。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Tombstone {
    pub account_id: String,
    pub deleted_at: i64,
    /// 执行删除的设备 id。
    pub device_id: String,
}

/// 双端同步载荷。
///
/// **硬约束：这里没有、也不允许出现凭据字段。**
/// 凭据（令牌 / cookie / 设备标识）只留在执行端的加密仓里，
/// 同步只搬「账号是什么、积分多少、发生过什么」。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncPayload {
    pub protocol: u32,
    pub from_device: String,
    pub from_name: String,
    #[serde(default = "default_kind")]
    pub from_kind: String,
    pub sent_at: i64,
    /// 增量起点：只带此时间之后的快照/活动；0 表示全量。
    #[serde(default)]
    pub since: i64,
    #[serde(default)]
    pub accounts: Vec<Account>,
    #[serde(default)]
    pub snapshots: Vec<VendorSnapshot>,
    #[serde(default)]
    pub events: Vec<ActivityEvent>,
    #[serde(default)]
    pub tombstones: Vec<Tombstone>,
    /// 执行端本机持有凭据的账号 id。只用于对端展示「能不能签」，不含凭据本身。
    #[serde(default)]
    pub credential_ids: Vec<String>,
    /// 对端的下一次自动签到时刻（epoch 秒）。
    #[serde(default)]
    pub next_run_at: Option<i64>,
}

fn default_kind() -> String {
    "pc".into()
}

impl SyncPayload {
    pub fn new(device: &DeviceInfo, since: i64) -> Self {
        Self {
            protocol: SYNC_PROTOCOL,
            from_device: device.id.clone(),
            from_name: device.name.clone(),
            from_kind: device.kind.clone(),
            sent_at: chrono::Local::now().timestamp(),
            since,
            accounts: Vec::new(),
            snapshots: Vec::new(),
            events: Vec::new(),
            tombstones: Vec::new(),
            credential_ids: Vec::new(),
            next_run_at: None,
        }
    }
}

/// 一次合并的结果，直接展示给用户。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct MergeReport {
    pub accounts_added: usize,
    pub accounts_updated: usize,
    pub accounts_removed: usize,
    pub snapshots_added: usize,
    pub events_added: usize,
    /// 无法自动合并、需要用户手动处理的说明。
    #[serde(default)]
    pub conflicts: Vec<String>,
}

impl MergeReport {
    pub fn is_noop(&self) -> bool {
        self.accounts_added == 0
            && self.accounts_updated == 0
            && self.accounts_removed == 0
            && self.snapshots_added == 0
            && self.events_added == 0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vendor_roundtrip() {
        for v in Vendor::ALL {
            let json = serde_json::to_string(&v).unwrap();
            // 落盘值必须与 id() 完全一致，否则「文档写的 id」和「文件里的值」会分叉。
            assert_eq!(json, format!("\"{}\"", v.id()));
            let back: Vendor = serde_json::from_str(&json).unwrap();
            assert_eq!(v, back);
            assert_eq!(Vendor::parse(v.id()), Some(v));
        }
    }

    #[test]
    fn credential_debug_masks_secrets() {
        let c = Credential::from_refresh_token("abcdefghijklmnopqrstuvwxyz");
        let s = format!("{c:?}");
        assert!(!s.contains("abcdefghijklmnopqrstuvwxyz"));
        assert!(s.contains("abcdef"));
    }

    #[test]
    fn default_time_follows_refresh_window() {
        assert_eq!(Vendor::QoderCn.default_checkin_time(), (10, 10));
        assert_eq!(Vendor::WorkBuddy.default_checkin_time(), (0, 10));
    }
}
