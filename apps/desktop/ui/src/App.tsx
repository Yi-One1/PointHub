import React, { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { api } from "./api";
import { BRAND_MARK, BRAND_MARK_DARK } from "./assets";
import { useTheme } from "./theme";
import { PointsIcon } from "./PointsIcon";
import { VendorIcon } from "./VendorIcon";
import type {
  AccountView,
  ClientStatus,
  DayPoint,
  LanInfo,
  LocalImportStatus,
  Settings,
  StateView,
  ThemeMode,
  UpdateCheck,
} from "./types";

type Tab = "overview" | "accounts" | "settings";

export default function App() {
  const [view, setView] = useState<StateView | null>(null);
  const [tab, setTab] = useState<Tab>("overview");
  const [busy, setBusy] = useState<string | null>(null);
  const [toasts, setToasts] = useState<{ id: number; text: string; kind: "ok" | "warn" }[]>([]);
  const toastSeq = useRef(0);

  const notify = useCallback((text: string, kind: "ok" | "warn" = "ok") => {
    const id = ++toastSeq.current;
    setToasts((t) => [...t, { id, text, kind }]);
    setTimeout(() => setToasts((t) => t.filter((x) => x.id !== id)), 4200);
  }, []);

  // 首屏：拉一次状态
  useEffect(() => {
    api
      .bootstrap()
      .then(setView)
      .catch((e) => notify(`初始化失败：${e}`, "warn"));
  }, [notify]);

  // 后台调度或悬浮球改了东西时，后端会推一条事件；收到就重拉，不做轮询。
  useEffect(() => {
    let unlisten: (() => void) | undefined;
    let unlistenLogin: (() => void) | undefined;
    let alive = true;
    (async () => {
      try {
        const { listen } = await import("@tauri-apps/api/event");
        const un = await listen("ph://changed", () => {
          api.bootstrap().then(setView).catch(() => undefined);
        });
        if (alive) unlisten = un;
        else un();

        // 浏览器登录导入的结果：后端已经建好账号，这里只负责把它说出来。
        const unLogin = await listen<string>("ph://login", (e) => {
          const msg = e.payload ?? "浏览器登录完成";
          const bad = msg.includes("失效") || msg.includes("失败") || msg.includes("超时");
          notify(msg, bad ? "warn" : "ok");
          api.bootstrap().then(setView).catch(() => undefined);
        });
        if (alive) unlistenLogin = unLogin;
        else unLogin();

      } catch {
        /* 纯浏览器预览时没有 tauri 事件，忽略即可 */
      }
    })();
    return () => {
      alive = false;
      unlisten?.();
      unlistenLogin?.();
    };
  }, [notify]);

  // 主题：settings.theme 决定；system 时跟随系统并监听变化
  useTheme(view?.settings.theme);

  // 远程模式下定期从电脑拉一次数据，让积分「实时」可见。
  // 只在页面可见时拉，后台不空转。
  useEffect(() => {
    if (view?.mode !== "remote") return;
    const timer = setInterval(() => {
      if (document.hidden) return;
      api.bootstrap().then(setView).catch(() => undefined);
    }, 30_000);
    return () => clearInterval(timer);
  }, [view?.mode]);

  const platformCount = useMemo(
    () => view?.platforms.reduce((n, p) => n + (p.checkin ? 1 : 0), 0) ?? 0,
    [view]
  );

  const run = async (key: string, fn: () => Promise<void>) => {
    setBusy(key);
    try {
      await fn();
    } catch (e) {
      notify(String(e), "warn");
    } finally {
      setBusy(null);
    }
  };

  const refresh = () =>
    run("refresh", async () => {
      const v = await api.refreshAll();
      setView(v);
      notify(`已刷新 ${v.accounts.length} 个账号`);
    });

  const checkinAll = () =>
    run("checkin-all", async () => {
      const outcomes = await api.checkin();
      setView(await api.bootstrap());
      const claimed = outcomes.filter((o) => o.status === "claimed").length;
      const failed = outcomes.filter((o) => o.status === "failed" || o.status === "authfailed").length;
      notify(
        failed > 0 ? `领取 ${claimed} 个，${failed} 个需要处理` : `领取完成，新到 ${claimed} 个账号的积分`,
        failed > 0 ? "warn" : "ok"
      );
    });

  const checkinOne = (a: AccountView) =>
    run(`checkin-${a.id}`, async () => {
      const [o] = await api.checkin([a.id]);
      setView(await api.bootstrap());
      if (o) notify(`${a.label}：${o.status_text} · ${o.message}`, o.status === "failed" ? "warn" : "ok");
    });

  const travel = (a: AccountView) =>
    run(`travel-${a.id}`, async () => {
      const msg = await api.travel(a.id, "auto");
      notify(`${a.label}：${msg}`);
      setView(await api.refreshAll());
    });

  const toggle = (a: AccountView, enabled: boolean) =>
    run(`toggle-${a.id}`, async () => {
      await api.setAccountEnabled(a.id, enabled);
      setView(await api.bootstrap());
    });

  const saveSettings = (next: Settings) =>
    run("settings", async () => {
      const v = await api.saveSettings(next);
      setView(v);
      notify("设置已保存");
    });

  if (!view) {
    return (
      <div className="boot">
        <img className="boot-mark mark-light" src={BRAND_MARK} alt="" />
        <img className="boot-mark mark-dark" src={BRAND_MARK_DARK} alt="" />
        <div>正在读取账号数据…</div>
      </div>
    );
  }

  return (
    <div className="app">
      <aside className="rail">
        <div className="brand">
          <img className="mark mark-light" src={BRAND_MARK} alt="" aria-hidden />
          <img className="mark mark-dark" src={BRAND_MARK_DARK} alt="" aria-hidden />
          <div>
            <div className="bname">积分枢纽</div>
            <div className="bsub">POINTHUB</div>
          </div>
        </div>

        <nav>
          <NavItem active={tab === "overview"} onClick={() => setTab("overview")} label="总览" />
          <NavItem
            active={tab === "accounts"}
            onClick={() => setTab("accounts")}
            label="账号"
            badge={view.accounts.length}
          />
          <NavItem active={tab === "settings"} onClick={() => setTab("settings")} label="设置" />
        </nav>

        <div className="afoot">
          数据只在本机
          <br />
          {view.vault_kind}
          <br />
          v{view.version} · {platformCount} 个平台
        </div>
      </aside>

      <main>
        {tab === "overview" && (
          <Overview
            view={view}
            busy={busy}
            onRefresh={refresh}
            onCheckinAll={checkinAll}
            onCheckinOne={checkinOne}
            onTravel={travel}
            onToggle={toggle}
            onAdd={() => setTab("accounts")}
          />
        )}

        {tab === "accounts" && (
          <AccountsPage
            view={view}
            busy={busy}
            onChanged={setView}
            notify={notify}
            onOpenSettings={() => setTab("settings")}
          />
        )}

        {tab === "settings" && (
          <SettingsPage view={view} busy={busy} onSave={saveSettings} notify={notify} />
        )}
      </main>

      <div className="toast-stack">
        {toasts.map((t) => (
          <div key={t.id} className={`toast toast-${t.kind}`}>
            {t.text}
          </div>
        ))}
      </div>
    </div>
  );
}

// ------------------------------------------------------------------ 总览

function Overview({
  view,
  busy,
  onRefresh,
  onCheckinAll,
  onCheckinOne,
  onTravel,
  onToggle,
  onAdd,
}: {
  view: StateView;
  busy: string | null;
  onRefresh: () => void;
  onCheckinAll: () => void;
  onCheckinOne: (a: AccountView) => void;
  onTravel: (a: AccountView) => void;
  onToggle: (a: AccountView, enabled: boolean) => void;
  onAdd: () => void;
}) {
  const enabled = view.accounts.filter((a) => a.enabled);
  const done = enabled.filter((a) => a.checked_in_today === true).length;
  const pending = Math.max(enabled.length - done, 0);
  const pct = enabled.length === 0 ? 0 : Math.round((done / enabled.length) * 100);
  // 「今日新增」与趋势图同源：都是余额差，避免两个数字互相打脸。
  const todayGain = view.trend.length > 0 ? view.trend[view.trend.length - 1].gain : null;

  // 按平台分组，顺序跟随能力表——平台顺序是产品决定，不该被账号加入顺序带跑。
  const groups = useMemo(() => {
    const map = new Map<string, AccountView[]>();
    for (const a of view.accounts) {
      const list = map.get(a.vendor);
      if (list) list.push(a);
      else map.set(a.vendor, [a]);
    }
    const order = view.platforms.map((p) => p.vendor);
    const keys = [...order.filter((v) => map.has(v)), ...[...map.keys()].filter((v) => !order.includes(v))];
    return keys.map((v) => {
      const list = map.get(v)!;
      const cap = view.platforms.find((p) => p.vendor === v);
      return {
        vendor: v,
        display: cap?.display ?? list[0].vendor_display,
        verified: cap?.verified ?? false,
        accounts: list,
        subtotal: list.reduce((n, a) => n + (a.balance ?? 0), 0),
        unknown: list.filter((a) => a.balance === null).length,
      };
    });
  }, [view.accounts, view.platforms]);

  const next = nextRun(view.next_run_at, view.settings.scheduler_enabled);
  const lastSample = view.aggregate.last_sampled_at ? clock(view.aggregate.last_sampled_at) : null;

  return (
    <>
      <div className="head">
        <div>
          <h1>总览</h1>
          <p className="sub">
            {view.mode === "remote" && (
              <span className="tag tag-muted">远程 · {view.remote?.server_name ?? "电脑"}</span>
            )}
            {view.mode === "remote" && view.remote?.error
              ? ` 连接异常：${view.remote.error}`
              : enabled.length === 0
                ? "还没有账号，去「账号」页导入一个"
                : `今日已签 ${done} / ${enabled.length}${lastSample ? ` · 最近采样 ${lastSample}` : ""}`}
          </p>
        </div>
        <div className="acts">
          <button className="btn" onClick={onRefresh} disabled={busy !== null}>
            {busy === "refresh" ? "刷新中…" : "刷新积分"}
          </button>
          <button className="btn primary" onClick={onCheckinAll} disabled={busy !== null || enabled.length === 0}>
            {busy === "checkin-all" ? "签到中…" : "一键全部签到"}
          </button>
        </div>
      </div>

      <div className="kpis">
        <div className="kpi">
          <div className="klabel">积分合计</div>
          <div className="kval">
            {credits(view.aggregate.total_balance)}
            <PointsIcon tone="bronze" size={18} />
          </div>
          <div className="prog" role="progressbar" aria-valuenow={pct} aria-valuemin={0} aria-valuemax={100}>
            <i style={{ width: `${pct}%` }} />
          </div>
        </div>
        <div className="kpi">
          <div className="klabel">今日已签</div>
          <div className="kval">
            {done}
            <small>/ {enabled.length}</small>
          </div>
          <div className="kfoot">{pending > 0 ? `${pending} 个待签` : "全部完成"}</div>
        </div>
        <div className="kpi">
          <div className="klabel">今日新增</div>
          <div className="kval">
            {todayGain === null ? (
              "—"
            ) : (
              <>
                {todayGain >= 0 ? "+" : ""}
                {credits(todayGain)}
                <PointsIcon tone="jade" size={18} />
              </>
            )}
          </div>
          <div className="kfoot">较昨日余额</div>
        </div>
        <div className="kpi">
          <div className="klabel">下一次自动</div>
          <div className="kval">
            {next.value}
            {next.small && <small>{next.small}</small>}
          </div>
          <div className="kfoot">{next.foot}</div>
        </div>
      </div>

      {view.accounts.length === 0 ? (
        <div className="empty">
          <div className="empty-title">还没有纳管任何账号</div>
          <p className="empty-text">
            {view.mode === "remote"
              ? "电脑上还没有账号。先在电脑端导入，手机这边会自动同步过来。"
              : "导出一个长效令牌就能开始。令牌会以当前 Windows 用户身份加密保存，不会离开这台电脑。"}
          </p>
          {view.mode !== "remote" && (
            <button className="btn primary" onClick={onAdd}>
              去添加账号
            </button>
          )}
        </div>
      ) : (
        <div className="panels">
          {groups.map((g) => (
            <section className="panel" key={g.vendor}>
              <div className="phead">
                <VendorIcon vendor={g.vendor} size={20} />
                <span className="pname">{g.display}</span>
                {!g.verified && <span className="tag tag-muted">端点待实测</span>}
                <span className="psub">
                  {g.accounts.length} 账号 · 小计 {credits(g.subtotal)}
                  <PointsIcon tone="ink" size={8} />
                  {g.unknown > 0 ? ` · ${g.unknown} 个未知` : ""}
                </span>
              </div>

              {g.accounts.map((a) => {
                const st = statusOf(a);
                const working = busy === `checkin-${a.id}` || busy === `travel-${a.id}`;
                return (
                  <div className="arow" key={a.id}>
                    <div className="acell">
                      <div className="rname">
                        {a.label}
                        {!a.enabled && <span className="tag tag-muted">已停用</span>}
                        {!a.has_credential && <span className="tag tag-warn">缺凭据</span>}
                      </div>
                      <div className="rhint" title={a.schedule_text}>
                        {a.hint}
                        {a.streak_days !== null && a.streak_days > 0 ? ` · 连签 ${a.streak_days} 天` : ""}
                      </div>
                    </div>

                    <div className="rnum">
                      {a.balance === null ? (
                        <span className="rnum-none">—</span>
                      ) : (
                        <>
                          {credits(a.balance)}
                          <PointsIcon tone="jade" size={13} />
                        </>
                      )}
                    </div>

                    <span className={`pill pill-${st.kind}`} title={a.last_error ?? st.hint}>
                      <i />
                      {st.text}
                    </span>

                    <div className="tf">
                      {a.can_travel && (
                        <button
                          className="go"
                          onClick={() => onTravel(a)}
                          disabled={working || !a.has_credential || busy !== null}
                        >
                          派猫猫
                        </button>
                      )}
                      <button
                        className={`go ${a.can_checkin ? "solid" : ""}`}
                        onClick={() => onCheckinOne(a)}
                        disabled={working || busy !== null || !a.has_credential || !a.can_checkin}
                        title={a.can_checkin ? a.schedule_text : "该平台签到端点待实测"}
                      >
                        {working ? "处理中" : a.can_checkin ? "签到" : "待实测"}
                      </button>
                      <button
                        className={`switch mini ${a.enabled ? "on" : ""}`}
                        role="switch"
                        aria-checked={a.enabled}
                        aria-label={`启用 ${a.label}`}
                        onClick={() => onToggle(a, !a.enabled)}
                        style={view.mode === "remote" ? { display: "none" } : undefined}
                      >
                        <span />
                      </button>
                    </div>
                  </div>
                );
              })}
            </section>
          ))}
        </div>
      )}

      <div className="bottom">
        <TrendCard trend={view.trend} />
        <ActivityCard events={view.events} />
      </div>
    </>
  );
}

function TrendCard({ trend }: { trend: DayPoint[] }) {
  // 用「每日新增」而不是「余额合计」：余额曲线七天内几乎是条直线，
  // 画出来像装饰；新增才看得出哪天真的领到了。
  const gains = trend.map((t) => t.gain);
  const max = Math.max(...gains.map((g) => Math.abs(g ?? 0)), 1);
  const hasAny = gains.some((g) => g !== null && g !== 0);
  const [hover, setHover] = useState<number | null>(null);
  const cur = hover !== null ? trend[hover] : null;

  return (
    <div className="bcard">
      <div className="btitle">
        <span>近 7 日每日新增</span>
        {cur && (
          <span className="btip">
            {cur.date}
            {cur.gain === null ? (
              " · 无对比"
            ) : (
              <>
                {" · 新增 "}
                {cur.gain >= 0 ? "+" : ""}
                {credits(cur.gain)}
                <PointsIcon tone="jade" size={8} />
              </>
            )}
            {" · 合计 "}
            {credits(cur.total)}
            <PointsIcon tone="ink" size={8} />
          </span>
        )}
      </div>
      {!hasAny ? (
        <div className="ev-empty">还不到两天数据，攒够两天才有对比</div>
      ) : (
        <div className="bars">
          {trend.map((t, i) => {
            const g = t.gain;
            const h = g === null || g === 0 ? 3 : Math.max(6, Math.round((Math.abs(g) / max) * 56));
            return (
              <div
                key={t.date}
                title={`${t.date} · ${g === null ? "无对比" : `${g >= 0 ? "+" : ""}${g}`} · 合计 ${t.total}`}
                onMouseEnter={() => setHover(i)}
                onMouseLeave={() => setHover(null)}
              >
                <i style={{ height: `${h}px` }} data-empty={g === null || g === 0 ? "1" : "0"} />
                <span>{i === trend.length - 1 ? "今日" : t.date}</span>
              </div>
            );
          })}
        </div>
      )}
    </div>
  );
}

function ActivityCard({ events }: { events: StateView["events"] }) {
  const list = events.slice(0, 4);
  return (
    <div className="bcard">
      <div className="btitle">最近活动</div>
      {list.length === 0 ? (
        <div className="ev-empty">还没有签到记录，跑一次就有</div>
      ) : (
        list.map((e, i) => (
          <div className="ev" key={`${e.at}-${e.account_id}-${i}`}>
            <b>{e.label}</b>
            <span className="ev-text">{e.text}</span>
            {e.delta !== null && e.delta > 0 && (
              <span className="ev-delta">
                +{e.delta}
                <PointsIcon tone="jade" size={8} />
              </span>
            )}
            <time>{clock(e.at)}</time>
          </div>
        ))
      )}
    </div>
  );
}

function statusOf(a: AccountView): { kind: string; text: string; hint: string } {
  if (!a.enabled) return { kind: "idle", text: "已停用", hint: "该账号已停用，不会自动签到" };
  if (!a.has_credential) return { kind: "idle", text: "缺凭据", hint: "还没有导入令牌" };
  if (a.last_error) return { kind: "err", text: "上次失败", hint: a.last_error };
  // 能力不支持时不给「待签到」——那会让人一直等一个不会发生的动作。
  if (!a.can_checkin) return { kind: "idle", text: "未接入", hint: "该平台签到端点待实测" };
  if (a.checked_in_today === true) return { kind: "ok", text: "今日已签", hint: "今天已领到" };
  if (a.checked_in_today === false) return { kind: "todo", text: "待签到", hint: "今天还没领" };
  return { kind: "idle", text: "未采样", hint: "还没有拉到余额" };
}

function nextRun(ts: number | null, enabled: boolean): { value: string; small: string; foot: string } {
  if (ts === null) {
    return enabled
      ? { value: "—", small: "", foot: "没有开启签到的账号" }
      : { value: "已关闭", small: "", foot: "设置里可开启定时签到" };
  }
  const d = new Date(ts * 1000);
  const hh = String(d.getHours()).padStart(2, "0");
  const mm = String(d.getMinutes()).padStart(2, "0");
  const today = new Date();
  const tomorrow = new Date(today.getTime());
  tomorrow.setDate(today.getDate() + 1);
  const sameDay = d.toDateString() === today.toDateString();
  const nextDay = d.toDateString() === tomorrow.toDateString();
  const label = sameDay ? "今天" : nextDay ? "明天" : `${d.getMonth() + 1}/${d.getDate()}`;
  if (!enabled) {
    // 关掉定时也别藏着时间：用户需要知道「打开后几点跑」。
    return { value: "已关闭", small: "", foot: `开启后 ${label} ${hh}:${mm}` };
  }
  return { value: `${hh}:${mm}`, small: label, foot: `${label} ${hh}:${mm} 起` };
}

// ------------------------------------------------------------------ 账号页

function AccountsPage({
  view,
  busy,
  onChanged,
  notify,
  onOpenSettings,
}: {
  view: StateView;
  busy: string | null;
  onChanged: (v: StateView) => void;
  notify: (t: string, k?: "ok" | "warn") => void;
  onOpenSettings: () => void;
}) {
  const [vendor, setVendor] = useState("workbuddy");
  const [mode, setMode] = useState<"browser" | "local" | "token">("browser");
  const isPhone = /Android|iPhone|iPad|iPod/i.test(navigator.userAgent);
  const [local, setLocal] = useState<LocalImportStatus[]>([]);
  const [importing, setImporting] = useState(false);
  const [label, setLabel] = useState("");
  const [secret, setSecret] = useState("");
  const [kind, setKind] = useState<"refresh" | "access">("refresh");
  const [deviceId, setDeviceId] = useState("");
  const [domain, setDomain] = useState("");
  const [target, setTarget] = useState<AccountView | null>(null);
  const [opening, setOpening] = useState(false);
  // 授权链接：浏览器没被自动拉起来时的兜底入口
  const [authUrl, setAuthUrl] = useState("");

  // 后端把授权链接推过来，浏览器被拦或用户想换设备打开时还能手动复制
  useEffect(() => {
    let un: (() => void) | undefined;
    let alive = true;
    (async () => {
      try {
        const { listen } = await import("@tauri-apps/api/event");
        const fn = await listen<string>("ph://auth-url", (e) => setAuthUrl(e.payload ?? ""));
        if (alive) un = fn;
        else fn();
      } catch {
        /* 纯浏览器预览时没有 tauri 事件 */
      }
    })();
    return () => {
      alive = false;
      un?.();
    };
  }, []);

  const cap = view.platforms.find((p) => p.vendor === vendor);
  const localStatus = local.find((l) => l.vendor === vendor);

  // 本机导入只在电脑上探测；手机上没有客户端，也不该多一次 IPC。
  useEffect(() => {
    if (isPhone) return;
    let alive = true;
    api
      .localImportScan()
      .then((rows) => alive && setLocal(rows))
      .catch(() => alive && setLocal([]));
    return () => {
      alive = false;
    };
  }, [isPhone]);

  const startLocalImport = async () => {
    setImporting(true);
    try {
      const msg = await api.localImport(vendor);
      onChanged(await api.bootstrap());
      notify(msg);
    } catch (e) {
      notify(String(e), "warn");
    } finally {
      setImporting(false);
    }
  };

  const startBrowserLogin = async () => {
    setOpening(true);
    setAuthUrl("");
    try {
      await api.browserLogin(vendor);
      notify(`已用你的浏览器打开 ${cap?.display ?? vendor} 授权页，授权完成后会自动导入`);
    } catch (e) {
      notify(String(e), "warn");
    } finally {
      setOpening(false);
    }
  };

  const submit = async () => {
    try {
      const msg = await api.addAccount({
        vendor,
        label: label || `${cap?.display ?? vendor} 账号`,
        secret,
        secret_kind: kind,
        device_id: deviceId || undefined,
        domain: domain || undefined,
      });
      setLabel("");
      setSecret("");
      setDeviceId("");
      setDomain("");
      onChanged(await api.bootstrap());
      notify(msg);
    } catch (e) {
      notify(String(e), "warn");
    }
  };

  const remove = async (a: AccountView) => {
    try {
      await api.removeAccount(a.id);
      setTarget(null);
      onChanged(await api.bootstrap());
      notify(`已删除 ${a.label}`);
    } catch (e) {
      notify(String(e), "warn");
    }
  };

  if (view.mode === "remote") {
    return (
      <>
        <div className="head">
          <div>
            <h1>账号</h1>
            <p className="sub">远程模式：账号、凭据和签到都在电脑上</p>
          </div>
        </div>

        <div className="empty">
          <div className="empty-title">账号由电脑管理</div>
          <p className="empty-text">
            在电脑端的「账号」页导入、删除或停用账号；手机这边只负责实时查看和远程签到。
          </p>
        </div>
      </>
    );
  }

  return (
    <>
      <div className="head">
        <div>
          <h1>账号</h1>
          <p className="sub">同一平台下可以有多个账号，各自独立定时、独立凭据</p>
        </div>
      </div>

      <div className="split">
        <section className="panel">
          <div className="phead">
            <span className="pname">已纳管</span>
            <span className="psub">{view.accounts.length} 个</span>
          </div>

          {view.accounts.length === 0 ? (
            <p className="panel-empty">还没有账号。右侧导入第一个。</p>
          ) : (
            <ul className="acct-list">
              {view.accounts.map((a) => (
                <li key={a.id} className="acct-row">
                  <VendorIcon vendor={a.vendor} size={24} />
                  <div className="acct-main">
                    <div className="acct-title">
                      {a.label}
                      <span className="acct-vendor">{a.vendor_display}</span>
                      {!a.enabled && <span className="tag tag-muted">已停用</span>}
                      {!a.has_credential && <span className="tag tag-warn">缺凭据</span>}
                    </div>
                    <div className="acct-sub">
                      {a.hint} · {a.schedule_text}
                    </div>
                  </div>
                  <div className="acct-balance">
                    {a.balance === null ? (
                      "—"
                    ) : (
                      <>
                        {credits(a.balance)}
                        <PointsIcon tone="jade" size={12} />
                      </>
                    )}
                  </div>
                  <button className="icon-btn" title="删除" onClick={() => setTarget(a)} disabled={busy !== null}>
                    <IconTrash />
                  </button>
                </li>
              ))}
            </ul>
          )}
        </section>

        <section className="panel">
          <div className="phead">
            <span className="pname">导入账号</span>
          </div>

          <div className="pbody">
            <div className="field">
              <label>平台</label>
              <div className="seg">
                {view.platforms.map((p) => (
                  <button
                    key={p.vendor}
                    className={`seg-item ${vendor === p.vendor ? "active" : ""}`}
                    onClick={() => setVendor(p.vendor)}
                  >
                    <VendorIcon vendor={p.vendor} size={15} />
                    {p.display}
                  </button>
                ))}
              </div>
              {cap && !cap.checkin && (
                <p className="hint warn">
                  {cap.display} 当前版本只做展示，签到端点待实测。导入后不会自动签到。
                </p>
              )}
            </div>

            <div className="field">
              <label>导入方式</label>
              <div className="seg">
                <button
                  className={`seg-item ${mode === "browser" ? "active" : ""}`}
                  onClick={() => setMode("browser")}
                >
                  浏览器授权（推荐）
                </button>
                {!isPhone && (
                  <button
                    className={`seg-item ${mode === "local" ? "active" : ""}`}
                    onClick={() => setMode("local")}
                  >
                    本机导入
                  </button>
                )}
                <button className={`seg-item ${mode === "token" ? "active" : ""}`} onClick={() => setMode("token")}>
                  手动令牌
                </button>
              </div>
            </div>

            {mode === "browser" ? (
              <>
                <ol className="steps">
                  <li>点下面的按钮，会在你<strong>平时用的浏览器</strong>里新开一个标签页（不是新窗口）</li>
                  <li>登录并点「授权」，看到成功提示就可以关掉那个标签页</li>
                  <li>这里会自动导入；导入完会显示余额，确认令牌真的能用</li>
                </ol>
                <button className="btn primary block" onClick={startBrowserLogin} disabled={opening}>
                  {opening ? "正在打开…" : `用浏览器授权 ${cap?.display ?? "平台"}`}
                </button>
                {authUrl && (
                  <p className="hint">
                    浏览器没自动打开？把这行地址复制到浏览器里：
                    <br />
                    <code style={{ wordBreak: "break-all", userSelect: "all" }}>{authUrl}</code>
                  </p>
                )}
                <p className="hint">
                  走的是平台官方授权：软件只拿到一个由你本人确认过的令牌（含续期令牌），加密存在本机；
                  不读取你浏览器的任何数据，也不用另开一个专用浏览器。
                </p>
              </>
            ) : mode === "local" ? (
              <>
                {localStatus ? (
                  localStatus.available ? (
                    <>
                      <p className="hint">{localStatus.detail}</p>
                      <ol className="steps">
                        <li>只读取本机已登录客户端的登录态，<strong>不改动</strong>客户端文件</li>
                        <li>导入前会联网验证一次余额；验证不过不会留下账号</li>
                        <li>导入后由本应用负责签到，同一账号不要在两处同时签到</li>
                        <li>同一个账号重复导入会自动更新凭据，不会出现两条记录</li>
                      </ol>
                      <button
                        className="btn primary block"
                        onClick={startLocalImport}
                        disabled={importing || busy !== null}
                      >
                        {importing ? "正在导入…" : `从本机导入 ${cap?.display ?? vendor}`}
                      </button>
                    </>
                  ) : (
                    <>
                      <p className="hint warn">{localStatus.detail}</p>
                      <button className="link-btn" onClick={() => setMode("token")}>
                        改用手动令牌
                      </button>
                    </>
                  )
                ) : (
                  <p className="hint">正在检查本机客户端…</p>
                )}
              </>
            ) : (
              <>
                <div className="field">
                  <label>别名</label>
                  <input value={label} onChange={(e) => setLabel(e.target.value)} placeholder="例如：主号" />
                </div>

                <div className="field">
                  <label>凭据类型</label>
                  <div className="seg">
                    <button
                      className={`seg-item ${kind === "refresh" ? "active" : ""}`}
                      onClick={() => setKind("refresh")}
                    >
                      长效令牌（推荐）
                    </button>
                    <button className={`seg-item ${kind === "access" ? "active" : ""}`} onClick={() => setKind("access")}>
                      短期令牌
                    </button>
                  </div>
                </div>

                <div className="field">
                  <label>令牌</label>
                  <textarea
                    value={secret}
                    onChange={(e) => setSecret(e.target.value)}
                    rows={3}
                    placeholder="粘贴令牌原文；只保存在本机加密仓"
                    spellCheck={false}
                  />
                </div>

                {vendor === "trae-cn" && (
                  <div className="field">
                    <label>设备 ID（Trae 必填）</label>
                    <input
                      value={deviceId}
                      onChange={(e) => setDeviceId(e.target.value)}
                      placeholder="x-device-id 原值"
                      spellCheck={false}
                    />
                  </div>
                )}

                <div className="field">
                  <label>鉴权域名（可选）</label>
                  <input
                    value={domain}
                    onChange={(e) => setDomain(e.target.value)}
                    placeholder="默认 www.codebuddy.cn"
                    spellCheck={false}
                  />
                  <p className="hint">WorkBuddy 以登录态里的域名为准；留空用默认值。</p>
                </div>

                <button className="btn primary block" onClick={submit} disabled={!secret.trim() || busy !== null}>
                  添加
                </button>
              </>
            )}

            <button className="link-btn" onClick={onOpenSettings}>
              凭据存在哪？看设置页的说明
            </button>
          </div>
        </section>
      </div>

      {target && (
        <Modal onClose={() => setTarget(null)} title={`删除「${target.label}」`}>
          <p className="modal-text">
            这会同时删除该账号的加密凭据，且不可恢复。签到历史会保留（用于趋势统计）。
          </p>
          <div className="modal-actions">
            <button className="btn" onClick={() => setTarget(null)}>
              取消
            </button>
            <button className="btn danger" onClick={() => remove(target)}>
              确认删除
            </button>
          </div>
        </Modal>
      )}
    </>
  );
}

// ------------------------------------------------------------------ 设置页

function SettingsPage({
  view,
  busy,
  onSave,
  notify,
}: {
  view: StateView;
  busy: string | null;
  onSave: (s: Settings) => void;
  notify: (t: string, k?: "ok" | "warn") => void;
}) {
  const [draft, setDraft] = useState<Settings>(view.settings);
  const [newDir, setNewDir] = useState("");
  // 配对码会过期，隔一会儿拉一次；界面停留时保持最新
  const [lan, setLan] = useState<LanInfo | null>(null);
  // 安卓端自己就是手机，再显示「手机访问」没有意义
  const isPhone = /Android|iPhone|iPad|iPod/i.test(navigator.userAgent);

  // 远程连接状态与配对表单
  const [client, setClient] = useState<ClientStatus | null>(null);
  const [baseUrl, setBaseUrl] = useState("");
  const [code, setCode] = useState("");
  const [connecting, setConnecting] = useState(false);

  // 更新：检查结果与下载状态。来源改了要先保存，命令读的是已落盘的设置。
  const [update, setUpdate] = useState<UpdateCheck | null>(null);
  const [checking, setChecking] = useState(false);
  const [applying, setApplying] = useState(false);
  const [updateMsg, setUpdateMsg] = useState<string | null>(null);

  const checkUpdate = async () => {
    setChecking(true);
    setUpdateMsg(null);
    try {
      setUpdate(await api.updateCheck());
    } catch (e) {
      setUpdateMsg(String(e));
    } finally {
      setChecking(false);
    }
  };

  const applyUpdate = async () => {
    setApplying(true);
    setUpdateMsg(null);
    try {
      const r = await api.updateApply();
      setUpdateMsg(r.detail);
      if (r.action === "latest") setUpdate((u) => (u ? { ...u, latest: null } : u));
    } catch (e) {
      setUpdateMsg(String(e));
    } finally {
      setApplying(false);
    }
  };

  useEffect(() => {
    if (!isPhone) return;
    api
      .clientStatus()
      .then((c) => {
        setClient(c);
        if (c.base_url) setBaseUrl(c.base_url);
      })
      .catch(() => undefined);
  }, [isPhone]);

  const connect = async () => {
    setConnecting(true);
    try {
      const c = await api.clientPair(baseUrl.trim(), code.trim());
      setClient(c);
      setDraft({ ...draft, mode: "remote" });
      setCode("");
      notify(`已连接 ${c.server_name ?? "电脑"}`, "ok");
    } catch (e) {
      notify(String(e), "warn");
    } finally {
      setConnecting(false);
    }
  };

  const disconnect = async () => {
    setConnecting(true);
    try {
      const c = await api.clientUnpair();
      setClient(c);
      setDraft({ ...draft, mode: "local" });
      notify("已断开，回到本机模式", "ok");
    } catch (e) {
      notify(String(e), "warn");
    } finally {
      setConnecting(false);
    }
  };

  useEffect(() => {
    let alive = true;
    const pull = () => {
      api
        .lanInfo()
        .then((v) => {
          if (alive) setLan(v);
        })
        .catch(() => undefined);
    };
    pull();
    const timer = setInterval(pull, 5000);
    return () => {
      alive = false;
      clearInterval(timer);
    };
  }, []);

  const patch = (p: Partial<Settings>) => setDraft({ ...draft, ...p });
  const dirty = JSON.stringify(draft) !== JSON.stringify(view.settings);

  return (
    <>
      <div className="head">
        <div>
          <h1>设置</h1>
          <p className="sub">改动立即生效；带「保存」按钮的项需要确认</p>
        </div>
        <div className="acts">
          <button className="btn" onClick={() => setDraft(view.settings)} disabled={!dirty}>
            还原
          </button>
          <button className="btn primary" onClick={() => onSave(draft)} disabled={!dirty || busy !== null}>
            {busy === "settings" ? "保存中…" : "保存"}
          </button>
        </div>
      </div>

      {isPhone && (
        <section className="panel">
          <div className="phead">
            <span className="pname">连接方式</span>
            {client?.paired && <span className="pnote">已连接 {client.server_name}</span>}
          </div>

          <div className="row">
            <div>
              <div className="row-title">运行模式</div>
              <div className="row-desc">
                本机模式：账号和签到都在手机上；连接电脑：手机只做遥控器，凭据留在电脑
              </div>
            </div>
            <div className="seg">
              <button
                className={`seg-item ${draft.mode === "local" ? "active" : ""}`}
                onClick={() => patch({ mode: "local" })}
              >
                本机模式
              </button>
              <button
                className={`seg-item ${draft.mode === "remote" ? "active" : ""}`}
                onClick={() => patch({ mode: "remote" })}
              >
                连接电脑
              </button>
            </div>
          </div>

          {draft.mode === "remote" &&
            (client?.paired ? (
              <>
                <div className="row">
                  <div>
                    <div className="row-title">已连接</div>
                    <div className="row-desc">
                      {client.server_name ?? "电脑"} · {client.base_url}
                    </div>
                  </div>
                  <button className="btn" onClick={disconnect} disabled={connecting}>
                    {connecting ? "处理中…" : "断开"}
                  </button>
                </div>
                <div className="row">
                  <div className="row-desc">断开连接会同时回到本机模式；账号仍留在电脑上。</div>
                </div>
              </>
            ) : (
              <>
                <div className="field">
                  <label>电脑地址</label>
                  <input
                    value={baseUrl}
                    onChange={(e) => setBaseUrl(e.target.value)}
                    placeholder="192.168.1.5:47821"
                    spellCheck={false}
                  />
                  <p className="hint">在电脑端「设置 → 手机访问」里能看到地址。</p>
                </div>

                <div className="field">
                  <label>配对码</label>
                  <input
                    value={code}
                    onChange={(e) => setCode(e.target.value)}
                    placeholder="6 位数字"
                    inputMode="numeric"
                    maxLength={6}
                  />
                  <p className="hint">配对码 5 分钟有效，用过一次就换新码。</p>
                </div>

                <button
                  className="btn primary block"
                  onClick={connect}
                  disabled={connecting || !baseUrl.trim() || !code.trim()}
                >
                  {connecting ? "连接中…" : "连接电脑"}
                </button>
              </>
            ))}
        </section>
      )}

      {!isPhone && (
      <section className="panel">
        <div className="phead">
          <span className="pname">手机访问</span>
          {lan?.running && <span className="pnote">同一 Wi-Fi 下可用</span>}
        </div>

        <div className="row">
          <div>
            <div className="row-title">局域网访问</div>
            <div className="row-desc">
              手机和电脑连同一个 Wi-Fi，用手机浏览器打开地址即可；改了开关要重启应用才生效
            </div>
          </div>
          <Switch
            checked={draft.sync.enabled}
            onChange={(v) => patch({ sync: { ...draft.sync, enabled: v } })}
            label="局域网访问"
          />
        </div>

        {lan?.running ? (
          <>
            <div className="row">
              <div>
                <div className="row-title">访问地址</div>
                <div className="row-desc">{lan.url ?? "没取到本机内网地址，检查是否连着 Wi-Fi"}</div>
              </div>
              {lan.url && (
                <button
                  className="btn"
                  onClick={() => {
                    navigator.clipboard?.writeText(lan.url!).then(
                      () => notify("地址已复制", "ok"),
                      () => notify("复制失败，手动抄一下", "warn"),
                    );
                  }}
                >
                  复制
                </button>
              )}
            </div>

            <div className="row">
              <div>
                <div className="row-title">配对码</div>
                <div className="row-desc">
                  {`手机上输这 6 位数字即可连接；约 ${Math.max(
                    1,
                    Math.round((lan.code_left_secs || 0) / 60),
                  )} 分钟后换新码，配对过的手机不受影响`}
                </div>
              </div>
              <div className="pin">{lan.code ?? "······"}</div>
            </div>
          </>
        ) : (
          <div className="row">
            <div className="row-desc">
              {lan?.enabled
                ? `服务没起来，端口 ${lan.port} 可能被别的程序占了`
                : "局域网访问已关闭，打开开关并重启应用后生效"}
            </div>
          </div>
        )}

        <div className="row">
          <div>
            <div className="row-title">允许手机签到</div>
            <div className="row-desc">关掉后手机只能看账本，不能替你点签到</div>
          </div>
          <Switch
            checked={draft.sync.allow_remote_checkin}
            onChange={(v) => patch({ sync: { ...draft.sync, allow_remote_checkin: v } })}
            label="允许手机签到"
          />
        </div>
      </section>
      )}

      <section className="panel">
        <div className="phead">
          <span className="pname">外观</span>
        </div>

        <div className="row">
          <div>
            <div className="row-title">主题</div>
            <div className="row-desc">
              {isPhone ? "「跟随系统」会随手机的深色模式切换" : "「跟随系统」会随 Windows 的浅色/深色切换"}
            </div>
          </div>
          <div className="seg">
            {(["light", "dark", "system"] as ThemeMode[]).map((m) => (
              <button
                key={m}
                className={`seg-item ${draft.theme === m ? "active" : ""}`}
                onClick={() => patch({ theme: m })}
              >
                {m === "light" ? (isPhone ? "正常" : "浅色") : m === "dark" ? "深色" : "跟随系统"}
              </button>
            ))}
          </div>
        </div>

        {!isPhone && (
          <>
        <div className="row">
          <div>
            <div className="row-title">桌面悬浮球</div>
            <div className="row-desc">常驻桌面；单击展开全部账号，双击打开主窗口</div>
          </div>
          <Switch
            checked={draft.ball.enabled}
            onChange={(v) => patch({ ball: { ...draft.ball, enabled: v } })}
            label="悬浮球"
          />
        </div>

        <div className="row">
          <div>
            <div className="row-title">球面显示账号</div>
            <div className="row-desc">默认全部合计；也可只盯某个号。悬浮球上也能改</div>
          </div>
          <select
            className="select"
            value={draft.ball.selected ?? ""}
            onChange={(e) => patch({ ball: { ...draft.ball, selected: e.target.value || null } })}
            disabled={view.accounts.length === 0}
          >
            <option value="">全部合计</option>
            {view.accounts.map((a) => (
              <option key={a.id} value={a.id}>
                {a.vendor_display} · {a.label}
              </option>
            ))}
          </select>
        </div>

        <div className="row">
          <div>
            <div className="row-title">球面显示内容</div>
            <div className="row-desc">选「待签数」时忽略上面的账号选择</div>
          </div>
          <div className="seg">
            <button
              className={`seg-item ${draft.ball.show === "total_credits" ? "active" : ""}`}
              onClick={() => patch({ ball: { ...draft.ball, show: "total_credits" } })}
            >
              积分
            </button>
            <button
              className={`seg-item ${draft.ball.show === "pending_count" ? "active" : ""}`}
              onClick={() => patch({ ball: { ...draft.ball, show: "pending_count" } })}
            >
              待签数
            </button>
          </div>
        </div>
          </>
        )}
      </section>

      <section className="panel">
        <div className="phead">
          <span className="pname">运行</span>
        </div>

        {!isPhone && (
          <>
        <div className="row">
          <div>
            <div className="row-title">开机自启动</div>
            <div className="row-desc">写入当前用户启动项，不需要管理员权限</div>
          </div>
          <Switch checked={draft.autostart} onChange={(v) => patch({ autostart: v })} label="开机自启" />
        </div>

        <div className="row">
          <div>
            <div className="row-title">关闭窗口时最小化到托盘</div>
            <div className="row-desc">关掉后点 × 会直接退出程序，自动签到也会停止</div>
          </div>
          <Switch
            checked={draft.minimize_to_tray}
            onChange={(v) => patch({ minimize_to_tray: v })}
            label="最小化到托盘"
          />
        </div>
          </>
        )}

        <div className="row">
          <div>
            <div className="row-title">定时自动签到</div>
            <div className="row-desc">按每个账号的平台刷新时间执行，每 45 秒核对一次</div>
          </div>
          <Switch
            checked={draft.scheduler_enabled}
            onChange={(v) => patch({ scheduler_enabled: v })}
            label="定时签到"
          />
        </div>

        <div className="row">
          <div>
            <div className="row-title">错过当天补签</div>
            <div className="row-desc">关闭后只认「到点后 30 分钟内」，过时不再补</div>
          </div>
          <Switch
            checked={draft.catch_up_missed}
            onChange={(v) => patch({ catch_up_missed: v })}
            label="补签"
          />
        </div>

        {!isPhone && (
          <div className="row">
            <div>
              <div className="row-title">积分自动刷新</div>
              <div className="row-desc">主窗口可见时按间隔重新采样；最小化后暂停</div>
            </div>
            <div className="seg">
              {[
                { v: 0, label: "关闭" },
                { v: 5, label: "5 分钟" },
                { v: 10, label: "10 分钟" },
                { v: 30, label: "30 分钟" },
              ].map((o) => (
                <button
                  key={o.v}
                  className={`seg-item ${draft.auto_refresh_minutes === o.v ? "active" : ""}`}
                  onClick={() => patch({ auto_refresh_minutes: o.v })}
                >
                  {o.label}
                </button>
              ))}
            </div>
          </div>
        )}

        <div className="row">
          <div>
            <div className="row-title">系统通知</div>
            <div className="row-desc">签到结束弹一条；失败也弹</div>
          </div>
          <div className="seg">
            <button
              className={`seg-item ${draft.notify_on_checkin ? "active" : ""}`}
              onClick={() => patch({ notify_on_checkin: !draft.notify_on_checkin })}
            >
              成功 {draft.notify_on_checkin ? "开" : "关"}
            </button>
            <button
              className={`seg-item ${draft.notify_on_failure ? "active" : ""}`}
              onClick={() => patch({ notify_on_failure: !draft.notify_on_failure })}
            >
              失败 {draft.notify_on_failure ? "开" : "关"}
            </button>
          </div>
        </div>
      </section>

      <section className="panel">
        <div className="phead">
          <span className="pname">更新</span>
          <span className="pnote">当前 v{view.version}</span>
        </div>

        <div className="row">
          <div>
            <div className="row-title">启动时检查更新</div>
            <div className="row-desc">发现新版本只弹通知，不会自动安装</div>
          </div>
          <Switch
            checked={draft.update.enabled}
            onChange={(v) => patch({ update: { ...draft.update, enabled: v } })}
            label="自动检查"
          />
        </div>

        <div className="row">
          <div>
            <div className="row-title">更新来源</div>
            <div className="row-desc">直连 GitHub 慢时可换镜像；改完先保存，再点检查</div>
          </div>
          <div className="seg">
            <button
              className={`seg-item ${draft.update.source === "github" ? "active" : ""}`}
              onClick={() => patch({ update: { ...draft.update, source: "github" } })}
            >
              GitHub
            </button>
            <button
              className={`seg-item ${draft.update.source === "mirror" ? "active" : ""}`}
              onClick={() => patch({ update: { ...draft.update, source: "mirror" } })}
            >
              镜像
            </button>
          </div>
        </div>

        {draft.update.source === "mirror" && (
          <div className="field">
            <label>镜像前缀</label>
            <input
              value={draft.update.mirror}
              onChange={(e) => patch({ update: { ...draft.update, mirror: e.target.value } })}
              placeholder="https://ghfast.top/"
              spellCheck={false}
            />
            <p className="hint">
              会拼在完整 GitHub 地址前面；查询版本和下载安装包走同一条规则
            </p>
          </div>
        )}

        <div className="row">
          <div>
            <div className="row-title">检查最新版本</div>
            <div className="row-desc">
              {checking
                ? "正在查询…"
                : update === null
                  ? "点右侧按钮查询发布记录"
                  : update.latest
                    ? `发现 v${update.latest.version}`
                    : "已是最新版本"}
            </div>
            {update?.latest && (
              <div className="row-desc">
                {update.latest.asset
                  ? `安装包：${update.latest.asset.name}`
                  : "这个版本没有本平台的安装包，请用「手动下载」"}
              </div>
            )}
            {updateMsg && <div className="row-desc">{updateMsg}</div>}
          </div>
          <div style={{ display: "flex", gap: 8, flexShrink: 0 }}>
            <button
              className="btn"
              onClick={() => api.updateOpenPage().catch((e) => notify(String(e), "warn"))}
            >
              手动下载
            </button>
            <button className="btn primary" onClick={checkUpdate} disabled={checking}>
              {checking ? "查询中…" : "检查"}
            </button>
          </div>
        </div>

        {update?.latest && (
          <div className="row">
            <div>
              <div className="row-title">下载并安装</div>
              <div className="row-desc">
                {isPhone
                  ? "跳到浏览器下载 APK，下载完点开按提示安装"
                  : "下载完成后自动打开安装程序"}
              </div>
            </div>
            <button
              className="btn primary"
              onClick={applyUpdate}
              disabled={applying || !update.latest.asset}
            >
              {applying ? "处理中…" : "下载"}
            </button>
          </div>
        )}
      </section>

      <section className="panel">
        <div className="phead">
          <span className="pname">数据</span>
        </div>

        <div className="row">
          <div>
            <div className="row-title">数据目录</div>
            <div className="row-desc mono">{view.data_dir}</div>
          </div>
          <button className="btn" onClick={() => api.openDataDir().catch((e) => notify(String(e), "warn"))}>
            打开
          </button>
        </div>

        <div className="row">
          <div>
            <div className="row-title">更换数据目录</div>
            <div className="row-desc">会复制现有数据到新位置，重启后生效</div>
          </div>
          <div className="inline-form">
            <input value={newDir} onChange={(e) => setNewDir(e.target.value)} placeholder="D:\PointHub" />
            <button
              className="btn"
              disabled={!newDir.trim()}
              onClick={async () => {
                try {
                  const msg = await api.relocateData(newDir.trim());
                  notify(msg);
                } catch (e) {
                  notify(String(e), "warn");
                }
              }}
            >
              搬迁
            </button>
          </div>
        </div>

        <div className="row">
          <div>
            <div className="row-title">凭据保护</div>
            <div className="row-desc">{view.vault_kind}，绑定当前 Windows 用户；换机后需重新导入</div>
          </div>
          <span className="tag tag-ok">已加密</span>
        </div>
      </section>

      <section className="panel">
        <div className="phead">
          <span className="pname">平台能力</span>
        </div>
        <table className="cap-table">
          <thead>
            <tr>
              <th>平台</th>
              <th>签到</th>
              <th>余额</th>
              <th>令牌续期</th>
              <th>派对活动</th>
              <th>验证状态</th>
            </tr>
          </thead>
          <tbody>
            {view.platforms.map((p) => (
              <tr key={p.vendor}>
                <td className="cap-vendor">
                  <VendorIcon vendor={p.vendor} size={15} />
                  {p.display}
                </td>
                <td>{yes(p.checkin)}</td>
                <td>{yes(p.balance)}</td>
                <td>{yes(p.token_refresh)}</td>
                <td>{yes(p.travel)}</td>
                <td>
                  {p.verified ? <span className="tag tag-ok">已实机验证</span> : <span className="tag tag-muted">待实测</span>}
                </td>
              </tr>
            ))}
          </tbody>
        </table>
      </section>

      <section className="panel">
        <div className="phead">
          <span className="pname">关于</span>
        </div>
        <div className="row">
          <div>
            <div className="row-title">积分枢纽 PointHub v{view.version}</div>
            <div className="row-desc">使用非公开接口，可能因平台改版失效；请仅管理本人持有的账号。</div>
          </div>
          <button className="btn" onClick={() => api.quit().catch(() => undefined)}>
            退出程序
          </button>
        </div>
      </section>
    </>
  );
}

function yes(b: boolean) {
  return b ? "支持" : "—";
}

function Switch({ checked, onChange, label }: { checked: boolean; onChange: (v: boolean) => void; label: string }) {
  return (
    <button
      className={`switch ${checked ? "on" : ""}`}
      role="switch"
      aria-checked={checked}
      aria-label={label}
      onClick={() => onChange(!checked)}
    >
      <span />
    </button>
  );
}

function Modal({ title, children, onClose }: { title: string; children: React.ReactNode; onClose: () => void }) {
  return (
    <div className="modal-mask" onClick={onClose}>
      <div className="modal" onClick={(e) => e.stopPropagation()}>
        <div className="modal-head">
          <h3>{title}</h3>
          <button className="icon-btn" onClick={onClose} aria-label="关闭">
            <IconClose />
          </button>
        </div>
        {children}
      </div>
    </div>
  );
}

function NavItem({
  active,
  onClick,
  label,
  badge,
}: {
  active: boolean;
  onClick: () => void;
  label: string;
  badge?: number;
}) {
  return (
    <button className={`nav ${active ? "on" : ""}`} onClick={onClick}>
      <span>{label}</span>
      {badge !== undefined && badge > 0 && <span className="cnt">{badge}</span>}
    </button>
  );
}

function IconTrash() {
  return (
    <svg viewBox="0 0 20 20" width="15" height="15" fill="none" stroke="currentColor" strokeWidth="1.5">
      <path d="M4 6h12M8 6V4.5h4V6M6 6l.7 9.2c.05.7.6 1.3 1.3 1.3h4c.7 0 1.25-.6 1.3-1.3L14 6" />
    </svg>
  );
}

function IconClose() {
  return (
    <svg viewBox="0 0 20 20" width="15" height="15" fill="none" stroke="currentColor" strokeWidth="1.6">
      <path d="M5 5l10 10M15 5L5 15" />
    </svg>
  );
}

/// 积分展示：最多两位小数，整数不带小数点（`204.62` / `3,600`）。
/// 上游会给出 `204.62000029` 这种字符串小数，直接显示会带一串噪声。
function credits(n: number): string {
  return n.toLocaleString("zh-CN", { maximumFractionDigits: 2 });
}

function clock(ts: number): string {
  const d = new Date(ts * 1000);
  const hhmm = d.toLocaleTimeString("zh-CN", { hour: "2-digit", minute: "2-digit" });
  const today = new Date();
  if (d.toDateString() === today.toDateString()) return hhmm;
  const yesterday = new Date(today.getTime());
  yesterday.setDate(today.getDate() - 1);
  // 只写时分会被误读成今天，跨天的记录必须带上「昨天」或日期。
  if (d.toDateString() === yesterday.toDateString()) return `昨天 ${hhmm}`;
  return `${d.getMonth() + 1}/${d.getDate()} ${hhmm}`;
}
