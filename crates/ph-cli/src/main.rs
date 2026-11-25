//! `phctl` —— 积分枢纽的命令行内核。
//!
//! 它存在的意义不只是「方便」：GUI 出错时，用它可以把「界面问题」和
//! 「签到链路问题」分开。任何一次线上故障，先跑 `phctl status` 就能定位
//! 是凭据、网络、还是平台改版。
//!
//! 用法：`phctl <命令> [参数]`，`phctl help` 看全部。

use ph_core::{Credential, Vendor};
use ph_engine::Engine;
use ph_providers::TravelAction;
use std::collections::HashMap;
use std::path::PathBuf;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .expect("无法启动 tokio 运行时");
    match rt.block_on(run(args)) {
        Ok(code) => std::process::exit(code),
        Err(e) => {
            eprintln!("错误：{e}");
            std::process::exit(1);
        }
    }
}

async fn run(args: Vec<String>) -> Result<i32, String> {
    // 允许 `--data-dir` 出现在命令之前（`phctl --data-dir X doctor` 与
    // `phctl doctor --data-dir X` 等价），否则用户很容易一上来就撞错。
    let mut args = args;
    let mut early_data_dir: Option<PathBuf> = None;
    if let Some(pos) = args.iter().position(|a| a == "--data-dir") {
        if let Some(v) = args.get(pos + 1).cloned() {
            early_data_dir = Some(PathBuf::from(v));
            args.drain(pos..=pos + 1);
        }
    }

    let (cmd, rest) = match args.split_first() {
        Some((c, r)) => (c.clone(), r.to_vec()),
        None => {
            print_help();
            return Ok(0);
        }
    };
    let opts = parse_opts(&rest);
    let data_dir = early_data_dir.or_else(|| opts.get("data-dir").map(PathBuf::from));

    match cmd.as_str() {
        "help" | "--help" | "-h" => {
            print_help();
            Ok(0)
        }
        "doctor" => cmd_doctor(data_dir),
        "account" => {
            let sub = opts.get("_0").cloned().unwrap_or_default();
            match sub.as_str() {
                "list" | "" => cmd_account_list(data_dir),
                "add" => cmd_account_add(data_dir, &opts),
                "rm" | "remove" => cmd_account_rm(data_dir, &opts),
                other => Err(format!("未知子命令 account {other}，可用：list / add / rm")),
            }
        }
        "status" => cmd_status(data_dir, &opts).await,
        "checkin" => cmd_checkin(data_dir, &opts).await,
        "travel" => cmd_travel(data_dir, &opts).await,
        "plan" => cmd_plan(data_dir),
        _ => Err(format!("未知命令 {cmd}，试试 `phctl help`")),
    }
}

/// 极简参数解析：`--key value` 与 `--flag`；位置参数进 `_0/_1`。
fn parse_opts(args: &[String]) -> HashMap<String, String> {
    let mut map = HashMap::new();
    let mut i = 0;
    let mut positional = 0;
    while i < args.len() {
        let a = &args[i];
        if let Some(key) = a.strip_prefix("--") {
            let has_value = args.get(i + 1).map(|n| !n.starts_with("--")).unwrap_or(false);
            if has_value {
                map.insert(key.to_string(), args[i + 1].clone());
                i += 2;
            } else {
                map.insert(key.to_string(), "true".to_string());
                i += 1;
            }
        } else {
            map.insert(format!("_{positional}"), a.clone());
            positional += 1;
            i += 1;
        }
    }
    map
}

fn open(dir: Option<PathBuf>) -> Result<Engine, String> {
    Engine::open(dir).map_err(|e| e.user_message())
}

fn cmd_doctor(dir: Option<PathBuf>) -> Result<i32, String> {
    let eng = open(dir)?;
    println!("积分枢纽 PointHub v{} (CLI)", ph_core::VERSION);
    println!("数据目录   : {}", eng.data_dir().display());
    println!("凭据加密   : {}", eng.vault_kind());
    println!(
        "账号数量   : {}（已导入凭据 {}）",
        eng.accounts().len(),
        eng.accounts().iter().filter(|a| eng.has_credential(&a.id)).count()
    );
    println!("历史记录   : {} 条", eng.history_count());
    println!();
    println!("平台能力矩阵：");
    for (vendor, cap) in ph_providers::all_capabilities() {
        println!(
            "  {:<10} 签到:{:<5} 余额:{:<5} 令牌续期:{:<5} 附加活动:{:<5} 验证:{}",
            vendor.display(),
            yn(cap.checkin),
            yn(cap.balance),
            yn(cap.token_refresh),
            yn(cap.travel),
            if cap.verified { "已实机" } else { "待实测" }
        );
    }
    Ok(0)
}

fn yn(b: bool) -> &'static str {
    if b {
        "有"
    } else {
        "无"
    }
}

fn cmd_account_list(dir: Option<PathBuf>) -> Result<i32, String> {
    let eng = open(dir)?;
    if eng.accounts().is_empty() {
        println!("还没有账号。用 `phctl account add --vendor workbuddy --label 主号 --refresh-token <令牌>` 添加。");
        return Ok(0);
    }
    println!(
        "{:<24} {:<12} {:<12} {:<10} {:<8} {}",
        "ID", "平台", "别名", "标识", "状态", "计划"
    );
    for a in eng.accounts() {
        println!(
            "{:<24} {:<12} {:<12} {:<10} {:<8} {}",
            short_id(&a.id),
            a.vendor.display(),
            a.label,
            a.hint,
            if !a.enabled {
                "已停用"
            } else if eng.has_credential(&a.id) {
                "就绪"
            } else {
                "缺凭据"
            },
            a.schedule.describe()
        );
        if let Some(err) = &a.last_error {
            println!("  └ 最近错误：{err}");
        }
    }
    Ok(0)
}

fn short_id(id: &str) -> String {
    if id.len() > 12 {
        format!("{}…", &id[..8])
    } else {
        id.to_string()
    }
}

fn cmd_account_add(dir: Option<PathBuf>, opts: &HashMap<String, String>) -> Result<i32, String> {
    let mut eng = open(dir)?;
    let vendor_raw = opts
        .get("vendor")
        .ok_or("缺少 --vendor（workbuddy / trae-cn / qoder-cn）")?;
    let vendor = Vendor::parse(vendor_raw)
        .ok_or_else(|| format!("不认识的平台：{vendor_raw}"))?;

    let mut cred = Credential::default();
    if let Some(t) = opts.get("refresh-token") {
        cred.refresh_token = Some(t.trim().to_string());
    }
    if let Some(t) = opts.get("access-token") {
        cred.access_token = Some(t.trim().to_string());
    }
    if let Some(t) = opts.get("device-id") {
        cred.device_id = Some(t.trim().to_string());
    }
    if let Some(t) = opts.get("cookie") {
        cred.cookie = Some(t.trim().to_string());
    }
    if let Some(t) = opts.get("domain") {
        cred.domain = Some(t.trim().to_string());
    }
    if cred.refresh_token.is_none() && cred.access_token.is_none() {
        return Err("至少要给 --refresh-token 或 --access-token".into());
    }
    let label = opts.get("label").cloned().unwrap_or_default();
    let acc = eng
        .add_account(vendor, label, cred)
        .map_err(|e| e.user_message())?;
    println!(
        "已添加：{} / {}（id {}）",
        acc.vendor.display(),
        acc.label,
        acc.id
    );
    println!("下一步：`phctl status --id {}` 验证凭据是否可用。", acc.id);
    Ok(0)
}

fn cmd_account_rm(dir: Option<PathBuf>, opts: &HashMap<String, String>) -> Result<i32, String> {
    let mut eng = open(dir)?;
    let key = opts
        .get("id")
        .or_else(|| opts.get("_1"))
        .ok_or("缺少账号 id")?;
    let id = eng
        .find(key)
        .map(|a| a.id.clone())
        .ok_or_else(|| format!("找不到账号 {key}"))?;
    eng.remove_account(&id).map_err(|e| e.user_message())?;
    println!("已删除 {id}");
    Ok(0)
}

async fn cmd_status(dir: Option<PathBuf>, opts: &HashMap<String, String>) -> Result<i32, String> {
    let mut eng = open(dir)?;
    if eng.accounts().is_empty() {
        println!("还没有账号。");
        return Ok(0);
    }
    let targets: Vec<String> = match opts.get("id") {
        Some(key) => vec![eng
            .find(key)
            .map(|a| a.id.clone())
            .ok_or_else(|| format!("找不到账号 {key}"))?],
        None => eng.accounts().iter().map(|a| a.id.clone()).collect(),
    };
    for id in targets {
        let label = eng.find(&id).map(|a| a.label.clone()).unwrap_or_default();
        match eng.snapshot_one(&id).await {
            Ok(s) => {
                println!(
                    "{:<10} 余额 {:<8} 今日签到 {:<6} 连签 {:<4} 今日新增 {}",
                    label,
                    s.balance
                        .map(ph_providers::workbuddy::fmt_credits)
                        .unwrap_or_else(|| "未知".into()),
                    match s.checked_in_today {
                        Some(true) => "已签",
                        Some(false) => "未签",
                        None => "未知",
                    },
                    s.streak_days
                        .map(|d| d.to_string())
                        .unwrap_or_else(|| "-".into()),
                    s.today_gain
                        .map(|g| format!("+{g}"))
                        .unwrap_or_else(|| "-".into())
                );
                if let Some(n) = s.note {
                    println!("  └ {n}");
                }
            }
            Err(e) => println!("{label:<10} 查询失败：{}", e.user_message()),
        }
    }
    Ok(0)
}

async fn cmd_checkin(dir: Option<PathBuf>, opts: &HashMap<String, String>) -> Result<i32, String> {
    let mut eng = open(dir)?;
    let outcomes = match opts.get("id") {
        Some(key) => {
            let id = eng
                .find(key)
                .map(|a| a.id.clone())
                .ok_or_else(|| format!("找不到账号 {key}"))?;
            vec![eng.checkin_one(&id).await.map_err(|e| e.user_message())?]
        }
        None => eng.checkin_all().await,
    };
    let mut bad = 0;
    for o in &outcomes {
        let label = eng.find(&o.account_id).map(|a| a.label.clone()).unwrap_or_default();
        println!(
            "{:<10} {:<8} {}",
            label,
            o.status.label(),
            o.message
        );
        if !o.status.is_ok() {
            bad += 1;
        }
    }
    println!(
        "完成 {} 个账号，成功 {}，需处理 {}",
        outcomes.len(),
        outcomes.len() - bad,
        bad
    );
    Ok(if bad > 0 { 2 } else { 0 })
}

async fn cmd_travel(dir: Option<PathBuf>, opts: &HashMap<String, String>) -> Result<i32, String> {
    let mut eng = open(dir)?;
    let action = if opts.contains_key("claim") {
        TravelAction::ClaimOnly
    } else if opts.contains_key("depart") {
        TravelAction::DepartOnly
    } else if opts.contains_key("status") {
        TravelAction::Status
    } else {
        TravelAction::Auto
    };
    let location = opts.get("location").and_then(|s| s.parse::<u32>().ok());
    let targets: Vec<String> = match opts.get("id") {
        Some(key) => vec![eng
            .find(key)
            .map(|a| a.id.clone())
            .ok_or_else(|| format!("找不到账号 {key}"))?],
        None => eng
            .accounts()
            .iter()
            .filter(|a| a.enabled && a.travel_enabled)
            .map(|a| a.id.clone())
            .collect(),
    };
    if targets.is_empty() {
        println!("没有启用派猫猫旅行的账号。");
        return Ok(0);
    }
    for id in targets {
        let label = eng.find(&id).map(|a| a.label.clone()).unwrap_or_default();
        match eng.travel_one(&id, action, location).await {
            Ok(t) => {
                println!("{:<10} {} {}", label, t.state.label(), t.location.clone().unwrap_or_default());
                if let Some(n) = t.note {
                    println!("  └ {n}");
                }
            }
            Err(e) => println!("{label:<10} 失败：{}", e.user_message()),
        }
    }
    Ok(0)
}

fn cmd_plan(dir: Option<PathBuf>) -> Result<i32, String> {
    let eng = open(dir)?;
    let now = chrono::Local::now().naive_local();
    println!("当前时间：{}", now.format("%Y-%m-%d %H:%M:%S"));
    println!();
    if eng.accounts().is_empty() {
        println!("还没有账号。");
        return Ok(0);
    }
    for a in eng.accounts() {
        let next = a.schedule.next_run_after(now, &a.id);
        let due = a.schedule.due_today(now, &a.id);
        println!(
            "{:<10} 下次 {}  {}",
            a.label,
            next.format("%m-%d %H:%M"),
            if due { "（今日已到点）" } else { "" }
        );
    }
    if let Some(w) = eng.next_wakeup(now) {
        println!();
        println!("下一次唤醒：{}", w.format("%Y-%m-%d %H:%M:%S"));
    }
    Ok(0)
}

fn print_help() {
    println!(
        r#"积分枢纽 PointHub v{ver} · 命令行内核

用法：phctl <命令> [选项]

命令
  doctor                       环境自检 + 平台能力矩阵
  account list                 列出账号
  account add                  添加账号
  account rm <id|别名>          删除账号
  status [--id <账号>]          查询积分与今日签到状态
  checkin [--id <账号>]         签到（不带 --id 即一键全部）
  travel [--id <账号>] [--status|--claim|--depart] [--location 1-4]
                               派猫猫旅行（仅 WorkBuddy）
  plan                         查看各账号下一次执行时间

通用选项
  --data-dir <路径>             指定数据目录（默认 %LOCALAPPDATA%\PointHub）

account add 选项
  --vendor <workbuddy|trae-cn|qoder-cn>   必填
  --label <别名>                          默认「<平台> 账号」
  --refresh-token <长效令牌>               推荐，可自续期
  --access-token <短期令牌>                没有长效令牌时用
  --device-id <设备id>                     Trae CN 必填
  --domain <域名或完整地址>                 WorkBuddy 默认 www.codebuddy.cn
  --cookie <Cookie>                        部分平台需要

示例
  phctl account add --vendor workbuddy --label 主号 --refresh-token <令牌>
  phctl status
  phctl checkin
  phctl travel --id 主号
"#,
        ver = ph_core::VERSION
    );
}
