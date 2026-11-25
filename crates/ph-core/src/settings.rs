use serde::{Deserialize, Serialize};

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

impl Default for SyncSettings {
    fn default() -> Self {
        Self {
            enabled: true,
            port: default_port(),
            allow_remote_checkin: true,
        }
    }
}

/// 全局设置。整体落盘为 `settings.json`。
///
/// 注意：数据目录本身不在这里——它由 `ph-store` 的定位逻辑决定，
/// 否则会出现「改目录后设置文件找不到」的循环依赖。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Settings {
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
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            theme: ThemeMode::default(),
            autostart: false,
            minimize_to_tray: true,
            notify_on_checkin: true,
            notify_on_failure: true,
            ball: BallSettings::default(),
            sync: SyncSettings::default(),
            scheduler_enabled: true,
            catch_up_missed: true,
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
    }
}
