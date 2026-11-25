import React, { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { api } from "./api";
import { useTheme } from "./theme";
import type { AccountView, DayPoint, Settings, StateView, ThemeMode } from "./types";

type Tab = "overview" | "accounts" | "settings";

/** 平台识别色，只用于这一点小圆点，不铺大面积。 */
const VENDOR_TONE: Record<string, string> = {
  workbuddy: "wb",
  "trae-cn": "trae",
  "qoder-cn": "qoder",
};

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
        <div className="boot-mark" />
        <div>正在读取账号数据…</div>
      </div>
    );
  }

  return (
    <div className="app">
      <aside className="rail">
        <div className="brand">
          <div className="mark" aria-hidden>
            <svg viewBox="0 0 24 24" width="17" height="17" fill="none" stroke="currentColor" strokeWidth="1.7">
              <circle cx="12" cy="12" r="8.2" />
              <circle cx="12" cy="12" r="2.1" fill="currentColor" />
              <circle cx="12" cy="3.8" r="1.5" fill="currentColor" />
              <circle cx="19.1" cy="16.1" r="1.5" fill="currentColor" />
              <circle cx="4.9" cy="16.1" r="1.5" fill="currentColor" />
            </svg>
          </div>
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
            {enabled.length === 0
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
          <div className="kval">{view.aggregate.total_balance.toLocaleString("zh-CN")}</div>
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
            {todayGain === null ? "—" : `${todayGain >= 0 ? "+" : ""}${todayGain.toLocaleString("zh-CN")}`}
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
            导出一个长效令牌就能开始。令牌会以当前 Windows 用户身份加密保存，不会离开这台电脑。
          </p>
          <button className="btn primary" onClick={onAdd}>
            去添加账号
          </button>
        </div>
      ) : (
        <div className="panels">
          {groups.map((g) => (
            <section className="panel" key={g.vendor}>
              <div className="phead">
                <span className={`dot tone-${VENDOR_TONE[g.vendor] ?? "wb"}`} />
                <span className="pname">{g.display}</span>
                {!g.verified && <span className="tag tag-muted">端点待实测</span>}
                <span className="psub">
                  {g.accounts.length} 账号 · 小计 {g.subtotal.toLocaleString("zh-CN")}
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
                          {a.balance.toLocaleString("zh-CN")}
                          <small>分</small>
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

  return (
    <div className="bcard">
      <div className="btitle">近 7 日每日新增</div>
      {!hasAny ? (
        <div className="ev-empty">还不到两天数据，攒够两天才有对比</div>
      ) : (
        <div className="bars">
          {trend.map((t, i) => {
            const g = t.gain;
            const h = g === null || g === 0 ? 3 : Math.max(6, Math.round((Math.abs(g) / max) * 56));
            return (
              <div key={t.date} title={`${t.date} · ${g === null ? "无对比" : `${g >= 0 ? "+" : ""}${g}`} · 合计 ${t.total}`}>
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
            {e.delta !== null && e.delta > 0 && <span className="ev-delta">+{e.delta}</span>}
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
  const [mode, setMode] = useState<"browser" | "token">("browser");
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
      await api.addAccount({
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
      notify("账号已添加");
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
                  <div className={`dot tone-${VENDOR_TONE[a.vendor] ?? "wb"}`} />
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
                    {a.balance === null ? "—" : a.balance.toLocaleString("zh-CN")}
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
                <button className={`seg-item ${mode === "token" ? "active" : ""}`} onClick={() => setMode("token")}>
                  手动令牌
                </button>
              </div>
            </div>

            {mode === "browser" ? (
              <>
                {vendor !== "workbuddy" && (
                  <p className="hint warn">
                    {cap?.display ?? vendor} 还没接入官方授权流程，请切到「手动令牌」导入。
                  </p>
                )}
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
                  走的是官方插件授权：软件只拿到一个由你本人确认过的长效令牌，加密存在本机；
                  不读取你浏览器的任何数据，也不用另开一个专用浏览器。
                </p>
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

      <section className="panel">
        <div className="phead">
          <span className="pname">外观</span>
        </div>

        <div className="row">
          <div>
            <div className="row-title">主题</div>
            <div className="row-desc">「跟随系统」会随 Windows 的浅色/深色切换</div>
          </div>
          <div className="seg">
            {(["light", "dark", "system"] as ThemeMode[]).map((m) => (
              <button
                key={m}
                className={`seg-item ${draft.theme === m ? "active" : ""}`}
                onClick={() => patch({ theme: m })}
              >
                {m === "light" ? "浅色" : m === "dark" ? "深色" : "跟随系统"}
              </button>
            ))}
          </div>
        </div>

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
      </section>

      <section className="panel">
        <div className="phead">
          <span className="pname">运行</span>
        </div>

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
                <td>{p.display}</td>
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
