//! 局域网互通：电脑上起一个**只对内网开放**的小服务，手机浏览器直接看账本、点签到。
//!
//! 为什么不是云端：凭据在电脑上，签到必须在电脑执行。上云等于把账号钥匙交给第三方，
//! 而需求根本不需要跨网。手机和电脑在同一个 Wi-Fi 里直连就够了。
//!
//! 安全边界（三条，缺一不可）：
//!
//! 1. **来源必须是内网地址**：服务绑在 `0.0.0.0`（家里可能有多块网卡），但每个连接
//!    先验来源 IP，公网地址一律直接断开；
//! 2. **配对码**：6 位数字、5 分钟有效、用一次就作废；配对后换成 32 字节随机 token；
//! 3. **只读开关**：关掉「允许手机签到」后，手机只能看，`/api/checkin` 直接 403。
//!
//! 没有做 HMAC 防重放：token 只在内网、只走 HTTP、且服务被内网来源限制卡死，
//! 上 HMAC 增加的是复杂度而不是安全性。真要在意，应该整体上 TLS，那是另一件事。

use rand::Rng;
use serde::Serialize;
use serde_json::{json, Value};
use std::net::{IpAddr, SocketAddr};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tauri::{AppHandle, Manager};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

use crate::SharedState;

const PAGE: &str = include_str!("../assets/lan.html");
/// 配对码有效期。
const PAIR_TTL: Duration = Duration::from_secs(300);
/// 请求体上限：手机端只发很小的 JSON。
const MAX_BODY: usize = 64 * 1024;
/// 单次读超时：局域网里不该有慢请求。
const READ_TIMEOUT: Duration = Duration::from_secs(6);

struct PairCode {
    code: String,
    expires_at: Instant,
}

/// 服务运行期的共享状态。
pub struct LanCtx {
    app: AppHandle,
    /// 已配对设备的 token（内存里，重启后需要重新配对）。
    tokens: Mutex<Vec<String>>,
    pair: Mutex<PairCode>,
    pub port: u16,
}

impl LanCtx {
    /// 当前有效的配对码；过期了就顺手换一个新的。
    pub fn current_code(&self) -> String {
        let mut p = self.pair.lock().expect("配对码锁");
        if Instant::now() >= p.expires_at {
            p.code = six_digits();
            p.expires_at = Instant::now() + PAIR_TTL;
        }
        p.code.clone()
    }

    pub fn code_left_secs(&self) -> u64 {
        let p = self.pair.lock().expect("配对码锁");
        p.expires_at.saturating_duration_since(Instant::now()).as_secs()
    }

    /// 校验并作废配对码：一旦配对成功就换新码，防止同一串被重放。
    fn take_code(&self, given: &str) -> bool {
        let mut p = self.pair.lock().expect("配对码锁");
        let ok = Instant::now() < p.expires_at && p.code == given.trim();
        if ok {
            p.code = six_digits();
            p.expires_at = Instant::now() + PAIR_TTL;
        }
        ok
    }

    fn issue_token(&self) -> String {
        let token = random_hex(32);
        let mut t = self.tokens.lock().expect("token 锁");
        t.push(token.clone());
        // 只留最近 16 台，够用且不会无限涨
        if t.len() > 16 {
            let drop = t.len() - 16;
            t.drain(0..drop);
        }
        token
    }

    fn token_ok(&self, token: &str) -> bool {
        let t = self.tokens.lock().expect("token 锁");
        t.iter().any(|x| x == token)
    }
}

fn six_digits() -> String {
    format!("{:06}", rand::thread_rng().gen_range(0..1_000_000))
}

fn random_hex(bytes: usize) -> String {
    let mut rng = rand::thread_rng();
    (0..bytes).map(|_| format!("{:02x}", rng.gen::<u8>())).collect()
}

/// 只允许内网来源：即使机器有公网 IP，也不该被外面访问到。
fn is_private(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => v4.is_private() || v4.is_loopback() || v4.is_link_local(),
        IpAddr::V6(v6) => v6.is_loopback(),
    }
}

/// 本机在局域网里的地址（给界面显示，方便手机输入）。
pub fn lan_ip() -> Option<String> {
    let listener = std::net::UdpSocket::bind("0.0.0.0:0").ok()?;
    // 连一个不存在的内网地址只是为了问路由表「我该从哪块网卡出去」，不会真的发包
    listener.connect("192.168.255.255:1").ok()?;
    let ip = listener.local_addr().ok()?.ip();
    if ip.is_loopback() || ip.is_unspecified() {
        None
    } else {
        Some(ip.to_string())
    }
}

/// 诊断日志：发布版没有控制台，只能落文件。
fn log(app: &AppHandle, msg: &str) {
    let dir = app.state::<SharedState>().data_dir.join(ph_core::files::LOGS_DIR);
    if std::fs::create_dir_all(&dir).is_err() {
        return;
    }
    use std::io::Write;
    if let Ok(mut f) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(dir.join("lan.log"))
    {
        let _ = writeln!(f, "[{}] {}", chrono::Local::now().format("%m-%d %H:%M:%S"), msg);
    }
}

// ------------------------------------------------------------------ 启动

/// 启动服务。返回上下文供界面查询配对码。
pub fn start(app: AppHandle) -> Result<Arc<LanCtx>, String> {
    // 开关与端口在 run() 时就从设置同步进原子变量了，这里不必再异步拿 engine
    let (enabled, port) = {
        use std::sync::atomic::Ordering;
        let st = app.state::<SharedState>();
        (
            st.sync_enabled.load(Ordering::Relaxed),
            st.sync_port.load(Ordering::Relaxed),
        )
    };
    if !enabled {
        return Err("局域网访问未开启".into());
    }

    let ctx = Arc::new(LanCtx {
        app,
        tokens: Mutex::new(Vec::new()),
        pair: Mutex::new(PairCode {
            code: six_digits(),
            expires_at: Instant::now() + PAIR_TTL,
        }),
        port,
    });

    let ctx2 = ctx.clone();
    tauri::async_runtime::spawn(async move {
        let addr = format!("0.0.0.0:{}", ctx2.port);
        let listener = match TcpListener::bind(&addr).await {
            Ok(l) => l,
            Err(e) => {
                log(&ctx2.app, &format!("监听 {addr} 失败：{e}"));
                return;
            }
        };
        let shown = lan_ip()
            .map(|ip| format!("http://{ip}:{}", ctx2.port))
            .unwrap_or_else(|| format!("端口 {}", ctx2.port));
        log(
            &ctx2.app,
            &format!(
                "手机访问地址：{shown}  配对码 {}（5 分钟内有效）",
                ctx2.current_code()
            ),
        );

        loop {
            let (stream, peer) = match listener.accept().await {
                Ok(v) => v,
                Err(_) => continue,
            };
            let ctx3 = ctx2.clone();
            tokio::spawn(async move {
                handle(stream, ctx3, peer).await;
            });
        }
    });

    Ok(ctx)
}

// ------------------------------------------------------------------ HTTP

struct Request {
    method: String,
    path: String,
    auth: String,
    body: Vec<u8>,
}

fn find(hay: &[u8], needle: &[u8]) -> Option<usize> {
    hay.windows(needle.len()).position(|w| w == needle)
}

async fn read_request(stream: &mut TcpStream) -> Option<Request> {
    let mut buf: Vec<u8> = Vec::new();
    let mut tmp = [0u8; 4096];
    let head_end = loop {
        if let Some(p) = find(&buf, b"\r\n\r\n") {
            break p;
        }
        if buf.len() > MAX_BODY {
            return None;
        }
        let n = match tokio::time::timeout(READ_TIMEOUT, stream.read(&mut tmp)).await {
            Ok(Ok(0)) | Ok(Err(_)) | Err(_) => return None,
            Ok(Ok(n)) => n,
        };
        buf.extend_from_slice(&tmp[..n]);
    };

    let head = String::from_utf8_lossy(&buf[..head_end]).to_string();
    let mut lines = head.lines();
    let mut first = lines.next()?.split_whitespace();
    let method = first.next()?.to_string();
    let path = first.next()?.split('?').next()?.to_string();

    let mut auth = String::new();
    let mut length = 0usize;
    for line in lines {
        let Some((k, v)) = line.split_once(':') else {
            continue;
        };
        let key = k.trim().to_ascii_lowercase();
        let val = v.trim();
        if key == "authorization" {
            auth = val.strip_prefix("Bearer ").unwrap_or(val).to_string();
        } else if key == "content-length" {
            length = val.parse().unwrap_or(0);
        }
    }
    if length > MAX_BODY {
        return None;
    }

    // 头部可能已经把 body 一起读进来了
    let mut body = buf[head_end + 4..].to_vec();
    while body.len() < length {
        let n = match tokio::time::timeout(READ_TIMEOUT, stream.read(&mut tmp)).await {
            Ok(Ok(0)) | Ok(Err(_)) | Err(_) => break,
            Ok(Ok(n)) => n,
        };
        body.extend_from_slice(&tmp[..n]);
    }
    body.truncate(length);

    Some(Request {
        method,
        path,
        auth,
        body,
    })
}

async fn respond(stream: &mut TcpStream, status: u16, ctype: &str, body: &[u8]) {
    let reason = match status {
        200 => "OK",
        400 => "Bad Request",
        401 => "Unauthorized",
        403 => "Forbidden",
        404 => "Not Found",
        405 => "Method Not Allowed",
        500 => "Internal Server Error",
        _ => "OK",
    };
    let head = format!(
        "HTTP/1.1 {status} {reason}\r\n\
         Content-Type: {ctype}\r\n\
         Content-Length: {}\r\n\
         Cache-Control: no-store\r\n\
         X-Content-Type-Options: nosniff\r\n\
         Connection: close\r\n\r\n",
        body.len()
    );
    let _ = stream.write_all(head.as_bytes()).await;
    let _ = stream.write_all(body).await;
    let _ = stream.flush().await;
}

async fn json(stream: &mut TcpStream, status: u16, v: &Value) {
    let body = serde_json::to_vec(v).unwrap_or_else(|_| b"{}".to_vec());
    respond(stream, status, "application/json; charset=utf-8", &body).await;
}

async fn handle(mut stream: TcpStream, ctx: Arc<LanCtx>, peer: SocketAddr) {
    if !is_private(peer.ip()) {
        // 局域网之外来的：连响应都不给，直接断
        return;
    }
    let Some(req) = read_request(&mut stream).await else {
        return;
    };

    match (req.method.as_str(), req.path.as_str()) {
        ("GET", "/") | ("GET", "/index.html") => {
            respond(&mut stream, 200, "text/html; charset=utf-8", PAGE.as_bytes()).await;
            return;
        }
        ("GET", "/api/ping") => {
            json(
                &mut stream,
                200,
                &json!({
                    "app": "pointhub",
                    "version": ph_core::VERSION,
                    "paired": false,
                }),
            )
            .await;
            return;
        }
        ("POST", "/api/pair") => {
            let body: Value = serde_json::from_slice(&req.body).unwrap_or(json!({}));
            let code = body.get("code").and_then(|c| c.as_str()).unwrap_or("");
            if !ctx.take_code(code) {
                json(&mut stream, 400, &json!({ "error": "配对码不对或已过期，回电脑上再看一眼" })).await;
                return;
            }
            let token = ctx.issue_token();
            log(
                &ctx.app,
                &format!(
                    "手机 {} 配对成功，新配对码 {}",
                    peer.ip(),
                    ctx.current_code()
                ),
            );
            json(&mut stream, 200, &json!({ "token": token })).await;
            return;
        }
        _ => {}
    }

    // 以下都要鉴权
    if !ctx.token_ok(&req.auth) {
        json(&mut stream, 401, &json!({ "error": "没有配对，或者配对被重置了" })).await;
        return;
    }

    match (req.method.as_str(), req.path.as_str()) {
        ("GET", "/api/state") => {
            let v = build_state(&ctx.app).await;
            json(&mut stream, 200, &v).await;
        }
        ("POST", "/api/checkin") => {
            let allow = {
                let state = ctx.app.state::<SharedState>();
                let engine = state.engine.lock().await;
                engine.settings().sync.allow_remote_checkin
            };
            if !allow {
                json(&mut stream, 403, &json!({ "error": "电脑上关掉了「允许手机签到」" })).await;
                return;
            }
            match crate::commands::run_checkin_inner(&ctx.app, None).await {
                Ok(outcomes) => {
                    let results: Vec<Value> = outcomes
                        .iter()
                        .map(|o| {
                            json!({
                                "label": o.label,
                                "status": o.status,
                                "status_text": o.status_text,
                            })
                        })
                        .collect();
                    json(&mut stream, 200, &json!({ "results": results })).await;
                }
                Err(e) => json(&mut stream, 500, &json!({ "error": e })).await,
            }
        }
        _ => json(&mut stream, 404, &json!({ "error": "没有这个接口" })).await,
    }
}

// ------------------------------------------------------------------ 视图

#[derive(Serialize)]
struct LanAccount {
    id: String,
    label: String,
    balance: Option<f64>,
    checked_in_today: Option<bool>,
    streak_days: Option<i64>,
    today_gain: Option<f64>,
    last_error: Option<String>,
}

/// 手机要的那部分状态。
///
/// 刻意不返回 `StateView`：里面有数据目录、悬浮球、托盘之类的本机信息，
/// 手机端一个都用不上，少给就少一分泄露面。
async fn build_state(app: &AppHandle) -> Value {
    let state = app.state::<SharedState>();
    let engine = state.engine.lock().await;
    let view = crate::commands::build_state(&engine);

    let accounts: Vec<LanAccount> = view
        .accounts
        .iter()
        .map(|a| LanAccount {
            id: a.id.clone(),
            label: a.label.clone(),
            balance: a.balance,
            checked_in_today: a.checked_in_today,
            streak_days: a.streak_days,
            today_gain: a.today_gain,
            last_error: a.last_error.clone(),
        })
        .collect();

    let checked_in = accounts
        .iter()
        .filter(|a| a.checked_in_today == Some(true))
        .count();
    let can_checkin = engine.settings().sync.allow_remote_checkin;

    json!({
        "version": view.version,
        "accounts": accounts,
        "total_balance": view.aggregate.total_balance,
        "checked_in": checked_in,
        "can_checkin": can_checkin,
        "next_run_at": view.next_run_at,
    })
}
