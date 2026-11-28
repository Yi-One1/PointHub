//! Tauri 命令层：前端能调的全部能力都在这里，一个不多。
//!
//! 约定：
//! - 命令只做「参数整形 + 调引擎 + 转成前端友好的结构」；
//! - 任何用户可见的错误都返回人话，不把 Rust 错误类型漏给前端；
//! - 需要通知用户的动作（签到完成/失败）在这里统一发系统通知。

#[cfg_attr(mobile, allow(unused_imports))]
use crate::{close_ball, open_ball, SharedState};
use ph_core::{AppMode, Credential, Settings, Vendor};
use ph_engine::Aggregate;
use ph_providers::{provider_for, TravelAction};
use serde::{Deserialize, Serialize};
use std::sync::atomic::Ordering;
use tauri::{AppHandle, Emitter, Manager, State};

// ------------------------------------------------------------------ 视图结构

#[derive(Serialize)]
pub struct PlatformCapView {
    pub vendor: String,
    pub display: String,
    pub checkin: bool,
    pub travel: bool,
    pub balance: bool,
    pub token_refresh: bool,
    pub verified: bool,
}

#[derive(Serialize)]
pub struct AccountView {
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
    pub last_checkin_at: Option<i64>,
    /// 失败后的自动重试最早时间（epoch 秒）；给界面展示「稍后重试」用。
    pub retry_after: Option<i64>,
    pub last_error: Option<String>,
    pub has_credential: bool,
    pub can_checkin: bool,
    pub can_travel: bool,
    pub verified: bool,
}

#[derive(Serialize)]
pub struct EventView {
    pub at: i64,
    pub account_id: String,
    pub label: String,
    pub vendor: String,
    pub kind: String,
    pub delta: Option<i64>,
    pub text: String,
}

#[derive(Serialize)]
pub struct StateView {
    pub version: String,
    pub data_dir: String,
    pub vault_kind: String,
    pub history_count: usize,
    pub accounts: Vec<AccountView>,
    pub aggregate: Aggregate,
    pub trend: Vec<ph_engine::DayPoint>,
    pub events: Vec<EventView>,
    /// 下一次自动签到的时刻（epoch 秒）；没有启用账号时为 None。
    pub next_run_at: Option<i64>,
    /// 今天到点但仍未签成功、且有错误信息的账号（总览页提示用）。
    pub checkin_alerts: Vec<ph_engine::CheckinAlert>,
    pub settings: Settings,
    pub platforms: Vec<PlatformCapView>,
    /// `local` | `remote`。
    pub mode: String,
    /// 远程连接信息；本机模式为 None。
    pub remote: Option<RemoteLinkView>,
}

/// 远程模式下的连接信息。
#[derive(Serialize, Clone)]
pub struct RemoteLinkView {
    pub paired: bool,
    pub base_url: Option<String>,
    pub server_name: Option<String>,
    pub device_id: Option<String>,
    /// 拉取失败时的原因；正常为 None。
    pub error: Option<String>,
}

#[derive(Serialize)]
pub struct ClientStatusView {
    pub mode: String,
    pub paired: bool,
    pub base_url: Option<String>,
    pub server_name: Option<String>,
    pub device_id: Option<String>,
}

#[derive(Serialize)]
pub struct OutcomeView {
    pub account_id: String,
    pub label: String,
    pub status: String,
    pub status_text: String,
    pub message: String,
    pub credit: Option<i64>,
}

pub(crate) fn build_state(engine: &ph_engine::Engine) -> StateView {
    let latest = engine.latest_snapshots();
    let accounts = engine
        .accounts()
        .iter()
        .map(|a| {
            let provider = provider_for(a.vendor);
            let cap = provider.capabilities();
            let snap = latest.get(&a.id);
            AccountView {
                id: a.id.clone(),
                vendor: a.vendor.id().to_string(),
                vendor_display: a.vendor.display().to_string(),
                label: a.label.clone(),
                hint: a.hint.clone(),
                enabled: a.enabled,
                checkin_enabled: a.checkin_enabled,
                travel_enabled: a.travel_enabled,
                schedule_text: a.schedule.describe(),
                balance: snap.and_then(|s| s.balance),
                checked_in_today: snap.and_then(|s| s.checked_in_today),
                streak_days: snap.and_then(|s| s.streak_days),
                today_gain: snap.and_then(|s| s.today_gain),
                travel_state: snap
                    .and_then(|s| s.travel.as_ref())
                    .map(|t| t.state.label().to_string()),
                travel_location: snap.and_then(|s| s.travel.as_ref()).and_then(|t| t.location.clone()),
                captured_at: snap.map(|s| s.captured_at),
                last_checkin_at: a.last_checkin_at,
                retry_after: a.retry_after,
                last_error: a.last_error.clone(),
                has_credential: engine.has_credential(&a.id),
                can_checkin: cap.checkin,
                can_travel: cap.travel && a.travel_enabled,
                verified: cap.verified,
            }
        })
        .collect();

    let platforms = ph_providers::all_capabilities()
        .into_iter()
        .map(|(v, c)| PlatformCapView {
            vendor: v.id().to_string(),
            display: v.display().to_string(),
            checkin: c.checkin,
            travel: c.travel,
            balance: c.balance,
            token_refresh: c.token_refresh,
            verified: c.verified,
        })
        .collect();

    StateView {
        version: ph_core::VERSION.to_string(),
        data_dir: engine.data_dir().to_string_lossy().to_string(),
        vault_kind: engine.vault_kind().to_string(),
        history_count: engine.history_count(),
        accounts,
        aggregate: engine.aggregate(),
        trend: engine.daily_series(7),
        events: engine
            .recent_events(12)
            .into_iter()
            .map(|e| EventView {
                at: e.at,
                account_id: e.account_id,
                label: e.label,
                vendor: e.vendor.id().to_string(),
                kind: e.kind,
                delta: e.delta,
                text: e.text,
            })
            .collect(),
        next_run_at: engine.next_run_at(),
        checkin_alerts: engine.checkin_alerts(chrono::Local::now().naive_local()),
        settings: engine.settings().clone(),
        platforms,
        mode: "local".into(),
        remote: None,
    }
}

/// 把远程载荷算出的视图拼成与本地一致的 `StateView`。
fn build_remote_state(
    view: ph_client::RemoteView,
    settings: Settings,
    remote: RemoteLinkView,
) -> StateView {
    let ph_client::RemoteView {
        server_name,
        accounts: remote_accounts,
        total_balance,
        account_count,
        pending_count,
        balance_known,
        last_sampled_at,
        trend,
        events,
        next_run_at,
        claimed_this_month,
        ..
    } = view;

    let accounts = remote_accounts
        .into_iter()
        .map(|a| {
            let verified = Vendor::parse(&a.vendor)
                .map(|v| ph_client::capabilities(v).verified)
                .unwrap_or(false);
            AccountView {
                id: a.id,
                vendor: a.vendor,
                vendor_display: a.vendor_display,
                label: a.label,
                hint: a.hint,
                enabled: a.enabled,
                checkin_enabled: a.checkin_enabled,
                travel_enabled: a.travel_enabled,
                schedule_text: a.schedule_text,
                balance: a.balance,
                checked_in_today: a.checked_in_today,
                streak_days: a.streak_days,
                today_gain: a.today_gain,
                travel_state: a.travel_state,
                travel_location: a.travel_location,
                captured_at: a.captured_at,
                last_checkin_at: None,
                retry_after: None,
                last_error: a.last_error,
                has_credential: a.has_credential,
                can_checkin: a.can_checkin,
                can_travel: a.can_travel,
                verified,
            }
        })
        .collect();

    let platforms = ph_providers::all_capabilities()
        .into_iter()
        .map(|(v, c)| PlatformCapView {
            vendor: v.id().to_string(),
            display: v.display().to_string(),
            checkin: c.checkin,
            travel: c.travel,
            balance: c.balance,
            token_refresh: c.token_refresh,
            verified: c.verified,
        })
        .collect();

    StateView {
        version: ph_core::VERSION.to_string(),
        data_dir: format!("远程 · {server_name}"),
        vault_kind: "凭据在电脑上".into(),
        history_count: 0,
        accounts,
        aggregate: Aggregate {
            total_balance,
            account_count,
            pending_count,
            balance_known,
            last_sampled_at,
            claimed_this_month,
        },
        trend: trend
            .into_iter()
            .map(|d| ph_engine::DayPoint {
                date: d.date,
                total: d.total,
                known: d.known,
                gain: d.gain,
                up: d.up,
                down: d.down,
            })
            .collect(),
        events: events
            .into_iter()
            .map(|e| EventView {
                at: e.at,
                account_id: e.account_id,
                label: e.label,
                vendor: e.vendor.id().to_string(),
                kind: e.kind,
                delta: e.delta,
                text: e.text,
            })
            .collect(),
        next_run_at,
        // 远程模式只展示电脑的数据，异常提示由电脑端负责，避免两端重复提示。
        checkin_alerts: Vec::new(),
        settings,
        platforms,
        mode: "remote".into(),
        remote: Some(remote),
    }
}

fn mode_str(m: AppMode) -> &'static str {
    match m {
        AppMode::Local => "local",
        AppMode::Remote => "remote",
    }
}

/// 远程模式下取一次电脑的数据；失败时回退到本机视图并带上错误原因，
/// 而不是整页报错——用户至少还能切回设置页重连。
async fn remote_state(state: &SharedState, settings: Settings, fallback: StateView) -> StateView {
    let client = state.client.lock().await;
    let Some(cfg) = client.config().cloned() else {
        let mut v = fallback;
        v.mode = "remote".into();
        v.remote = Some(RemoteLinkView {
            paired: false,
            base_url: None,
            server_name: None,
            device_id: None,
            error: Some("还没有连接电脑".into()),
        });
        return v;
    };
    let link = RemoteLinkView {
        paired: true,
        base_url: Some(cfg.base_url.clone()),
        server_name: Some(cfg.server_name.clone()),
        device_id: Some(cfg.device_id.clone()),
        error: None,
    };
    match client.pull(0).await {
        Ok(p) => build_remote_state(ph_client::RemoteView::build(&p, &cfg), settings, link),
        Err(e) => {
            let mut v = fallback;
            v.mode = "remote".into();
            v.remote = Some(RemoteLinkView {
                error: Some(e.user_message()),
                ..link
            });
            v
        }
    }
}

/// 远程模式下禁止修改账号；账号归电脑管。
async fn remote_guard(state: &SharedState) -> Result<(), String> {
    let engine = state.engine.lock().await;
    if engine.settings().mode == AppMode::Remote {
        return Err("远程模式下账号由电脑管理，请到电脑上操作".into());
    }
    Ok(())
}

// ------------------------------------------------------------------ 查询

#[tauri::command]
pub async fn bootstrap(state: State<'_, SharedState>) -> Result<StateView, String> {
    let (settings, local) = {
        let engine = state.engine.lock().await;
        (engine.settings().clone(), build_state(&engine))
    };
    if settings.mode == AppMode::Remote {
        return Ok(remote_state(state.inner(), settings, local).await);
    }
    Ok(local)
}

/// 全量采样后刷新视图。单个账号失败不影响其它账号。
///
/// 远程模式下这一步是「让电脑采样，再把结果拉回来」。
#[tauri::command]
pub async fn refresh_all(state: State<'_, SharedState>) -> Result<StateView, String> {
    run_refresh_inner(state.inner()).await
}

/// 供命令与后台自动刷新共用的采样执行体（D18）。
pub async fn run_refresh_inner(state: &SharedState) -> Result<StateView, String> {
    let (settings, local) = {
        let engine = state.engine.lock().await;
        (engine.settings().clone(), build_state(&engine))
    };
    if settings.mode == AppMode::Remote {
        {
            let client = state.client.lock().await;
            if client.is_paired() {
                client.refresh().await.map_err(|e| e.user_message())?;
            }
        }
        return Ok(remote_state(state, settings, local).await);
    }
    let mut engine = state.engine.lock().await;
    let (_snaps, _errs) = engine.snapshot_all().await;
    Ok(build_state(&engine))
}

// ------------------------------------------------------------------ 签到

#[tauri::command]
pub async fn checkin(
    app: AppHandle,
    ids: Option<Vec<String>>,
) -> Result<Vec<OutcomeView>, String> {
    run_checkin_inner(&app, ids).await
}

/// 供托盘菜单与命令共用的签到执行体（含通知）。
pub async fn run_checkin_inner(
    app: &AppHandle,
    ids: Option<Vec<String>>,
) -> Result<Vec<OutcomeView>, String> {
    let state = app.state::<SharedState>();
    let mode = {
        let engine = state.engine.lock().await;
        engine.settings().mode
    };
    // 远程模式：本机没有凭据，只能请求电脑执行。
    if mode == AppMode::Remote {
        let results = {
            let client = state.client.lock().await;
            client.checkin(ids).await.map_err(|e| e.user_message())?
        };
        let _ = app.emit("ph://changed", ());
        return Ok(results
            .into_iter()
            .map(|r| OutcomeView {
                account_id: String::new(),
                label: r.label,
                status: r.status,
                status_text: r.status_text,
                message: "已由电脑执行".into(),
                credit: None,
            })
            .collect());
    }
    let mut engine = state.engine.lock().await;
    let settings = engine.settings().clone();

    let raw = match ids {
        Some(list) if !list.is_empty() => {
            let mut out = Vec::new();
            for id in list {
                out.push(engine.checkin_one(&id).await);
            }
            out.into_iter().map(|r| r.map_err(|e| e.user_message())).collect::<Vec<_>>()
        }
        _ => engine
            .checkin_all()
            .await
            .into_iter()
            .map(Ok)
            .collect::<Vec<_>>(),
    };

    let mut views = Vec::new();
    let mut claimed = 0;
    let mut already = 0;
    let mut failed = 0;
    // 失败通知按「账号 + 自然日」去重：当天第一次失败才弹系统通知，
    // 后续失败只在总览页的「今日签到异常」里提示，避免每分钟刷屏。
    let mut notify_failure = false;
    for r in raw {
        match r {
            Ok(o) => {
                let label = engine
                    .find(&o.account_id)
                    .map(|a| a.label.clone())
                    .unwrap_or_else(|| o.account_id.clone());
                match o.status {
                    ph_core::CheckinStatus::Claimed => claimed += 1,
                    ph_core::CheckinStatus::AlreadyDone => already += 1,
                    ph_core::CheckinStatus::NotEligible => {}
                    _ => {
                        failed += 1;
                        if engine.mark_failure_notice(&o.account_id) {
                            notify_failure = true;
                        }
                    }
                }
                views.push(OutcomeView {
                    account_id: o.account_id,
                    label,
                    status: format!("{:?}", o.status).to_lowercase(),
                    status_text: o.status.label().to_string(),
                    message: o.message,
                    credit: o.credit,
                });
            }
            Err(msg) => {
                failed += 1;
                // 没有账号 id 的程序性错误无法按账号去重，直接提示一次。
                notify_failure = true;
                views.push(OutcomeView {
                    account_id: String::new(),
                    label: "未知账号".into(),
                    status: "failed".into(),
                    status_text: "失败".into(),
                    message: msg,
                    credit: None,
                });
            }
        }
    }
    drop(engine);

    // 系统通知：签到是后台行为，不给反馈用户会以为没跑。
    let should_notify = if failed > 0 {
        settings.notify_on_failure && notify_failure
    } else {
        settings.notify_on_checkin
    };
    if should_notify {
        use tauri_plugin_notification::NotificationExt;
        let body = format!(
            "新领 {claimed} 个 · 已签 {already} 个{}",
            if failed > 0 {
                format!(" · 失败 {failed} 个")
            } else {
                String::new()
            }
        );
        let _ = app
            .notification()
            .builder()
            .title("积分枢纽 · 签到完成")
            .body(body)
            .show();
    }

    Ok(views)
}

// ------------------------------------------------------------------ 派猫猫

#[tauri::command]
pub async fn travel(
    state: State<'_, SharedState>,
    id: String,
    action: Option<String>,
    location: Option<u32>,
) -> Result<String, String> {
    let act = match action.as_deref().unwrap_or("auto") {
        "status" => TravelAction::Status,
        "claim" => TravelAction::ClaimOnly,
        "depart" => TravelAction::DepartOnly,
        _ => TravelAction::Auto,
    };
    let mode = {
        let engine = state.engine.lock().await;
        engine.settings().mode
    };
    if mode == AppMode::Remote {
        let client = state.client.lock().await;
        return client.travel(&id).await.map_err(|e| e.user_message());
    }
    let mut engine = state.engine.lock().await;
    let snap = engine
        .travel_one(&id, act, location)
        .await
        .map_err(|e| e.user_message())?;
    Ok(format!(
        "{} {}{}",
        snap.state.label(),
        snap.location.unwrap_or_default(),
        snap.note
            .map(|n| format!("（{n}）"))
            .unwrap_or_default()
    ))
}

// ------------------------------------------------------------------ 账号

#[derive(Deserialize)]
pub struct AddAccountPayload {
    pub vendor: String,
    pub label: String,
    /// 令牌本体（长效或短期）。
    pub secret: String,
    /// "refresh" | "access"
    pub secret_kind: String,
    #[serde(default)]
    pub device_id: Option<String>,
    #[serde(default)]
    pub domain: Option<String>,
    #[serde(default)]
    pub cookie: Option<String>,
}

#[tauri::command]
pub async fn add_account(
    state: State<'_, SharedState>,
    payload: AddAccountPayload,
) -> Result<String, String> {
    remote_guard(state.inner()).await?;
    let vendor = Vendor::parse(&payload.vendor)
        .ok_or_else(|| format!("不认识的平台：{}", payload.vendor))?;
    if payload.secret.trim().is_empty() {
        return Err("令牌不能为空".into());
    }
    let mut cred = Credential::default();
    if payload.secret_kind == "access" {
        cred.access_token = Some(payload.secret.trim().to_string());
    } else {
        cred.refresh_token = Some(payload.secret.trim().to_string());
    }
    cred.device_id = payload.device_id.filter(|s| !s.trim().is_empty());
    cred.domain = payload.domain.filter(|s| !s.trim().is_empty());
    cred.cookie = payload.cookie.filter(|s| !s.trim().is_empty());

    // 手动令牌也可能和「浏览器授权 / 本机导入」是同一个账号，先问一次平台身份。
    if cred.platform_uid.is_none() {
        let provider = ph_providers::provider_for(vendor);
        if let Ok(Some(uid)) = provider.identity(&cred).await {
            cred.platform_uid = Some(uid);
        }
    }

    let mut engine = state.engine.lock().await;
    if let Some(uid) = cred.platform_uid.clone() {
        let _ = engine.adopt_identities(vendor, &uid).await;
    }
    let (acc, updated, merged) = engine
        .upsert_account(vendor, payload.label, cred)
        .map_err(|e| e.user_message())?;
    Ok(if updated {
        let tail = if merged > 0 {
            format!("，并合并了 {merged} 条重复记录")
        } else {
            String::new()
        };
        format!("该账号已存在，已更新凭据「{}」{tail}", acc.label)
    } else {
        format!("已添加账号「{}」", acc.label)
    })
}

// ------------------------------------------------------------------ 本机导入

#[derive(Serialize)]
pub struct LocalImportView {
    pub vendor: String,
    pub display: String,
    pub available: bool,
    /// 登录态已找到，但读取它需要用户允许一次额外的本地操作。
    pub needs_key: bool,
    pub detail: String,
}

/// 扫描本机客户端登录态（只读、不联网）。手机端恒为空列表。
#[tauri::command]
pub async fn local_import_scan(
    state: State<'_, SharedState>,
) -> Result<Vec<LocalImportView>, String> {
    remote_guard(state.inner()).await?;
    Ok(ph_providers::local_import::scan()
        .into_iter()
        .map(|s| LocalImportView {
            vendor: s.vendor.id().to_string(),
            display: s.vendor.display().to_string(),
            available: s.available,
            needs_key: s.needs_key,
            detail: s.detail,
        })
        .collect())
}

/// 读取本机客户端登录态并导入：先联网验证余额，通过后才落库。
#[tauri::command]
pub async fn local_import(
    app: AppHandle,
    state: State<'_, SharedState>,
    vendor: String,
    allow_key_fetch: Option<bool>,
) -> Result<String, String> {
    remote_guard(state.inner()).await?;
    let v = Vendor::parse(&vendor).ok_or_else(|| format!("不认识的平台：{vendor}"))?;
    let found = ph_providers::local_import::import(v, allow_key_fetch.unwrap_or(false))
        .map_err(|e| e.user_message())?;

    let (label, snap, updated, merged) = {
        let mut engine = state.engine.lock().await;
        let base = format!("{} 本机", v.display());
        let label = unique_label(&engine, v, &base);
        let (acc, snap, updated, merged) = engine
            .upsert_account_verified(v, label, found.credential)
            .await
            .map_err(|e| e.user_message())?;
        (acc.label, snap, updated, merged)
    };

    let bal = snap
        .balance
        .map(|b| b.to_string())
        .unwrap_or_else(|| "未知".into());
    let done = match snap.checked_in_today {
        Some(true) => "，今日已签",
        Some(false) => "，今日待签",
        None => "",
    };
    let message = if updated {
        let tail = if merged > 0 {
            format!("，并合并了 {merged} 条重复记录")
        } else {
            String::new()
        };
        format!("该账号已存在，已用本机登录态更新「{label}」{tail}：余额 {bal}{done}")
    } else {
        format!("已从本机导入「{label}」：余额 {bal}{done}")
    };
    let _ = app.emit("ph://login", &message);
    let _ = app.emit("ph://changed", ());
    Ok(message)
}

#[tauri::command]
pub async fn remove_account(state: State<'_, SharedState>, id: String) -> Result<(), String> {
    remote_guard(state.inner()).await?;
    let mut engine = state.engine.lock().await;
    engine.remove_account(&id).map_err(|e| e.user_message())
}

#[tauri::command]
pub async fn set_account_enabled(
    state: State<'_, SharedState>,
    id: String,
    enabled: bool,
) -> Result<(), String> {
    remote_guard(state.inner()).await?;
    let mut engine = state.engine.lock().await;
    engine
        .set_account_enabled(&id, enabled)
        .map_err(|e| e.user_message())
}

// ------------------------------------------------------------------ 浏览器登录导入

/// 打开某平台的官方登录页。拿到令牌后会自动建账号并回一条 `ph://login` 事件。
#[tauri::command]
pub async fn browser_login(
    app: AppHandle,
    state: State<'_, SharedState>,
    vendor: String,
) -> Result<(), String> {
    remote_guard(state.inner()).await?;
    let v = Vendor::parse(&vendor).ok_or_else(|| format!("不认识的平台：{vendor}"))?;
    crate::browser_login::start(app, v)
}

/// 浏览器授权成功后落库：建账号 -> 立刻读一次余额验证 -> 通知界面。
///
/// 「验证」这一步是刻意的：导入成功但令牌不可用，比导入失败更让人误判。
pub async fn import_from_browser(
    app: &AppHandle,
    vendor: Vendor,
    auth: crate::login::CapturedAuth,
) -> Result<(), String> {
    let cred = crate::login::build_credential(vendor, &auth);
    import_credential(app, vendor, cred).await
}

/// 三种浏览器授权流程共用的落库入口：建账号 → 读一次余额验证 → 通知界面。
pub async fn import_credential(
    app: &AppHandle,
    vendor: Vendor,
    cred: ph_core::Credential,
) -> Result<(), String> {
    let state = app.state::<SharedState>();

    let (account_id, label, updated, merged) = {
        let mut engine = state.engine.lock().await;
        // 浏览器授权拿到的是权威 user_id：先给旧的无身份记录补上，再 upsert 合并。
        if let Some(uid) = cred.platform_uid.clone() {
            let _ = engine.adopt_identities(vendor, &uid).await;
        }
        let base = format!("{} 网页", vendor.display());
        let label = unique_label(&engine, vendor, &base);
        let (acc, updated, merged) = engine
            .upsert_account(vendor, label, cred)
            .map_err(|e| e.user_message())?;
        (acc.id, acc.label, updated, merged)
    };

    let message = {
        let mut engine = state.engine.lock().await;
        let verb = if updated { "已更新" } else { "已导入" };
        match engine.snapshot_one(&account_id).await {
            Ok(s) => {
                let bal = s
                    .balance
                    .map(|b| b.to_string())
                    .unwrap_or_else(|| "未知".into());
                let done = match s.checked_in_today {
                    Some(true) => "，今日已签",
                    Some(false) => "，今日待签",
                    None => "",
                };
                if updated {
                    let tail = if merged > 0 {
                        format!("，并合并了 {merged} 条重复记录")
                    } else {
                        String::new()
                    };
                    format!("该账号已存在，{verb}凭据「{label}」{tail}：余额 {bal}{done}")
                } else {
                    format!("{verb}「{label}」：余额 {bal}{done}")
                }
            }
            Err(e) => format!("{verb}「{label}」，但读取余额失败：{}", e.user_message()),
        }
    };

    let _ = app.emit("ph://login", &message);
    let _ = app.emit("ph://changed", ());
    Ok(())
}

/// 同平台重名会让引擎拒绝新增，所以导入时自动编号。
fn unique_label(engine: &ph_engine::Engine, vendor: Vendor, base: &str) -> String {
    let used: Vec<&str> = engine
        .accounts()
        .iter()
        .filter(|a| a.vendor == vendor)
        .map(|a| a.label.as_str())
        .collect();
    if !used.contains(&base) {
        return base.to_string();
    }
    for n in 2..100 {
        let cand = format!("{base} {n}");
        if !used.iter().any(|l| *l == cand) {
            return cand;
        }
    }
    format!("{base} {}", chrono::Local::now().timestamp())
}

// ------------------------------------------------------------------ 局域网

/// 手机访问面板要的信息：地址、配对码、还剩多久。
#[derive(Serialize)]
pub struct LanInfo {
    /// 设置里的总开关。
    pub enabled: bool,
    /// 服务是否真的在跑（端口被占用时会启动失败）。
    pub running: bool,
    pub url: Option<String>,
    pub port: u16,
    pub code: Option<String>,
    /// 配对码剩余有效秒数。
    pub code_left_secs: u64,
    pub allow_checkin: bool,
}

#[tauri::command]
pub async fn lan_info(state: State<'_, SharedState>) -> Result<LanInfo, String> {
    use std::sync::atomic::Ordering;

    let (enabled, port) = (
        state.sync_enabled.load(Ordering::Relaxed),
        state.sync_port.load(Ordering::Relaxed),
    );
    let allow_checkin = {
        let engine = state.engine.lock().await;
        engine.settings().sync.allow_remote_checkin
    };

    // 配对码只在服务真的起来之后才有意义
    let ctx = state
        .lan
        .lock()
        .ok()
        .and_then(|slot| slot.as_ref().map(|c| (c.current_code(), c.code_left_secs(), c.port)));

    Ok(match ctx {
        Some((code, left, real_port)) => LanInfo {
            enabled,
            running: true,
            url: crate::lan::lan_ip().map(|ip| format!("http://{ip}:{real_port}")),
            port: real_port,
            code: Some(code),
            code_left_secs: left,
            allow_checkin,
        },
        None => LanInfo {
            enabled,
            running: false,
            url: None,
            port,
            code: None,
            code_left_secs: 0,
            allow_checkin,
        },
    })
}

// ------------------------------------------------------------------ 远程模式（安卓连电脑）

#[tauri::command]
pub async fn client_status(state: State<'_, SharedState>) -> Result<ClientStatusView, String> {
    let mode = {
        let engine = state.engine.lock().await;
        engine.settings().mode
    };
    let client = state.client.lock().await;
    let cfg = client.config().cloned();
    Ok(ClientStatusView {
        mode: mode_str(mode).to_string(),
        paired: client.is_paired(),
        base_url: cfg.as_ref().map(|c| c.base_url.clone()),
        server_name: cfg.as_ref().map(|c| c.server_name.clone()),
        device_id: cfg.as_ref().map(|c| c.device_id.clone()),
    })
}

async fn set_mode(state: &SharedState, mode: AppMode) -> Result<(), String> {
    let mut engine = state.engine.lock().await;
    let mut s = engine.settings().clone();
    s.mode = mode;
    engine.update_settings(s).map_err(|e| e.user_message())
}

#[tauri::command]
pub async fn client_pair(
    app: AppHandle,
    state: State<'_, SharedState>,
    base_url: String,
    code: String,
) -> Result<ClientStatusView, String> {
    {
        let mut client = state.client.lock().await;
        client
            .pair(&base_url, &code)
            .await
            .map_err(|e| e.user_message())?;
    }
    set_mode(state.inner(), AppMode::Remote).await?;
    let _ = app.emit("ph://changed", ());
    client_status(state).await
}

#[tauri::command]
pub async fn client_unpair(
    app: AppHandle,
    state: State<'_, SharedState>,
) -> Result<ClientStatusView, String> {
    {
        let mut client = state.client.lock().await;
        client.unpair().map_err(|e| e.user_message())?;
    }
    set_mode(state.inner(), AppMode::Local).await?;
    let _ = app.emit("ph://changed", ());
    client_status(state).await
}

// ------------------------------------------------------------------ 设置

#[tauri::command]
pub fn default_settings() -> Settings {
    Settings::default()
}

#[tauri::command]
pub async fn save_settings(
    app: AppHandle,
    state: State<'_, SharedState>,
    settings: Settings,
) -> Result<StateView, String> {
    {
        let mut engine = state.engine.lock().await;
        engine
            .update_settings(settings.clone())
            .map_err(|e| e.user_message())?;
    }

    state
        .minimize_to_tray
        .store(settings.minimize_to_tray, Ordering::Relaxed);

    // 自启状态对齐（仅桌面；安卓没有这个机制）
    #[cfg(desktop)]
    {
        use tauri_plugin_autostart::ManagerExt;
        let manager = app.autolaunch();
        let current = manager.is_enabled().unwrap_or(false);
        if settings.autostart && !current {
            let _ = manager.enable();
        } else if !settings.autostart && current {
            let _ = manager.disable();
        }
    }

    // 悬浮球开关
    if settings.ball.enabled != state.ball_enabled.load(Ordering::Relaxed) {
        let _ = apply_ball(&app, settings.ball.enabled);
    }

    // 局域网开关改为即时生效：先停旧监听，再按新设置决定是否重启。
    let was_enabled = state.sync_enabled.swap(settings.sync.enabled, Ordering::Relaxed);
    let old_port = state.sync_port.swap(settings.sync.port, Ordering::Relaxed);
    if was_enabled != settings.sync.enabled || old_port != settings.sync.port {
        if let Ok(mut slot) = state.lan.lock() {
            if let Some(ctx) = slot.take() {
                ctx.stop();
            }
        }
        if settings.sync.enabled {
            match crate::lan::start(app.clone()) {
                Ok(ctx) => {
                    if let Ok(mut slot) = state.lan.lock() {
                        *slot = Some(ctx);
                    }
                }
                Err(e) => eprintln!("局域网服务启动失败：{e}"),
            }
        }
    }

    let (saved, local) = {
        let engine = state.engine.lock().await;
        (engine.settings().clone(), build_state(&engine))
    };
    if saved.mode == AppMode::Remote {
        return Ok(remote_state(state.inner(), saved, local).await);
    }
    Ok(local)
}

#[tauri::command]
pub fn set_ball(app: AppHandle, enabled: bool) -> Result<(), String> {
    apply_ball(&app, enabled)
}

/// 悬浮球收起时的直径、展开后的面板尺寸（逻辑像素）。
const BALL_SIZE: f64 = 96.0;
const BALL_PANEL_W: f64 = 336.0;
const BALL_PANEL_H: f64 = 384.0;

/// 展开 / 收起悬浮球窗口。
///
/// 关键点：**右边缘锚定**。球贴在屏幕右侧时，若向左扩展且不锚定，
/// 面板会直接飞出屏幕；锚定后球体位置看不出变化，面板从它下面长出来。
/// 再对四边做一次夹紧，防止贴边时露到屏幕外。
#[tauri::command]
pub fn set_ball_expanded(app: AppHandle, expanded: bool) -> Result<(), String> {
    #[cfg(desktop)]
    {
        use tauri::{PhysicalPosition, PhysicalSize};
        let w = match app.get_webview_window("ball") {
            Some(w) => w,
            None => return Ok(()),
        };
        let scale = w.scale_factor().map_err(|e| e.to_string())?;
        let pos = w.outer_position().map_err(|e| e.to_string())?;
        let size = w.outer_size().map_err(|e| e.to_string())?;

        let (lw, lh) = if expanded {
            (BALL_PANEL_W, BALL_PANEL_H)
        } else {
            (BALL_SIZE, BALL_SIZE)
        };
        let new_w = (lw * scale).round().max(1.0) as i32;
        let new_h = (lh * scale).round().max(1.0) as i32;

        let right = pos.x + size.width as i32;
        let mut x = right - new_w;
        let mut y = pos.y;

        if let Ok(Some(m)) = w.current_monitor() {
            let ms = m.size();
            let mp = m.position();
            let left = mp.x;
            let top = mp.y;
            let right_lim = mp.x + ms.width as i32;
            let bottom_lim = mp.y + ms.height as i32;
            if x + new_w > right_lim {
                x = right_lim - new_w;
            }
            if x < left {
                x = left;
            }
            if y + new_h > bottom_lim {
                y = bottom_lim - new_h;
            }
            if y < top {
                y = top;
            }
        }

        // 先定位再改尺寸：反过来的话窗口会先缩小/放大再跳一下，肉眼可见。
        w.set_position(PhysicalPosition::new(x, y))
            .map_err(|e| e.to_string())?;
        w.set_size(PhysicalSize::new(new_w as u32, new_h as u32))
            .map_err(|e| e.to_string())?;
    }
    #[cfg(mobile)]
    {
        let _ = (app, expanded);
    }
    Ok(())
}

/// 悬浮球绑定哪个账号（`None` = 全部合计）。
///
/// 单独开一个命令而不是复用 `save_settings`：球窗口若拿旧快照整包回写，
/// 会把主窗口刚改的别的设置覆盖掉。
#[tauri::command]
pub async fn set_ball_account(
    state: State<'_, SharedState>,
    account_id: Option<String>,
) -> Result<(), String> {
    let mut engine = state.engine.lock().await;
    let mut s = engine.settings().clone();
    s.ball.selected = account_id;
    engine.update_settings(s).map_err(|e| e.user_message())
}

pub fn apply_ball(app: &AppHandle, enabled: bool) -> Result<(), String> {
    let state = app.state::<SharedState>();
    state.ball_enabled.store(enabled, Ordering::Relaxed);
    #[cfg(desktop)]
    {
        if enabled {
            open_ball(app).map_err(|e| format!("悬浮球创建失败：{e}"))?;
        } else {
            close_ball(app);
        }
    }
    #[cfg(mobile)]
    {
        let _ = (open_ball, close_ball, enabled);
    }
    // 记住开关，下次启动沿用
    let engine_locked = state.engine.try_lock();
    if let Ok(mut engine) = engine_locked {
        let mut s = engine.settings().clone();
        s.ball.enabled = enabled;
        let _ = engine.update_settings(s);
    }
    Ok(())
}

// ------------------------------------------------------------------ 更新

#[derive(Serialize)]
pub struct UpdateCheckView {
    pub current: String,
    /// 有更新时才是 `Some`。
    pub latest: Option<ph_update::ReleaseInfo>,
    /// `github` | `mirror`
    pub source: String,
}

#[derive(Serialize)]
pub struct UpdateApplyView {
    /// `latest` | `installer` | `downloaded` | `permission`
    pub action: String,
    pub detail: String,
    pub version: String,
    /// 安装包落盘路径。
    pub path: Option<String>,
}

/// 更新设置与数据目录。**锁只在函数内持有**：后面要发网络请求，
/// 把锁带过去会把签到/刷新一起堵住。
async fn update_ctx(state: &SharedState) -> (ph_core::UpdateSettings, std::path::PathBuf) {
    let engine = state.engine.lock().await;
    (
        engine.settings().update.clone(),
        engine.data_dir().to_path_buf(),
    )
}

fn update_source_str(s: ph_core::UpdateSource) -> &'static str {
    match s {
        ph_core::UpdateSource::Github => "github",
        ph_core::UpdateSource::Mirror => "mirror",
    }
}

/// 安卓 APK 落盘目录。
///
/// `Context.getDataDir()`（引擎的数据目录）是应用数据根，FileProvider 的
/// `<files-path>` 只能覆盖根下的 `files/`，所以 APK 必须放在 `files/updates`。
#[cfg(any(target_os = "android", test))]
fn android_apk_dir(data_dir: &std::path::Path) -> std::path::PathBuf {
    data_dir.join("files").join(ph_core::files::UPDATES_DIR)
}

#[tauri::command]
pub async fn update_check(state: State<'_, SharedState>) -> Result<UpdateCheckView, String> {
    let (u, _) = update_ctx(state.inner()).await;
    let current = ph_core::VERSION.to_string();
    let latest = ph_update::check(&current, &u)
        .await
        .map_err(|e| e.user_message())?;
    Ok(UpdateCheckView {
        current,
        latest,
        source: update_source_str(u.source).to_string(),
    })
}

/// 下载并交给系统安装。
///
/// Windows：下载 NSIS 安装包并启动它（没有安装包时退回便携版，提示手动替换）。
/// 安卓：下载到应用私有目录后交给系统安装器；缺少「安装未知应用」授权时
/// 打开系统设置页，用户回来会自动继续。
#[tauri::command]
pub async fn update_apply(
    app: AppHandle,
    state: State<'_, SharedState>,
) -> Result<UpdateApplyView, String> {
    let (u, data_dir) = update_ctx(state.inner()).await;
    let current = ph_core::VERSION.to_string();

    let Some(release) = ph_update::check(&current, &u)
        .await
        .map_err(|e| e.user_message())?
    else {
        return Ok(UpdateApplyView {
            action: "latest".into(),
            detail: "已经是最新版本".into(),
            version: current,
            path: None,
        });
    };

    let Some(asset) = release.asset.clone() else {
        return Err(format!(
            "v{} 还没有上传本平台的安装包，可点「手动下载」到发布页自取",
            release.version
        ));
    };

    // 两个平台分支各自只用其中一部分参数；显式消费掉，避免编译告警。
    let _ = (&app, &data_dir);

    #[cfg(target_os = "android")]
    {
        let dir = android_apk_dir(&data_dir);
        // 用户去系统设置授权后回来会再点一次；已下好就直接复用，不重复走网络。
        let path = match ph_update::cached(&asset, &dir) {
            Some(p) => p,
            None => ph_update::download(&asset, &dir)
                .await
                .map_err(|e| e.user_message())?,
        };
        let outcome = crate::android_apk::install(&path)?;
        let (action, detail) = if outcome.ok {
            (
                "installer",
                "安装器已打开，按提示完成安装后重新打开应用".to_string(),
            )
        } else if outcome.need_permission {
            ("permission", outcome.message)
        } else if outcome.message.is_empty() {
            (
                "downloaded",
                "无法调起安装器，可点「手动下载」到发布页自取".to_string(),
            )
        } else {
            ("downloaded", outcome.message)
        };
        return Ok(UpdateApplyView {
            action: action.into(),
            detail,
            version: release.version,
            path: Some(path.display().to_string()),
        });
    }

    #[cfg(not(target_os = "android"))]
    {
        let dir = data_dir.join(ph_core::files::UPDATES_DIR);
        let path = ph_update::download(&asset, &dir)
            .await
            .map_err(|e| e.user_message())?;
        if asset.name.to_ascii_lowercase().ends_with("_setup.exe") {
            std::process::Command::new(&path)
                .spawn()
                .map_err(|e| format!("安装程序启动失败：{e}"))?;
            Ok(UpdateApplyView {
                action: "installer".into(),
                detail: "安装程序已打开；按向导完成后重新启动应用".into(),
                version: release.version,
                path: Some(path.display().to_string()),
            })
        } else {
            Ok(UpdateApplyView {
                action: "downloaded".into(),
                detail: "已下载便携版；请退出本程序后用它替换旧文件".into(),
                version: release.version,
                path: Some(path.display().to_string()),
            })
        }
    }
}

/// 手动下载兜底：在系统浏览器里打开 Releases 页面。
#[tauri::command]
pub async fn update_open_page(
    app: AppHandle,
    state: State<'_, SharedState>,
) -> Result<(), String> {
    use tauri_plugin_opener::OpenerExt;
    let (u, _) = update_ctx(state.inner()).await;
    app.opener()
        .open_url(ph_update::releases_page(&u), None::<&str>)
        .map_err(|e| format!("打开浏览器失败：{e}"))
}

// ------------------------------------------------------------------ 目录 / 退出

#[tauri::command]
pub async fn open_data_dir(state: State<'_, SharedState>) -> Result<(), String> {
    let engine = state.engine.lock().await;
    let dir = engine.data_dir().to_path_buf();
    drop(engine);
    #[cfg(windows)]
    {
        std::process::Command::new("explorer")
            .arg(&dir)
            .spawn()
            .map_err(|e| format!("打开目录失败：{e}"))?;
    }
    #[cfg(not(windows))]
    {
        let _ = &dir;
    }
    Ok(())
}

/// 打开系统「选择文件夹」对话框；取消返回 None。
#[tauri::command]
pub async fn pick_data_dir() -> Option<String> {
    #[cfg(not(any(target_os = "android", target_os = "ios")))]
    {
        tauri::async_runtime::spawn_blocking(|| {
            rfd::FileDialog::new()
                .set_title("选择数据目录")
                .pick_folder()
                .map(|p| p.to_string_lossy().to_string())
        })
        .await
        .ok()
        .flatten()
    }
    #[cfg(any(target_os = "android", target_os = "ios"))]
    {
        None
    }
}

/// 搬迁数据目录：复制现有文件到新位置，写指针，重启后生效。
#[tauri::command]
pub async fn relocate_data(
    state: State<'_, SharedState>,
    path: String,
) -> Result<String, String> {
    let target = std::path::PathBuf::from(path.trim());
    if target.as_os_str().is_empty() {
        return Err("路径不能为空".into());
    }
    let from = current_data_dir(&state).await;
    move_data_dir(&from, &target, true)
}

/// 恢复默认数据目录：复制回程序所在目录，并清掉指针（继续跟随程序）。
#[tauri::command]
pub async fn reset_data_dir(state: State<'_, SharedState>) -> Result<String, String> {
    let from = current_data_dir(&state).await;
    move_data_dir(&from, &ph_store::locate::default_dir(), false)
}

/// 默认数据目录：发行版是程序所在目录下的 `data`，预览是项目里的 `dist\data`。
#[tauri::command]
pub fn default_data_dir() -> String {
    ph_store::locate::default_dir().to_string_lossy().to_string()
}

async fn current_data_dir(state: &State<'_, SharedState>) -> std::path::PathBuf {
    let engine = state.engine.lock().await;
    let dir = engine.data_dir().to_path_buf();
    drop(engine);
    dir
}

/// `pin` 为 true 时写指针记住用户选的位置；恢复默认则清指针，让目录继续跟着程序走。
fn move_data_dir(
    from: &std::path::Path,
    target: &std::path::Path,
    pin: bool,
) -> Result<String, String> {
    if from == target {
        return Ok(if pin {
            "目标与当前目录相同，无需搬迁。".into()
        } else {
            "数据目录已经是默认位置。".into()
        });
    }
    if target.starts_with(from) {
        return Err("目标目录不能在当前数据目录内部".into());
    }
    if from.starts_with(target) {
        return Err("目标目录不能包含当前数据目录".into());
    }

    std::fs::create_dir_all(target).map_err(|e| format!("无法创建目标目录：{e}"))?;
    let mut copied = 0usize;
    copy_dir(from, target, &mut copied).map_err(|e| format!("复制失败：{e}"))?;
    // 先在新目录记下旧路径，再切指针；下次启动时按标记清理旧目录。
    ph_store::locate::write_cleanup_marker(target, from).map_err(|e| e.user_message())?;
    if pin {
        ph_store::locate::write_pointer(Some(target)).map_err(|e| e.user_message())?;
    } else {
        ph_store::locate::write_pointer(None).map_err(|e| e.user_message())?;
    }
    Ok(if pin {
        format!(
            "已复制 {copied} 个文件到 {}；重启程序后生效，启动时会自动清理旧目录。",
            target.display()
        )
    } else {
        format!(
            "已复制 {copied} 个文件回默认目录 {}；重启程序后生效，启动时会自动清理旧目录。",
            target.display()
        )
    })
}

fn copy_dir(from: &std::path::Path, to: &std::path::Path, count: &mut usize) -> std::io::Result<()> {
    for entry in std::fs::read_dir(from)? {
        let entry = entry?;
        let path = entry.path();
        let dest = to.join(entry.file_name());
        if path.is_dir() {
            std::fs::create_dir_all(&dest)?;
            copy_dir(&path, &dest, count)?;
        } else {
            std::fs::copy(&path, &dest)?;
            *count += 1;
        }
    }
    Ok(())
}

#[tauri::command]
pub fn show_main(app: AppHandle) {
    crate::bring_main_window(&app);
}

#[tauri::command]
pub fn quit_app(app: AppHandle) {
    app.exit(0);
}

#[cfg(all(test, not(target_os = "android")))]
mod android_install_tests {
    use super::android_apk_dir;

    #[test]
    fn apk_lands_under_files_dir() {
        // FileProvider 的 <files-path> 只映射 getFilesDir()，即数据根下的 files/。
        let dir = android_apk_dir(std::path::Path::new("/data/user/0/cn.pointhub.desktop"));
        assert_eq!(
            dir,
            std::path::Path::new("/data/user/0/cn.pointhub.desktop/files/updates")
        );
    }

    /// Kotlin 源与构建补丁是 Android 安装链路的三个约定点，任何一处被改掉
    /// 都只会在真机上失败（桌面编译不报错），所以在这里钉死。
    #[test]
    fn install_contract_is_wired() {
        let kotlin = include_str!("../../android/src/cn/pointhub/desktop/ApkInstaller.kt");
        assert!(kotlin.contains("@JvmStatic"), "Kotlin 安装入口必须是静态方法");
        assert!(
            kotlin.contains("canRequestPackageInstalls"),
            "缺少「安装未知应用」授权检查"
        );
        assert!(
            kotlin.contains(".fileprovider"),
            "FileProvider authority 与 manifest 不一致"
        );
        assert!(
            kotlin.contains("resumePending"),
            "从系统设置返回后没有继续安装"
        );

        let patch = include_str!("../../../../scripts/patch-android-sources.ps1");
        assert!(
            patch.contains("REQUEST_INSTALL_PACKAGES"),
            "构建补丁没有加安装权限"
        );
        assert!(
            patch.contains("files-path name="),
            "构建补丁没有暴露 files/updates 给 FileProvider"
        );
        assert!(
            patch.contains("ApkInstaller"),
            "构建补丁没有保留安装器类（R8 会把它剥掉）"
        );
    }
}

#[cfg(all(test, not(target_os = "android")))]
mod android_vault_tests {
    /// 安卓凭据仓改用 AndroidKeyStore 后，Kotlin、构建补丁、后台任务三处
    /// 必须同时成立；桌面编译看不出来，只能在真机上踩，所以这里钉死。
    #[test]
    fn keystore_contract_is_wired() {
        let keystore = include_str!("../../android/src/cn/pointhub/desktop/VaultKeystore.kt");
        assert!(keystore.contains("@JvmStatic"), "加解密入口必须是静态方法");
        assert!(
            keystore.contains("AndroidKeyStore"),
            "密钥没有托管到系统密钥库"
        );
        assert!(
            keystore.contains("AES/GCM/NoPadding"),
            "加解密算法必须与 Rust 侧的密文格式一致"
        );

        let keys = include_str!("../../android/src/cn/pointhub/desktop/VaultKeys.kt");
        assert!(
            keys.contains("loadLibrary"),
            "上下文桥必须先加载原生库"
        );

        let worker = include_str!("../../android/src/cn/pointhub/desktop/CheckinWorker.kt");
        assert!(
            worker.contains("VaultKeys.init"),
            "后台任务冷启动时没有初始化密钥服务"
        );

        let patch = include_str!("../../../../scripts/patch-android-sources.ps1");
        assert!(
            patch.contains("VaultKeystore"),
            "构建补丁没有保留密钥类（R8 会把它剥掉）"
        );
        assert!(
            patch.contains("VaultKeys.init(this)"),
            "Activity 启动前没有把上下文交给 Rust"
        );
    }
}
