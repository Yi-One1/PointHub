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
        }
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
}
