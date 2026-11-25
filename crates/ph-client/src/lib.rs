//! 手机端访问电脑上的 PointHub 局域网服务。
//!
//! 这里只做「HTTP 客户端 + 远程视图拼装」：
//! - 配对：用 6 位码换一台设备的长期令牌，令牌只存在客户端数据目录；
//! - 同步：pull / push 双端载荷，载荷里没有凭据；
//! - 视图：把同步载荷算成界面要的账号列表、合计、趋势。
//!
//! **不含签到逻辑**：签到永远在执行端（通常是电脑）完成，手机只发请求。

use ph_core::{
    Account, ActivityEvent, Error, MergeReport, Result, SyncPayload, Vendor, VendorSnapshot,
};
use ph_providers::provider_for;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// 客户端保存的配对信息。落盘为数据目录下的 `client.json`。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClientConfig {
    pub base_url: String,
    pub token: String,
    /// 服务端给这台客户端分配的设备 id。
    pub device_id: String,
    pub server_id: String,
    pub server_name: String,
    pub paired_at: i64,
}

#[derive(Debug, Clone, Deserialize)]
struct ServerInfo {
    id: String,
    name: String,
}

#[derive(Debug, Clone, Deserialize)]
struct PairResp {
    token: String,
    device_id: String,
    server: ServerInfo,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RemoteOutcome {
    pub label: String,
    pub status: String,
    pub status_text: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct RemoteAccount {
    pub id: String,
    pub vendor: String,
    pub vendor_display: String,
    pub label: String,
    pub hint: String,
    pub enabled: bool,
    pub checkin_enabled: bool,
    pub travel_enabled: bool,
    pub schedule_text: String,
    pub balance: Option<f64>,
    pub checked_in_today: Option<bool>,
    pub streak_days: Option<i64>,
    pub today_gain: Option<f64>,
    pub travel_state: Option<String>,
    pub travel_location: Option<String>,
    pub captured_at: Option<i64>,
    pub last_error: Option<String>,
    pub has_credential: bool,
    pub can_checkin: bool,
    pub can_travel: bool,
    pub executor: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct RemoteDay {
    pub date: String,
    pub total: f64,
    pub known: usize,
    pub gain: Option<f64>,
}

/// 手机界面要的远程状态。由一份 `SyncPayload` 纯函数算出，方便单测。
#[derive(Debug, Clone, Serialize, Default)]
pub struct RemoteView {
    pub server_name: String,
    pub base_url: String,
    pub accounts: Vec<RemoteAccount>,
    pub total_balance: f64,
    pub account_count: usize,
    pub pending_count: usize,
    pub balance_known: usize,
    pub last_sampled_at: Option<i64>,
    pub trend: Vec<RemoteDay>,
    pub events: Vec<ActivityEvent>,
    pub next_run_at: Option<i64>,
}

impl RemoteView {
    /// 把同步载荷算成界面视图。
    ///
    /// `can_checkin` 的三个条件缺一不可：平台支持、电脑持有凭据、电脑是执行端。
    /// 少判一个都会让手机上出现「点了没反应」的按钮。
    pub fn build(payload: &SyncPayload, config: &ClientConfig) -> Self {
        let latest = latest_by_account(&payload.snapshots);
        let mut accounts = Vec::with_capacity(payload.accounts.len());
        let mut total = 0f64;
        let mut known = 0usize;
        let mut pending = 0usize;
        let mut last_sampled: Option<i64> = None;

        for a in &payload.accounts {
            let snap = latest.get(&a.id);
            let cap = provider_for(a.vendor).capabilities();
            let has_cred = payload.credential_ids.iter().any(|x| x == &a.id);
            let on_server = a
                .executor
                .as_deref()
                .map(|e| e == payload.from_device)
                .unwrap_or(true);
            let balance = snap.and_then(|s| s.balance);

            if a.enabled {
                if let Some(b) = balance {
                    total += b;
                    known += 1;
                }
                if snap.map(|s| s.checked_in_today != Some(true)).unwrap_or(true) {
                    pending += 1;
                }
                if let Some(t) = snap.map(|s| s.captured_at) {
                    if last_sampled.map(|x| t > x).unwrap_or(true) {
                        last_sampled = Some(t);
                    }
                }
            }

            accounts.push(RemoteAccount {
                id: a.id.clone(),
                vendor: a.vendor.id().to_string(),
                vendor_display: a.vendor.display().to_string(),
                label: a.label.clone(),
                hint: a.hint.clone(),
                enabled: a.enabled,
                checkin_enabled: a.checkin_enabled,
                travel_enabled: a.travel_enabled,
                schedule_text: a.schedule.describe(),
                balance,
                checked_in_today: snap.and_then(|s| s.checked_in_today),
                streak_days: snap.and_then(|s| s.streak_days),
                today_gain: snap.and_then(|s| s.today_gain),
                travel_state: snap
                    .and_then(|s| s.travel.as_ref())
                    .map(|t| t.state.label().to_string()),
                travel_location: snap
                    .and_then(|s| s.travel.as_ref())
                    .and_then(|t| t.location.clone()),
                captured_at: snap.map(|s| s.captured_at),
                last_error: a.last_error.clone(),
                has_credential: has_cred,
                can_checkin: cap.checkin && has_cred && on_server,
                can_travel: cap.travel && has_cred && on_server && a.travel_enabled,
                executor: a.executor.clone(),
            });
        }

        let mut events = payload.events.clone();
        events.sort_by(|a, b| b.at.cmp(&a.at));
        events.truncate(12);

        Self {
            server_name: config.server_name.clone(),
            base_url: config.base_url.clone(),
            accounts,
            total_balance: total,
            account_count: payload.accounts.len(),
            pending_count: pending,
            balance_known: known,
            last_sampled_at: last_sampled,
            trend: daily_series(&payload.snapshots, &payload.accounts, 7),
            events,
            next_run_at: payload.next_run_at,
        }
    }
}

fn latest_by_account(snaps: &[VendorSnapshot]) -> BTreeMap<String, &VendorSnapshot> {
    let mut map: BTreeMap<String, &VendorSnapshot> = BTreeMap::new();
    for s in snaps {
        match map.get(&s.account_id) {
            Some(prev) if prev.captured_at >= s.captured_at => {}
            _ => {
                map.insert(s.account_id.clone(), s);
            }
        }
    }
    map
}

/// 近 `days` 天的每日余额合计（含今天，按日期升序）。
///
/// 与电脑端 `Engine::daily_series` 同一口径：只统计载荷里当前的账号，
/// 每天取最后一次采样；只有相邻两天都有数据才算得出新增。
fn daily_series(snaps: &[VendorSnapshot], accounts: &[Account], days: i64) -> Vec<RemoteDay> {
    use chrono::Datelike;

    let today = chrono::Local::now().date_naive();
    let from = today - chrono::Duration::days(days.max(1) - 1);
    let mut per_day: BTreeMap<chrono::NaiveDate, BTreeMap<String, f64>> = BTreeMap::new();

    for s in snaps {
        let Some(dt) = ph_core::schedule::local_from_epoch(s.captured_at) else {
            continue;
        };
        let d = dt.date();
        if d < from || d > today {
            continue;
        }
        if let Some(b) = s.balance {
            per_day.entry(d).or_default().insert(s.account_id.clone(), b);
        }
    }

    let mut out = Vec::with_capacity(days.max(1) as usize);
    let mut prev: Option<(f64, usize)> = None;
    for i in (0..days.max(1)).rev() {
        let d = today - chrono::Duration::days(i);
        let mut total = 0f64;
        let mut known = 0usize;
        if let Some(m) = per_day.get(&d) {
            for a in accounts {
                if let Some(b) = m.get(&a.id) {
                    total += *b;
                    known += 1;
                }
            }
        }
        let gain = match prev {
            Some((pt, pk)) if known > 0 && pk > 0 => Some(total - pt),
            _ => None,
        };
        if known > 0 {
            prev = Some((total, known));
        }
        out.push(RemoteDay {
            date: format!("{:02}-{:02}", d.month(), d.day()),
            total,
            known,
            gain,
        });
    }
    out
}

/// 去掉协议前缀差异与尾部斜杠，拒绝带路径的地址。
///
/// 用户通常只会抄「192.168.1.5:47821」，这里替他把 `http://` 补上。
pub fn normalize_base(input: &str) -> Result<String> {
    let s = input.trim();
    if s.is_empty() {
        return Err(Error::Invalid("请填写电脑地址".into()));
    }
    let with_scheme = if s.starts_with("http://") || s.starts_with("https://") {
        s.to_string()
    } else {
        format!("http://{s}")
    };
    let url = reqwest::Url::parse(&with_scheme)
        .map_err(|_| Error::Invalid(format!("地址格式不对：{s}")))?;
    if url.host_str().is_none() {
        return Err(Error::Invalid(format!("地址缺少主机：{s}")));
    }
    if url.path() != "/" {
        return Err(Error::Invalid("只填 IP:端口 即可，不要带路径".into()));
    }
    let mut out = url.to_string();
    while out.ends_with('/') {
        out.pop();
    }
    Ok(out)
}

/// 手机客户端。
pub struct Client {
    root: PathBuf,
    http: reqwest::Client,
    config: Option<ClientConfig>,
}

impl Client {
    pub fn open(root: impl AsRef<Path>) -> Result<Self> {
        let root = root.as_ref().to_path_buf();
        std::fs::create_dir_all(&root)?;
        let http = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(10))
            .user_agent(format!("PointHub/{}", ph_core::VERSION))
            .build()?;
        let config = load_config(&root)?;
        Ok(Self {
            root,
            http,
            config,
        })
    }

    pub fn config(&self) -> Option<&ClientConfig> {
        self.config.as_ref()
    }

    pub fn is_paired(&self) -> bool {
        self.config.is_some()
    }

    /// 忘记当前电脑。只删本地配对信息，不通知对端。
    pub fn unpair(&mut self) -> Result<()> {
        let p = self.root.join(ph_core::files::CLIENT);
        if p.exists() {
            std::fs::remove_file(&p)?;
        }
        self.config = None;
        Ok(())
    }

    /// 用配对码连接电脑。成功后配对信息立即落盘。
    pub async fn pair(&mut self, base_url: &str, code: &str) -> Result<ClientConfig> {
        let base = normalize_base(base_url)?;
        let code = code.trim();
        if code.is_empty() {
            return Err(Error::Invalid("请填写电脑上显示的 6 位配对码".into()));
        }
        let resp = self
            .http
            .post(format!("{base}/api/pair"))
            .json(&serde_json::json!({
                "code": code,
                "name": default_device_name(),
                "kind": device_kind(),
            }))
            .send()
            .await
            .map_err(network_hint)?;
        let pair: PairResp = decode(resp).await?;
        let cfg = ClientConfig {
            base_url: base,
            token: pair.token,
            device_id: pair.device_id,
            server_id: pair.server.id,
            server_name: pair.server.name,
            paired_at: chrono::Local::now().timestamp(),
        };
        save_config(&self.root, &cfg)?;
        self.config = Some(cfg.clone());
        Ok(cfg)
    }

    /// 拉取电脑端的同步载荷。`since` 为 0 表示全量。
    pub async fn pull(&self, since: i64) -> Result<SyncPayload> {
        let cfg = self.require()?;
        let resp = self
            .http
            .get(format!("{}/api/sync/pull?since={since}", cfg.base_url))
            .bearer_auth(&cfg.token)
            .send()
            .await
            .map_err(network_hint)?;
        decode(resp).await
    }

    /// 把本机载荷推给电脑。
    pub async fn push(&self, payload: &SyncPayload) -> Result<MergeReport> {
        let cfg = self.require()?;
        let resp = self
            .http
            .post(format!("{}/api/sync/push", cfg.base_url))
            .bearer_auth(&cfg.token)
            .json(payload)
            .send()
            .await
            .map_err(network_hint)?;
        decode(resp).await
    }

    /// 请求电脑采样一次积分。返回成功采样的账号数。
    pub async fn refresh(&self) -> Result<usize> {
        #[derive(Deserialize)]
        struct Resp {
            #[serde(default)]
            sampled: usize,
        }
        let cfg = self.require()?;
        let resp = self
            .http
            .post(format!("{}/api/refresh", cfg.base_url))
            .bearer_auth(&cfg.token)
            .json(&serde_json::json!({}))
            .send()
            .await
            .map_err(network_hint)?;
        Ok(decode::<Resp>(resp).await?.sampled)
    }

    /// 请求电脑执行签到。`ids` 为 None 表示全部签到。
    pub async fn checkin(&self, ids: Option<Vec<String>>) -> Result<Vec<RemoteOutcome>> {
        #[derive(Deserialize)]
        struct Resp {
            #[serde(default)]
            results: Vec<RemoteOutcome>,
        }
        let cfg = self.require()?;
        let resp = self
            .http
            .post(format!("{}/api/checkin", cfg.base_url))
            .bearer_auth(&cfg.token)
            .json(&serde_json::json!({ "ids": ids }))
            .send()
            .await
            .map_err(network_hint)?;
        Ok(decode::<Resp>(resp).await?.results)
    }

    /// 请求电脑对该账号执行派猫猫（自动判断派遣/领取）。
    pub async fn travel(&self, account_id: &str) -> Result<String> {
        #[derive(Deserialize)]
        struct Resp {
            message: String,
        }
        let cfg = self.require()?;
        let resp = self
            .http
            .post(format!("{}/api/travel", cfg.base_url))
            .bearer_auth(&cfg.token)
            .json(&serde_json::json!({ "id": account_id }))
            .send()
            .await
            .map_err(network_hint)?;
        Ok(decode::<Resp>(resp).await?.message)
    }

    fn require(&self) -> Result<&ClientConfig> {
        self.config
            .as_ref()
            .ok_or_else(|| Error::Auth("还没有连接电脑，请先配对".into()))
    }
}

fn config_path(root: &Path) -> PathBuf {
    root.join(ph_core::files::CLIENT)
}

fn load_config(root: &Path) -> Result<Option<ClientConfig>> {
    let p = config_path(root);
    if !p.exists() {
        return Ok(None);
    }
    let raw = std::fs::read_to_string(&p)?;
    if raw.trim().is_empty() {
        return Ok(None);
    }
    serde_json::from_str(&raw)
        .map(Some)
        .map_err(|e| Error::Format(format!("client.json 无法解析：{e}")))
}

fn save_config(root: &Path, cfg: &ClientConfig) -> Result<()> {
    let p = config_path(root);
    let body = serde_json::to_vec_pretty(cfg)
        .map_err(|e| Error::Format(format!("配对信息序列化失败：{e}")))?;
    let tmp = p.with_extension("tmp");
    std::fs::write(&tmp, body)?;
    if p.exists() {
        std::fs::remove_file(&p)?;
    }
    std::fs::rename(&tmp, &p)?;
    Ok(())
}

fn default_device_name() -> String {
    if device_kind() == "android" {
        "安卓手机".into()
    } else {
        "PointHub 客户端".into()
    }
}

fn device_kind() -> &'static str {
    if cfg!(target_os = "android") {
        "android"
    } else {
        "pc"
    }
}

/// 网络层失败时把真实原因挖出来，并补一句普通用户能照着做的排查方向。
///
/// `reqwest::Error` 的 Display 只有「error sending request」，真正的原因
/// （Permission denied / Network is unreachable / Connection timed out）
/// 藏在 source 链里；再补一次裸 TCP 探测，避免把系统级问题误判成「Wi-Fi 不对」。
fn network_hint(e: reqwest::Error) -> Error {
    let kind = if e.is_timeout() {
        "请求超时"
    } else if e.is_connect() {
        "连接失败"
    } else {
        "网络错误"
    };
    let mut detail = e.to_string();
    let mut src = std::error::Error::source(&e);
    while let Some(s) = src {
        let t = s.to_string();
        if !t.is_empty() && !detail.contains(&t) {
            detail.push_str("；");
            detail.push_str(&t);
        }
        src = s.source();
    }
    let probe = e
        .url()
        .map(|u| {
            tcp_probe(
                u.host_str().unwrap_or(""),
                u.port_or_known_default().unwrap_or(0),
            )
        })
        .unwrap_or_default();
    Error::Network(format!(
        "{kind}：{detail}{probe}（请确认手机和电脑连的是同一个 Wi-Fi）"
    ))
}

/// 绕过 HTTP 栈直接做一次 TCP 连接，把系统级错误原文带出来。
fn tcp_probe(host: &str, port: u16) -> String {
    use std::net::{TcpStream, ToSocketAddrs};
    use std::time::Duration;
    if host.is_empty() || port == 0 {
        return String::new();
    }
    let addrs: Vec<_> = match (host, port).to_socket_addrs() {
        Ok(it) => it.collect(),
        Err(e) => return format!("；TCP 自检：地址解析失败（{e}）"),
    };
    for addr in addrs {
        match TcpStream::connect_timeout(&addr, Duration::from_secs(3)) {
            Ok(_) => return format!("；TCP 自检：到 {addr} 可连通"),
            Err(e) => return format!("；TCP 自检：到 {addr} 失败（{e}）"),
        }
    }
    "；TCP 自检：没有可用地址".to_string()
}

/// 统一解析响应：非 2xx 时尽量取出服务端的 `error` 文案。
async fn decode<T: serde::de::DeserializeOwned>(resp: reqwest::Response) -> Result<T> {
    let status = resp.status();
    let bytes = resp.bytes().await?;
    if !status.is_success() {
        let msg = serde_json::from_slice::<serde_json::Value>(&bytes)
            .ok()
            .and_then(|v| {
                v.get("error")
                    .and_then(|e| e.as_str())
                    .map(|s| s.to_string())
            })
            .unwrap_or_else(|| format!("HTTP {}", status.as_u16()));
        return Err(match status.as_u16() {
            401 => Error::Auth(msg),
            403 => Error::Unsupported(msg),
            404 => Error::NotFound(msg),
            _ => Error::Upstream(msg),
        });
    }
    serde_json::from_slice(&bytes).map_err(|e| Error::Format(format!("响应无法解析：{e}")))
}

/// 平台能力查询（给桌面壳拼 StateView 用，避免重复依赖 ph-providers）。
pub fn capabilities(vendor: Vendor) -> ph_providers::Capabilities {
    provider_for(vendor).capabilities()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;

    fn tmpdir(tag: &str) -> PathBuf {
        let p = std::env::temp_dir().join(format!("ph-client-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&p);
        std::fs::create_dir_all(&p).unwrap();
        p
    }

    fn sample_payload() -> SyncPayload {
        let mut a1 = Account::new(Vendor::WorkBuddy, "主号", "ab...cd");
        a1.id = "a1".into();
        a1.executor = Some("pc-1".into());
        let mut a2 = Account::new(Vendor::TraeCn, "小号", "ef...gh");
        a2.id = "a2".into();
        a2.executor = Some("phone-9".into());

        let now = chrono::Local::now().timestamp();
        let mut s1 = VendorSnapshot::empty(&a1, now - 60);
        s1.balance = Some(100.0);
        s1.checked_in_today = Some(true);
        let mut s2 = VendorSnapshot::empty(&a2, now - 30);
        s2.balance = Some(50.0);
        s2.checked_in_today = Some(false);

        let mut p = SyncPayload::new(&ph_core::DeviceInfo::new("电脑", "pc"), 0);
        p.from_device = "pc-1".into();
        p.accounts = vec![a1, a2];
        p.snapshots = vec![s1, s2];
        p.credential_ids = vec!["a1".into(), "a2".into()];
        p.events = vec![
            ActivityEvent {
                at: now - 100,
                account_id: "a1".into(),
                label: "主号".into(),
                vendor: Vendor::WorkBuddy,
                kind: "checkin".into(),
                delta: Some(10),
                text: "已领取".into(),
            },
            ActivityEvent {
                at: now - 10,
                account_id: "a2".into(),
                label: "小号".into(),
                vendor: Vendor::TraeCn,
                kind: "checkin".into(),
                delta: Some(5),
                text: "今天已签".into(),
            },
        ];
        p
    }

    /// 极简 HTTP 服务：一次连接处理一个请求。
    async fn start_mock(
        handler: impl Fn(&str, &str, &[u8]) -> (u16, String) + Send + Sync + 'static,
    ) -> (String, tokio::task::JoinHandle<()>) {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let handler = Arc::new(handler);
        let task = tokio::spawn(async move {
            loop {
                let Ok((mut stream, _)) = listener.accept().await else {
                    break;
                };
                let h = handler.clone();
                tokio::spawn(async move {
                    let mut buf = Vec::new();
                    let mut tmp = [0u8; 4096];
                    let head_end = loop {
                        if let Some(p) = find(&buf, b"\r\n\r\n") {
                            break p;
                        }
                        match stream.read(&mut tmp).await {
                            Ok(0) | Err(_) => return,
                            Ok(n) => buf.extend_from_slice(&tmp[..n]),
                        }
                    };
                    let head = String::from_utf8_lossy(&buf[..head_end]).to_string();
                    let mut lines = head.lines();
                    let mut first = lines.next().unwrap_or("").split_whitespace();
                    let method = first.next().unwrap_or("").to_string();
                    let path = first.next().unwrap_or("").to_string();
                    let mut auth = String::new();
                    let mut len = 0usize;
                    for line in lines {
                        if let Some((k, v)) = line.split_once(':') {
                            match k.trim().to_ascii_lowercase().as_str() {
                                "authorization" => {
                                    auth = v.trim().strip_prefix("Bearer ").unwrap_or(v.trim()).to_string()
                                }
                                "content-length" => len = v.trim().parse().unwrap_or(0),
                                _ => {}
                            }
                        }
                    }
                    let mut body = buf[head_end + 4..].to_vec();
                    while body.len() < len {
                        match stream.read(&mut tmp).await {
                            Ok(0) | Err(_) => break,
                            Ok(n) => body.extend_from_slice(&tmp[..n]),
                        }
                    }
                    let (status, out) = h(&format!("{method} {path}"), &auth, &body);
                    let reason = if status == 200 { "OK" } else { "ERR" };
                    let resp = format!(
                        "HTTP/1.1 {status} {reason}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{out}",
                        out.len()
                    );
                    let _ = stream.write_all(resp.as_bytes()).await;
                    let _ = stream.flush().await;
                });
            }
        });
        (format!("http://{addr}"), task)
    }

    fn find(hay: &[u8], needle: &[u8]) -> Option<usize> {
        hay.windows(needle.len()).position(|w| w == needle)
    }

    #[test]
    fn normalize_base_accepts_common_input() {
        assert_eq!(normalize_base("192.168.1.5:47821").unwrap(), "http://192.168.1.5:47821");
        assert_eq!(normalize_base("http://10.0.0.2:8000/").unwrap(), "http://10.0.0.2:8000");
        assert!(normalize_base("  ").is_err());
        assert!(normalize_base("http://1.2.3.4:5/api").is_err());
    }

    #[tokio::test]
    async fn pair_then_pull_uses_saved_token() {
        let dir = tmpdir("pair");
        let payload_json = serde_json::to_string(&sample_payload()).unwrap();
        let (base, _task) = start_mock(move |line, auth, body| {
            if line == "POST /api/pair" {
                let v: serde_json::Value = serde_json::from_slice(body).unwrap();
                assert_eq!(v["code"], "123456");
                (
                    200,
                    r#"{"token":"tok-1","device_id":"dev-1","server":{"id":"srv-1","name":"书房电脑"}}"#
                        .to_string(),
                )
            } else if line.starts_with("GET /api/sync/pull") {
                assert_eq!(auth, "tok-1");
                (200, payload_json.clone())
            } else {
                (404, r#"{"error":"no"}"#.to_string())
            }
        })
        .await;

        let mut c = Client::open(&dir).unwrap();
        assert!(!c.is_paired());
        let cfg = c.pair(&base, "123456").await.unwrap();
        assert_eq!(cfg.server_name, "书房电脑");

        // 重新打开：配对信息必须还在。
        let c2 = Client::open(&dir).unwrap();
        assert_eq!(c2.config().unwrap().token, "tok-1");

        let p = c2.pull(0).await.unwrap();
        assert_eq!(p.accounts.len(), 2);
        assert_eq!(p.from_device, "pc-1");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn pull_maps_401_to_auth_error() {
        let dir = tmpdir("auth");
        let (base, _task) = start_mock(|line, _auth, _body| {
            if line == "POST /api/pair" {
                (
                    200,
                    r#"{"token":"bad","device_id":"d","server":{"id":"s","name":"pc"}}"#.to_string(),
                )
            } else {
                (401, r#"{"error":"配对被撤销了"}"#.to_string())
            }
        })
        .await;
        let mut c = Client::open(&dir).unwrap();
        c.pair(&base, "000000").await.unwrap();
        let err = c.pull(0).await.unwrap_err();
        assert!(matches!(err, Error::Auth(_)));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn checkin_returns_results_and_403_is_unsupported() {
        let dir = tmpdir("checkin");
        let allow = true;
        let (base, _task) = start_mock(move |line, _auth, _body| {
            if line == "POST /api/pair" {
                (
                    200,
                    r#"{"token":"t","device_id":"d","server":{"id":"s","name":"pc"}}"#.to_string(),
                )
            } else if allow {
                (
                    200,
                    r#"{"results":[{"label":"主号","status":"claimed","status_text":"已领取"}]}"#
                        .to_string(),
                )
            } else {
                (403, r#"{"error":"电脑上关掉了「允许手机签到」"}"#.to_string())
            }
        })
        .await;
        let mut c = Client::open(&dir).unwrap();
        c.pair(&base, "111111").await.unwrap();
        let out = c.checkin(None).await.unwrap();
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].status, "claimed");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn unpair_forgets_config() {
        let dir = tmpdir("unpair");
        let (base, _task) = start_mock(|_, _, _| {
            (
                200,
                r#"{"token":"t","device_id":"d","server":{"id":"s","name":"pc"}}"#.to_string(),
            )
        })
        .await;
        let mut c = Client::open(&dir).unwrap();
        c.pair(&base, "222222").await.unwrap();
        assert!(c.is_paired());
        c.unpair().unwrap();
        assert!(!c.is_paired());
        assert!(!config_path(&dir).exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn remote_view_aggregates_and_respects_executor() {
        let payload = sample_payload();
        let cfg = ClientConfig {
            base_url: "http://pc:1".into(),
            token: "t".into(),
            device_id: "phone-9".into(),
            server_id: "pc-1".into(),
            server_name: "书房电脑".into(),
            paired_at: 0,
        };
        let v = RemoteView::build(&payload, &cfg);

        assert_eq!(v.accounts.len(), 2);
        assert_eq!(v.total_balance, 150.0);
        assert_eq!(v.balance_known, 2);
        assert_eq!(v.pending_count, 1, "已签的 a1 不该算待签");
        assert_eq!(v.server_name, "书房电脑");
        // a1 由电脑执行：手机可远程签；a2 由 phone-9 执行：电脑签不了。
        assert!(v.accounts[0].can_checkin);
        assert!(!v.accounts[1].can_checkin);
        // 活动按时间倒序。
        assert_eq!(v.events[0].account_id, "a2");
        assert_eq!(v.trend.len(), 7);
        assert_eq!(v.trend[6].gain, None, "只有一天数据时没有对比");
    }

    #[test]
    fn remote_view_trend_computes_day_gain() {
        let now = chrono::Local::now().timestamp();
        let yesterday = now - 86_400;
        let mut a = Account::new(Vendor::WorkBuddy, "a", "h");
        a.id = "a1".into();
        a.executor = Some("pc-1".into());

        let mut s1 = VendorSnapshot::empty(&a, yesterday);
        s1.balance = Some(100.0);
        let mut s2 = VendorSnapshot::empty(&a, now);
        s2.balance = Some(140.0);

        let mut p = SyncPayload::new(&ph_core::DeviceInfo::new("pc", "pc"), 0);
        p.from_device = "pc-1".into();
        p.accounts = vec![a];
        p.snapshots = vec![s1, s2];
        p.credential_ids = vec!["a1".into()];

        let cfg = ClientConfig {
            base_url: "http://pc:1".into(),
            token: "t".into(),
            device_id: "phone-9".into(),
            server_id: "pc-1".into(),
            server_name: "pc".into(),
            paired_at: 0,
        };
        let v = RemoteView::build(&p, &cfg);
        assert_eq!(v.trend.len(), 7);
        assert_eq!(v.trend[5].total, 100.0, "昨天最后一条余额");
        assert_eq!(v.trend[6].total, 140.0, "今天最后一条余额");
        assert_eq!(v.trend[6].gain, Some(40.0), "相邻两天都有数据才算新增");
    }

    #[test]
    fn remote_view_hides_checkin_without_credential() {
        let mut payload = sample_payload();
        payload.credential_ids.clear();
        let cfg = ClientConfig {
            base_url: "http://pc:1".into(),
            token: "t".into(),
            device_id: "phone-9".into(),
            server_id: "pc-1".into(),
            server_name: "pc".into(),
            paired_at: 0,
        };
        let v = RemoteView::build(&payload, &cfg);
        assert!(v.accounts.iter().all(|a| !a.can_checkin));
        assert!(v.accounts.iter().all(|a| !a.has_credential));
    }
}
