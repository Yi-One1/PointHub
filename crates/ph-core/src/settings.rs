use crate::model::Vendor;
use serde::{Deserialize, Serialize};

/// 运行模式。
///
/// 桌面端固定按 `Local` 处理；安卓端可选「本机独立账号」或「连接电脑」。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AppMode {
    /// 账号与凭据都在本机，签到由本机执行。
    Local,
    /// 只做电脑的遥控器：账号、凭据、签到都在电脑上。
    Remote,
}

impl Default for AppMode {
    fn default() -> Self {
        AppMode::Local
    }
}

/// 界面主题。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ThemeMode {
    Light,
    Dark,
    /// 跟随系统。
    System,
}

impl Default for ThemeMode {
    fn default() -> Self {
        ThemeMode::System
    }
}

/// 悬浮球设置。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BallSettings {
    pub enabled: bool,
    /// 屏幕坐标；None 时由程序放到右上角。
    #[serde(default)]
    pub x: Option<i32>,
    #[serde(default)]
    pub y: Option<i32>,
    /// 直径（逻辑像素）。
    #[serde(default = "default_ball_size")]
    pub size: u32,
    /// 不透明度 0.3~1.0。
    #[serde(default = "default_opacity")]
    pub opacity: f32,
    /// 球面显示的内容：总积分 / 今日待签数。
    #[serde(default)]
    pub show: BallContent,
    /// 球面绑定哪个账号；None 表示「全部合计」。
    ///
    /// 用账号 id 而不是别名：别名可改，id 不会漂。
    #[serde(default)]
    pub selected: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BallContent {
    TotalCredits,
    PendingCount,
}

impl Default for BallContent {
    fn default() -> Self {
        BallContent::TotalCredits
    }
}

fn default_ball_size() -> u32 {
    72
}
fn default_opacity() -> f32 {
    0.92
}

impl Default for BallSettings {
    fn default() -> Self {
        Self {
            enabled: true,
            x: None,
            y: None,
            size: default_ball_size(),
            opacity: default_opacity(),
            show: BallContent::TotalCredits,
            selected: None,
        }
    }
}

/// 局域网互通设置。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncSettings {
    pub enabled: bool,
    /// 监听端口，0 表示由系统分配。
    #[serde(default = "default_port")]
    pub port: u16,
    /// 是否允许手机端触发签到（关闭后手机只能看）。
    #[serde(default = "default_true")]
    pub allow_remote_checkin: bool,
}

fn default_port() -> u16 {
    47821
}
fn default_true() -> bool {
    true
}

fn default_auto_refresh_minutes() -> u32 {
    10
}

impl Default for SyncSettings {
    fn default() -> Self {
        Self {
            enabled: true,
            port: default_port(),
            allow_remote_checkin: true,
        }
    }
}

/// 更新来源。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum UpdateSource {
    /// 直连 GitHub Releases。
    Github,
    /// 走镜像前缀（拼在 GitHub 地址前面）。
    Mirror,
}

impl Default for UpdateSource {
    fn default() -> Self {
        UpdateSource::Github
    }
}

/// 自动更新设置。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateSettings {
    /// 启动时自动检查（只提示，不自动装）。
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default)]
    pub source: UpdateSource,
    /// 镜像前缀，例如 `https://ghfast.top/`；会拼在完整 GitHub 地址前面。
    #[serde(default = "default_mirror")]
    pub mirror: String,
}

/// 默认镜像前缀。用户切到「镜像」时直接可用，不必先填地址。
pub fn default_mirror() -> String {
    "https://ghfast.top/".to_string()
}

impl Default for UpdateSettings {
    fn default() -> Self {
        Self {
            enabled: true,
            source: UpdateSource::Github,
            mirror: default_mirror(),
        }
    }
}

/// 错峰窗口的上限（分钟）。
///
/// 窗口只是「同平台多账号互相避开」用的，超过半小时就没有意义了；
/// 老设置里写进更大的值也按这个上限收敛，避免某个账号被推到下一个整点。
pub const MAX_SPREAD_MINUTES: u8 = 30;

/// 一个平台的自动签到时间设置。
///
/// 账号自身不再保存可编辑的时间：时间 = 平台基准 + 同平台序号错峰。
/// 用户改这里，该平台所有账号一起重算（见 `ph-engine::Engine::reconcile_schedules`）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PlatformSchedule {
    pub vendor: Vendor,
    pub hour: u8,
    pub minute: u8,
    /// 同平台多账号错开的总窗口（分钟）。0 = 不错开、不浮动。
    #[serde(default = "default_spread")]
    pub spread_minutes: u8,
}

fn default_spread() -> u8 {
    5
}

impl PlatformSchedule {
    /// 平台默认值：时刻取平台的额度刷新时间，窗口给 5 分钟。
    pub fn default_for(v: Vendor) -> Self {
        let (hour, minute) = v.default_checkin_time();
        Self {
            vendor: v,
            hour,
            minute,
            spread_minutes: default_spread(),
        }
    }

    pub fn hhmm(&self) -> String {
        format!("{:02}:{:02}", self.hour, self.minute)
    }
}

/// 全局设置。整体落盘为 `settings.json`。
///
/// 注意：数据目录本身不在这里——它由 `ph-store` 的定位逻辑决定，
/// 否则会出现「改目录后设置文件找不到」的循环依赖。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Settings {
    /// 本机模式 / 远程模式（安卓端用；桌面端忽略）。
    #[serde(default)]
    pub mode: AppMode,
    #[serde(default)]
    pub theme: ThemeMode,
    /// 开机自启。
    #[serde(default)]
    pub autostart: bool,
    /// 关闭窗口时最小化到托盘，而不是退出。
    #[serde(default = "default_true")]
    pub minimize_to_tray: bool,
    /// 签到完成后发系统通知。
    #[serde(default = "default_true")]
    pub notify_on_checkin: bool,
    /// 失败时也发通知。
    #[serde(default = "default_true")]
    pub notify_on_failure: bool,
    #[serde(default)]
    pub ball: BallSettings,
    #[serde(default)]
    pub sync: SyncSettings,
    /// 全局调度开关。
    #[serde(default = "default_true")]
    pub scheduler_enabled: bool,
    /// 错过当天窗口后，启动时是否补签。
    #[serde(default = "default_true")]
    pub catch_up_missed: bool,
    /// 主窗口可见时每隔多少分钟自动采样一次积分；0 表示关闭（D18）。
    #[serde(default = "default_auto_refresh_minutes")]
    pub auto_refresh_minutes: u32,
    /// 自动更新。
    #[serde(default)]
    pub update: UpdateSettings,
    /// 各平台的自动签到时间。老 `settings.json` 没有这一段，
    /// 读取后由 `ensure_platform_schedules()` 补齐。
    #[serde(default)]
    pub platform_schedules: Vec<PlatformSchedule>,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            mode: AppMode::default(),
            theme: ThemeMode::default(),
            autostart: false,
            minimize_to_tray: true,
            notify_on_checkin: true,
            notify_on_failure: true,
            ball: BallSettings::default(),
            sync: SyncSettings::default(),
            scheduler_enabled: true,
            catch_up_missed: true,
            auto_refresh_minutes: 10,
            update: UpdateSettings::default(),
            platform_schedules: Vendor::ALL
                .iter()
                .map(|v| PlatformSchedule::default_for(*v))
                .collect(),
        }
    }
}

impl Settings {
    /// 为 `Vendor::ALL` 中缺失的平台补默认值，并去掉重复项。
    ///
    /// 归一化成「按 `Vendor::ALL` 顺序、每平台一条」：界面照这个顺序渲染，
    /// 用户不用面对一个乱序或带重复行的列表。返回是否有改动（调用方据此回写）。
    pub fn ensure_platform_schedules(&mut self) -> bool {
        let mut next: Vec<PlatformSchedule> = Vec::with_capacity(Vendor::ALL.len());
        for v in Vendor::ALL {
            // 同平台只留第一条：后来的重复行是历史数据，不该在界面上叠出来。
            let mut found = self
                .platform_schedules
                .iter()
                .find(|p| p.vendor == v)
                .cloned()
                .unwrap_or_else(|| PlatformSchedule::default_for(v));
            // 超限值在归一化时就收敛掉：否则界面拿到 99 显示在 max=30 的框里，
            // 用户既改不动也想不通，而账号实际时间又是按 30 算的。
            if found.spread_minutes > MAX_SPREAD_MINUTES {
                found.spread_minutes = MAX_SPREAD_MINUTES;
            }
            next.push(found);
        }
        if next == self.platform_schedules {
            return false;
        }
        self.platform_schedules = next;
        true
    }

    /// 取某平台的签到时间设置；缺失时回落到平台默认值。
    ///
    /// 窗口在这里也收敛到 30 分钟：即使调用方没先跑 `ensure_platform_schedules()`
    /// 也不会拿到超限的值；`ensure` 会把超限的值就地改掉并落盘。
    pub fn platform_schedule(&self, v: Vendor) -> PlatformSchedule {
        let mut p = self
            .platform_schedules
            .iter()
            .find(|p| p.vendor == v)
            .cloned()
            .unwrap_or_else(|| PlatformSchedule::default_for(v));
        if p.spread_minutes > MAX_SPREAD_MINUTES {
            p.spread_minutes = MAX_SPREAD_MINUTES;
        }
        p
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settings_tolerate_partial_json() {
        // 老版本 settings.json 缺字段时不能直接崩，必须能补默认值。
        let s: Settings = serde_json::from_str(r#"{"theme":"dark"}"#).unwrap();
        assert_eq!(s.theme, ThemeMode::Dark);
        assert!(s.ball.enabled);
        assert_eq!(s.sync.port, 47821);
        // 老设置里没有 update 段，也要能补出默认值
        assert!(s.update.enabled);
        assert_eq!(s.update.source, UpdateSource::Github);
        assert_eq!(s.update.mirror, "https://ghfast.top/");
    }

    #[test]
    fn default_update_mirror_is_ready_to_use() {
        assert_eq!(UpdateSettings::default().mirror, "https://ghfast.top/");
        assert_eq!(Settings::default().update.mirror, "https://ghfast.top/");
    }

    #[test]
    fn old_settings_json_gets_platform_schedules_backfilled() {
        // 老版本 settings.json 完全没有 platform_schedules 段。
        let mut s: Settings = serde_json::from_str(r#"{"theme":"dark"}"#).unwrap();
        assert!(s.platform_schedules.is_empty(), "缺字段时应先按空表读入");
        assert!(s.ensure_platform_schedules(), "补齐三个平台应报告有改动");
        assert_eq!(s.platform_schedules.len(), 3);
        assert_eq!(
            s.platform_schedules.iter().map(|p| p.vendor).collect::<Vec<_>>(),
            Vendor::ALL.to_vec(),
            "顺序固定为 Vendor::ALL，界面不用自己排"
        );
        // Qoder 是 10:10 刷新，窗口默认 5 分钟。
        let q = s.platform_schedule(Vendor::QoderCn);
        assert_eq!((q.hour, q.minute), (10, 10));
        assert_eq!(q.spread_minutes, 5);
        assert_eq!(q.hhmm(), "10:10");
        // 已经补齐后再调一次不应再报改动。
        assert!(!s.ensure_platform_schedules());
    }

    #[test]
    fn ensure_platform_schedules_drops_duplicates() {
        let mut s = Settings::default();
        // 模拟历史数据：重复的 WorkBuddy + 只保留用户改过的 Qoder。
        s.platform_schedules = vec![
            PlatformSchedule {
                vendor: Vendor::QoderCn,
                hour: 9,
                minute: 45,
                spread_minutes: 8,
            },
            PlatformSchedule::default_for(Vendor::WorkBuddy),
            PlatformSchedule {
                vendor: Vendor::WorkBuddy,
                hour: 3,
                minute: 3,
                spread_minutes: 3,
            },
        ];
        assert!(s.ensure_platform_schedules());
        assert_eq!(s.platform_schedules.len(), 3, "每平台只留一条");
        // 用户改过的值要保住。
        let q = s.platform_schedule(Vendor::QoderCn);
        assert_eq!((q.hour, q.minute, q.spread_minutes), (9, 45, 8));
        // 重复的 WorkBuddy 保留第一条。
        let wb = s.platform_schedule(Vendor::WorkBuddy);
        assert_eq!((wb.hour, wb.minute), (0, 10));
    }

    #[test]
    fn spread_is_capped_at_thirty_minutes() {
        let mut s = Settings::default();
        s.platform_schedules = vec![PlatformSchedule {
            vendor: Vendor::TraeCn,
            hour: 0,
            minute: 10,
            spread_minutes: 99,
        }];
        // 只读一次（不归一化）时也要收敛，避免调用方拿到 99。
        assert_eq!(s.platform_schedule(Vendor::TraeCn).spread_minutes, 30);
        // 归一化会把超限值就地收敛：界面显示的与账号实际用的才会一致。
        assert!(s.ensure_platform_schedules());
        assert_eq!(s.platform_schedule(Vendor::TraeCn).spread_minutes, 30);
        assert_eq!(s.platform_schedules[1].spread_minutes, 30);
    }

    #[test]
    fn missing_platform_falls_back_to_default() {
        let mut s = Settings::default();
        s.platform_schedules.clear();
        let wb = s.platform_schedule(Vendor::WorkBuddy);
        assert_eq!((wb.hour, wb.minute, wb.spread_minutes), (0, 10, 5));
    }
}
