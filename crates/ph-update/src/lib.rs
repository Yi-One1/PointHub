//! 自动更新：查询 GitHub Releases、比较版本、下载安装包。
//!
//! 这里只做「查 + 下」，**不做静默自替换**：
//! - Windows 下载 NSIS 安装包后交给系统安装（安装器负责覆盖旧文件）；
//! - 安卓把 APK 地址交给系统浏览器下载，由用户在通知栏点开安装
//!   （应用内安装需要额外的原生 Intent 插件，见 DECISIONS D08）。
//!
//! 更新源可配置：默认直连 GitHub；国内慢时在设置里填镜像前缀，
//! 前缀会拼在完整 GitHub 地址前面，查询与下载走同一条规则。

use ph_core::{Error, Result, UpdateSettings, UpdateSource};
use serde::Serialize;
use serde_json::Value;
use std::path::{Path, PathBuf};
use std::time::Duration;

/// 仓库的 Releases API。
pub const RELEASES_API: &str = "https://api.github.com/repos/Yi-One1/PointHub/releases/latest";
/// 仓库的 Releases 页面（手动下载兜底）。
pub const RELEASES_PAGE: &str = "https://github.com/Yi-One1/PointHub/releases/latest";

/// 当前平台该拿哪种安装包。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Target {
    Windows,
    Android,
}

impl Target {
    /// 编译期确定，不做运行期判断：Windows 包里永远不会有 Android 分支。
    pub fn current() -> Self {
        #[cfg(target_os = "android")]
        {
            Target::Android
        }
        #[cfg(not(target_os = "android"))]
        {
            Target::Windows
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct AssetInfo {
    pub name: String,
    /// 已套用镜像前缀的下载地址。
    pub url: String,
    pub size: Option<u64>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ReleaseInfo {
    /// 归一化后的版本号（去掉 `v` 前缀）。
    pub version: String,
    pub notes: String,
    pub page_url: String,
    pub published_at: Option<String>,
    /// 当前平台的安装包；该版本没上传时为空。
    pub asset: Option<AssetInfo>,
}

/// 把 GitHub 地址套上镜像前缀。镜像为空时原样返回。
pub fn proxied(s: &UpdateSettings, url: &str) -> String {
    if s.source != UpdateSource::Mirror {
        return url.to_string();
    }
    let prefix = s.mirror.trim();
    if prefix.is_empty() {
        return url.to_string();
    }
    format!("{}/{}", prefix.trim_end_matches('/'), url)
}

pub fn api_url(s: &UpdateSettings) -> String {
    proxied(s, RELEASES_API)
}

pub fn releases_page(s: &UpdateSettings) -> String {
    proxied(s, RELEASES_PAGE)
}

/// `v0.5.0` / `0.5.0-beta.1` → `0.5.0`。
pub fn normalize_version(tag: &str) -> String {
    let t = tag.trim().trim_start_matches(['v', 'V']);
    t.split(['-', '+']).next().unwrap_or("").trim().to_string()
}

fn version_parts(v: &str) -> Option<Vec<u64>> {
    let core = normalize_version(v);
    if core.is_empty() {
        return None;
    }
    core.split('.').map(|p| p.parse::<u64>().ok()).collect()
}

/// `latest` 是否比 `current` 新。任一侧解析不了就当作「不是更新」，
/// 宁可漏一次提示，也不要把用户引去装一个更旧的包。
pub fn is_newer(current: &str, latest: &str) -> bool {
    match (version_parts(current), version_parts(latest)) {
        (Some(cur), Some(new)) => new > cur,
        _ => false,
    }
}

/// 从 GitHub 的 release JSON 里取版本、说明和当前平台的安装包。
pub fn parse_release(body: &Value, s: &UpdateSettings, target: Target) -> Result<ReleaseInfo> {
    let tag = body
        .get("tag_name")
        .and_then(Value::as_str)
        .ok_or_else(|| Error::Format("发布信息里没有 tag_name".into()))?;
    let version = normalize_version(tag);
    if version.is_empty() {
        return Err(Error::Format("发布信息的版本号为空".into()));
    }

    let page_url = proxied(
        s,
        body.get("html_url")
            .and_then(Value::as_str)
            .unwrap_or(RELEASES_PAGE),
    );
    let notes = body
        .get("body")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    let published_at = body
        .get("published_at")
        .and_then(Value::as_str)
        .map(str::to_string);

    let assets = body
        .get("assets")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let asset = pick_asset(&assets, target).map(|(name, url, size)| AssetInfo {
        name,
        url: proxied(s, &url),
        size,
    });

    Ok(ReleaseInfo {
        version,
        notes,
        page_url,
        published_at,
        asset,
    })
}

/// 在资产列表里挑出当前平台该下载的那个。
///
/// Windows 优先 `_Setup.exe`：便携版的 exe 无法在运行中替换自己，
/// 安装包才会走「覆盖安装」流程。安卓排除带 `debug` 的排障包。
fn pick_asset(assets: &[Value], target: Target) -> Option<(String, String, Option<u64>)> {
    let candidates: Vec<(String, String, Option<u64>)> = assets
        .iter()
        .filter_map(|a| {
            let name = a.get("name").and_then(Value::as_str)?.to_string();
            let url = a
                .get("browser_download_url")
                .and_then(Value::as_str)?
                .to_string();
            let size = a.get("size").and_then(Value::as_u64);
            Some((name, url, size))
        })
        .collect();

    let lower = |s: &str| s.to_ascii_lowercase();
    let chosen = match target {
        Target::Windows => candidates
            .iter()
            .find(|(n, _, _)| lower(n).ends_with("_setup.exe"))
            .or_else(|| candidates.iter().find(|(n, _, _)| lower(n).ends_with(".exe"))),
        Target::Android => candidates
            .iter()
            .find(|(n, _, _)| lower(n).ends_with(".apk") && !lower(n).contains("debug")),
    };
    chosen.cloned()
}

fn build_client() -> Result<reqwest::Client> {
    reqwest::Client::builder()
        .user_agent(concat!("PointHub-Updater/", env!("CARGO_PKG_VERSION")))
        .timeout(Duration::from_secs(25))
        .connect_timeout(Duration::from_secs(10))
        .build()
        .map_err(|e| Error::Network(format!("HTTP 客户端初始化失败：{e}")))
}

/// 查最近的发布；返回 `Some` 表示确实有更新。
pub async fn check(current: &str, s: &UpdateSettings) -> Result<Option<ReleaseInfo>> {
    check_for(current, s, Target::current()).await
}

/// 指定平台版本的 `check`，测试与桌面端显式调用时用。
pub async fn check_for(
    current: &str,
    s: &UpdateSettings,
    target: Target,
) -> Result<Option<ReleaseInfo>> {
    let client = build_client()?;
    let resp = client
        .get(api_url(s))
        .header("Accept", "application/vnd.github+json")
        .send()
        .await?;

    let status = resp.status();
    if !status.is_success() {
        let msg = match status.as_u16() {
            403 => "更新源拒绝访问（可能是请求太频繁），稍后再试".to_string(),
            404 => "更新源里还没有发布记录".to_string(),
            c => format!("更新源返回 HTTP {c}"),
        };
        return Err(Error::Upstream(msg));
    }

    let body: Value = resp.json().await?;
    let release = parse_release(&body, s, target)?;
    Ok(if is_newer(current, &release.version) {
        Some(release)
    } else {
        None
    })
}

/// 下载安装包到 `dir`，返回落盘路径。大小对不上视为下载不完整。
pub async fn download(asset: &AssetInfo, dir: &Path) -> Result<PathBuf> {
    std::fs::create_dir_all(dir)?;
    let client = build_client()?;
    let resp = client.get(&asset.url).send().await?;
    let status = resp.status();
    if !status.is_success() {
        return Err(Error::Network(format!(
            "下载安装包失败：HTTP {}",
            status.as_u16()
        )));
    }
    let bytes = resp.bytes().await?;
    if bytes.is_empty() {
        return Err(Error::Network("下载到的安装包是空的".into()));
    }
    if let Some(expect) = asset.size {
        if expect > 0 && bytes.len() as u64 != expect {
            return Err(Error::Network(format!(
                "安装包不完整：应为 {expect} 字节，实际 {} 字节",
                bytes.len()
            )));
        }
    }

    let path = dir.join(safe_file_name(&asset.name));
    std::fs::write(&path, &bytes)?;
    Ok(path)
}

/// 资产名来自网络，落盘前只保留最后一段，避免路径穿越。
fn safe_file_name(name: &str) -> String {
    let base = name
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or("")
        .trim()
        .to_string();
    if base.is_empty() || base == "." || base == ".." {
        "update.bin".to_string()
    } else {
        base
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn github() -> UpdateSettings {
        UpdateSettings::default()
    }

    fn mirror() -> UpdateSettings {
        UpdateSettings {
            source: UpdateSource::Mirror,
            mirror: "https://ghfast.top/".into(),
            ..UpdateSettings::default()
        }
    }

    #[test]
    fn version_comparison() {
        assert!(is_newer("0.4.2", "0.4.3"));
        assert!(is_newer("0.4.2", "0.5.0"));
        assert!(is_newer("0.9.9", "0.10.0"));
        assert!(!is_newer("0.4.2", "v0.4.2"));
        assert!(!is_newer("0.5.0", "0.4.9"));
        // 解析不了时不当成更新，避免把用户引去装更旧的包
        assert!(!is_newer("0.4.2", "nightly"));
        assert!(!is_newer("nightly", "0.5.0"));
    }

    #[test]
    fn version_normalization() {
        assert_eq!(normalize_version("v0.5.0"), "0.5.0");
        assert_eq!(normalize_version(" 0.5.0 "), "0.5.0");
        assert_eq!(normalize_version("0.5.0-beta.1"), "0.5.0");
    }

    #[test]
    fn mirror_prefix_applies_to_api_and_assets() {
        let g = github();
        assert_eq!(api_url(&g), RELEASES_API);
        assert_eq!(releases_page(&g), RELEASES_PAGE);
        assert_eq!(
            api_url(&mirror()),
            format!("https://ghfast.top/{RELEASES_API}")
        );

        let body = json!({
            "tag_name": "v0.5.0",
            "html_url": "https://github.com/Yi-One1/PointHub/releases/tag/v0.5.0",
            "assets": [
                {"name": "setup.exe", "browser_download_url": "https://github.com/a/setup.exe", "size": 10}
            ]
        });
        let r = parse_release(&body, &mirror(), Target::Windows).unwrap();
        assert_eq!(
            r.asset.unwrap().url,
            "https://ghfast.top/https://github.com/a/setup.exe"
        );
    }

    #[test]
    fn windows_prefers_setup_over_portable() {
        let body = json!({
            "tag_name": "v0.5.0",
            "assets": [
                {"name": "积分枢纽PointHub_v0.5.0.exe", "browser_download_url": "https://github.com/a/portable.exe"},
                {"name": "积分枢纽PointHub_v0.5.0_Setup.exe", "browser_download_url": "https://github.com/a/setup.exe", "size": 123}
            ]
        });
        let r = parse_release(&body, &github(), Target::Windows).unwrap();
        let asset = r.asset.expect("应选中安装包");
        assert!(asset.name.ends_with("_Setup.exe"));
        assert_eq!(asset.size, Some(123));
    }

    #[test]
    fn android_skips_debug_apk() {
        let body = json!({
            "tag_name": "v0.5.0",
            "assets": [
                {"name": "积分枢纽PointHub_v0.5.0_arm64-debug.apk", "browser_download_url": "https://github.com/a/debug.apk"},
                {"name": "积分枢纽PointHub_v0.5.0.apk", "browser_download_url": "https://github.com/a/app.apk", "size": 7400000}
            ]
        });
        let r = parse_release(&body, &github(), Target::Android).unwrap();
        let asset = r.asset.expect("应选到正式 APK");
        assert_eq!(asset.name, "积分枢纽PointHub_v0.5.0.apk");
        assert_eq!(asset.size, Some(7_400_000));
    }

    #[test]
    fn release_without_platform_asset_is_ok() {
        let body = json!({"tag_name": "v0.5.0", "assets": []});
        let r = parse_release(&body, &github(), Target::Windows).unwrap();
        assert_eq!(r.version, "0.5.0");
        assert!(r.asset.is_none());
    }

    #[test]
    fn asset_name_cannot_escape_directory() {
        assert_eq!(safe_file_name("../../evil.exe"), "evil.exe");
        assert_eq!(safe_file_name(".."), "update.bin");
        assert_eq!(safe_file_name("C:\\Windows\\x.exe"), "x.exe");
    }
}
