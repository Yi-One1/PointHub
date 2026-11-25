import React, { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { api } from "./api";
import type { AccountView, Settings, StateView, ThemeMode } from "./types";

type Tab = "overview" | "accounts" | "settings";

const VENDOR_TONE: Record<string, string> = {
  workbuddy: "sky",
  "trae-cn": "amber",
  "qoder-cn": "indigo",
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

  // 主题：settings.theme 决定；system 时跟随系统并监听变化
  useEffect(() => {
    const mode: ThemeMode = view?.settings.theme ?? "system";
    const mq = window.matchMedia("(prefers-color-scheme: dark)");
    const apply = () => {
      const dark = mode === "dark" || (mode === "system" && mq.matches);
      document.documentElement.dataset.theme = dark ? "dark" : "light";
    };
    apply();
    if (mode === "system") {
      mq.addEventListener("change", apply);
      return () => mq.removeEventListener("change", apply);
    }
  }, [view?.settings.theme]);

  const doneCount = useMemo(() => {
    if (!view) return 0;
    return view.accounts.filter((a) => a.enabled && a.checked_in_today === true).length;
  }, [view]);

  const readyCount = useMemo(
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
      const v = await api.bootstrap();
      setView(v);
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
      const v = await api.bootstrap();
      setView(v);
      if (o) notify(`${a.label}：${o.status_text} · ${o.message}`, o.status === "failed" ? "warn" : "ok");
    });

  const travel = (a: AccountView) =>
    run(`travel-${a.id}`, async () => {
      const msg = await api.travel(a.id, "auto");
      notify(`${a.label}：${msg}`);
      const v = await api.refreshAll();
      setView(v);
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

  const total = view.aggregate.total_balance;
  const ready = view.accounts.filter((a) => a.enabled).length;
  const pct = ready === 0 ? 0 : Math.round((doneCount / ready) * 100);

  return (
    <div className="app">
      <aside className="rail">
        <div className="brand">
          <div className="brand-mark" aria-hidden>
            <svg viewBox="0 0 24 24" width="17" height="17">
              <circle cx="12" cy="12" r="8.2" fill="none" stroke="currentColor" strokeWidth="1.7" />
              <circle cx="12" cy="12" r="2.1" fill="currentColor" />
              <circle cx="12" cy="3.8" r="1.5" fill="currentColor" />
              <circle cx="19.1" cy="16.1" r="1.5" fill="currentColor" />
              <circle cx="4.9" cy="16.1" r="1.5" fill="currentColor" />
            </svg>
          </div>
          <div>
            <div className="brand-name">积分枢纽</div>
            <div className="brand-sub">PointHub</div>
          </div>
        </div>

        <nav className="nav">
          <NavItem active={tab === "overview"} onClick={() => setTab("overview")} label="总览" icon={<IconGrid />} />
          <NavItem active={tab === "accounts"} onClick={() => setTab("accounts")} label="账号" icon={<IconUsers />} badge={view.accounts.length} />
          <NavItem active={tab === "settings"} onClick={() => setTab("settings")} label="设置" icon={<IconGear />} />
        </nav>

        <div className="rail-foot">
          <div className="rail-stat">
            <span className="rail-stat-label">平台</span>
            <span className="rail-stat-value">{readyCount}</span>
          </div>
          <div className="rail-stat">
            <span className="rail-stat-label">版本</span>
            <span className="rail-stat-value">v{view.version}</span>
          </div>
          <div className="rail-note">数据只在本机 · {view.vault_kind}</div>
        </div>
      </aside>

      <main className="main">
        {tab === "overview" && (
          <>
            <header className="page-head">
              <div>
                <h1>总览</h1>
                <p className="subtle">
                  {ready === 0
                    ? "还没有账号，去「账号」页导入一个"
                    : `今日已签 ${doneCount} / ${ready}${view.aggregate.last_sampled_at ? ` · 最近采样 ${clock(view.aggregate.last_sampled_at)}` : ""}`}
                </p>
              </div>
              <div className="head-actions">
                <button className="btn" onClick={refresh} disabled={busy !== null}>
                  {busy === "refresh" ? "刷新中…" : "刷新积分"}
                </button>
                <button
                  className="btn btn-primary"
                  onClick={checkinAll}
                  disabled={busy !== null || ready === 0}
                >
                  {busy === "checkin-all" ? "签到中…" : "一键全部签到"}
                </button>
              </div>
            </header>

            <section className="hero">
              <div className="hero-main">
                <div className="metric-label">可用积分合计</div>
                <div className="metric-value">{total.toLocaleString("zh-CN")}</div>
                <div className="progress" role="progressbar" aria-valuenow={pct} aria-valuemin={0} aria-valuemax={100}>
                  <div className="progress-fill" style={{ width: `${pct}%` }} />
                </div>
                <div className="metric-foot">
                  {doneCount} / {ready} 个账号今日已签
                  {view.aggregate.pending_count > 0 && (
                    <span className="metric-flag">{view.aggregate.pending_count} 个待签</span>
                  )}
                </div>
              </div>
              <div className="hero-side">
                <Stat label="纳管账号" value={String(view.accounts.length)} />
                <Stat label="有余额数据" value={`${view.aggregate.balance_known}`} />
                <Stat label="历史记录" value={`${view.history_count}`} />
              </div>
            </section>

            {view.accounts.length === 0 ? (
              <EmptyState onAdd={() => setTab("accounts")} />
            ) : (
              <div className="grid">
                {view.accounts.map((a) => (
                  <AccountCard
                    key={a.id}
                    account={a}
                    busy={busy === `checkin-${a.id}` || busy === `travel-${a.id}`}
                    onCheckin={() => checkinOne(a)}
                    onTravel={() => travel(a)}
                    onToggle={(enabled) =>
                      run(`toggle-${a.id}`, async () => {
                        await api.setAccountEnabled(a.id, enabled);
                        setView(await api.bootstrap());
                      })
                    }
                  />
                ))}
              </div>
            )}
          </>
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

// ------------------------------------------------------------------ 总览卡片

function AccountCard({
  account,
  busy,
  onCheckin,
  onTravel,
  onToggle,
}: {
  account: AccountView;
  busy: boolean;
  onCheckin: () => void;
  onTravel: () => void;
  onToggle: (enabled: boolean) => void;
}) {
  const tone = VENDOR_TONE[account.vendor] ?? "sky";
  const status = statusOf(account);

  return (
    <article className={`card tone-${tone} ${account.enabled ? "" : "card-off"}`}>
      <div className="card-top">
        <div>
          <div className="vendor-line">
            <span className="vendor-name">{account.vendor_display}</span>
            {!account.verified && <span className="tag tag-muted">待实测</span>}
            {account.travel_enabled && account.can_travel && <span className="tag">派猫猫</span>}
          </div>
          <div className="card-title">{account.label}</div>
        </div>
        <button
          className={`switch ${account.enabled ? "on" : ""}`}
          role="switch"
          aria-checked={account.enabled}
          aria-label={`启用 ${account.label}`}
          onClick={() => onToggle(!account.enabled)}
        >
          <span />
        </button>
      </div>

      <div className="card-mid">
        <div className="balance">
          {account.balance === null ? <span className="balance-none">积分未知</span> : account.balance.toLocaleString("zh-CN")}
        </div>
        <span className={`pill pill-${status.kind}`}>
          <i />
          {status.text}
        </span>
      </div>

      <div className="card-meta">
        <span>{account.hint}</span>
        {account.streak_days !== null && <span>连签 {account.streak_days} 天</span>}
        {account.today_gain !== null && account.today_gain > 0 && <span className="gain">今日 +{account.today_gain}</span>}
        {account.travel_location && <span>{account.travel_location}</span>}
      </div>

      {account.last_error && <div className="card-error">{account.last_error}</div>}

      <div className="card-actions">
        <button className="btn btn-sm btn-primary" onClick={onCheckin} disabled={busy || !account.has_credential || !account.can_checkin}>
          {busy ? "处理中…" : "签到"}
        </button>
        {account.can_travel && (
          <button className="btn btn-sm" onClick={onTravel} disabled={busy || !account.has_credential}>
            派猫猫
          </button>
        )}
        <span className="card-schedule">{account.schedule_text}</span>
      </div>
    </article>
  );
}

function statusOf(a: AccountView): { kind: string; text: string } {
  if (!a.has_credential) return { kind: "idle", text: "缺凭据" };
  if (a.checked_in_today === true) return { kind: "ok", text: "今日已签" };
  if (a.checked_in_today === false) return { kind: "todo", text: "待签到" };
  return { kind: "idle", text: "未采样" };
}

function Stat({ label, value }: { label: string; value: string }) {
  return (
    <div className="stat">
      <div className="stat-label">{label}</div>
      <div className="stat-value">{value}</div>
    </div>
  );
}

function EmptyState({ onAdd }: { onAdd: () => void }) {
  return (
    <div className="empty">
      <div className="empty-title">还没有纳管任何账号</div>
      <p className="empty-text">
        导出一个长效令牌就能开始。令牌会以当前 Windows 用户身份加密保存，不会离开这台电脑。
      </p>
      <button className="btn btn-primary" onClick={onAdd}>
        去添加账号
      </button>
    </div>
  );
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
  const [label, setLabel] = useState("");
  const [secret, setSecret] = useState("");
  const [kind, setKind] = useState<"refresh" | "access">("refresh");
  const [deviceId, setDeviceId] = useState("");
  const [domain, setDomain] = useState("");
  const [target, setTarget] = useState<AccountView | null>(null);

  const cap = view.platforms.find((p) => p.vendor === vendor);

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
      <header className="page-head">
        <div>
          <h1>账号</h1>
          <p className="subtle">同一平台下可以有多个账号，各自独立定时、独立凭据</p>
        </div>
      </header>

      <div className="split">
        <section className="panel">
          <div className="panel-head">
            <h2>已纳管</h2>
            <span className="subtle">{view.accounts.length} 个</span>
          </div>

          {view.accounts.length === 0 ? (
            <p className="panel-empty">还没有账号。右侧导入第一个。</p>
          ) : (
            <ul className="acct-list">
              {view.accounts.map((a) => (
                <li key={a.id} className="acct-row">
                  <div className={`acct-dot tone-${VENDOR_TONE[a.vendor] ?? "sky"}`} />
                  <div className="acct-main">
                    <div className="acct-title">
                      {a.label}
                      <span className="acct-vendor">{a.vendor_display}</span>
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
          <div className="panel-head">
            <h2>导入账号</h2>
          </div>

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
            <label>别名</label>
            <input value={label} onChange={(e) => setLabel(e.target.value)} placeholder={`例如：主号`} />
          </div>

          <div className="field">
            <label>凭据类型</label>
            <div className="seg">
              <button className={`seg-item ${kind === "refresh" ? "active" : ""}`} onClick={() => setKind("refresh")}>
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
              <input value={deviceId} onChange={(e) => setDeviceId(e.target.value)} placeholder="x-device-id 原值" spellCheck={false} />
            </div>
          )}

          <div className="field">
            <label>鉴权域名（可选）</label>
            <input value={domain} onChange={(e) => setDomain(e.target.value)} placeholder="默认 www.codebuddy.cn" spellCheck={false} />
            <p className="hint">WorkBuddy 以登录态里的域名为准；留空用默认值。</p>
          </div>

          <button className="btn btn-primary btn-block" onClick={submit} disabled={!secret.trim() || busy !== null}>
            添加
          </button>
          <button className="link-btn" onClick={onOpenSettings}>
            凭据存在哪？看设置页的说明
          </button>
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
            <button className="btn btn-danger" onClick={() => remove(target)}>
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
      <header className="page-head">
        <div>
          <h1>设置</h1>
          <p className="subtle">改动立即生效；带「保存」按钮的项需要确认</p>
        </div>
        <div className="head-actions">
          <button className="btn" onClick={() => setDraft(view.settings)} disabled={!dirty}>
            还原
          </button>
          <button className="btn btn-primary" onClick={() => onSave(draft)} disabled={!dirty || busy !== null}>
            {busy === "settings" ? "保存中…" : "保存"}
          </button>
        </div>
      </header>

      <section className="panel">
        <div className="panel-head">
          <h2>外观</h2>
        </div>
        <div className="row">
          <div>
            <div className="row-title">主题</div>
            <div className="row-desc">「跟随系统」会随 Windows 的浅色/深色切换</div>
          </div>
          <div className="seg">
            {(["light", "dark", "system"] as ThemeMode[]).map((m) => (
              <button key={m} className={`seg-item ${draft.theme === m ? "active" : ""}`} onClick={() => patch({ theme: m })}>
                {m === "light" ? "浅色" : m === "dark" ? "深色" : "跟随系统"}
              </button>
            ))}
          </div>
        </div>
        <div className="row">
          <div>
            <div className="row-title">桌面悬浮球</div>
            <div className="row-desc">常驻桌面显示总积分，双击打开主窗口</div>
          </div>
          <Switch checked={draft.ball.enabled} onChange={(v) => patch({ ball: { ...draft.ball, enabled: v } })} label="悬浮球" />
        </div>
        <div className="row">
          <div>
            <div className="row-title">悬浮球显示</div>
            <div className="row-desc">球面上显示哪个数字</div>
          </div>
          <div className="seg">
            <button
              className={`seg-item ${draft.ball.show === "total_credits" ? "active" : ""}`}
              onClick={() => patch({ ball: { ...draft.ball, show: "total_credits" } })}
            >
              总积分
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
        <div className="panel-head">
          <h2>运行</h2>
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
          <Switch checked={draft.minimize_to_tray} onChange={(v) => patch({ minimize_to_tray: v })} label="最小化到托盘" />
        </div>
        <div className="row">
          <div>
            <div className="row-title">定时自动签到</div>
            <div className="row-desc">按每个账号的平台刷新时间执行，错过当天会在启动时补签</div>
          </div>
          <Switch checked={draft.scheduler_enabled} onChange={(v) => patch({ scheduler_enabled: v })} label="定时签到" />
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
        <div className="panel-head">
          <h2>数据</h2>
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
            <input value={newDir} onChange={(e) => setNewDir(e.target.value)} placeholder="D:\\PointHub" />
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
        <div className="panel-head">
          <h2>平台能力</h2>
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
                <td>{p.verified ? <span className="tag tag-ok">已实机验证</span> : <span className="tag tag-muted">待实测</span>}</td>
              </tr>
            ))}
          </tbody>
        </table>
      </section>

      <section className="panel">
        <div className="panel-head">
          <h2>关于</h2>
        </div>
        <div className="row">
          <div>
            <div className="row-title">积分枢纽 PointHub v{view.version}</div>
            <div className="row-desc">
              使用非公开接口，可能因平台改版失效；请仅管理本人持有的账号。
            </div>
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
    <button className={`switch ${checked ? "on" : ""}`} role="switch" aria-checked={checked} aria-label={label} onClick={() => onChange(!checked)}>
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

function NavItem({ active, onClick, label, icon, badge }: { active: boolean; onClick: () => void; label: string; icon: React.ReactNode; badge?: number }) {
  return (
    <button className={`nav-item ${active ? "active" : ""}`} onClick={onClick}>
      <span className="nav-icon">{icon}</span>
      <span>{label}</span>
      {badge !== undefined && badge > 0 && <span className="nav-badge">{badge}</span>}
    </button>
  );
}

// ------------------------------------------------------------------ 图标（内联 SVG，不引外部资源）

function IconGrid() {
  return (
    <svg viewBox="0 0 20 20" width="16" height="16" fill="none" stroke="currentColor" strokeWidth="1.5">
      <rect x="3" y="3" width="6" height="6" rx="1.6" />
      <rect x="11" y="3" width="6" height="6" rx="1.6" />
      <rect x="3" y="11" width="6" height="6" rx="1.6" />
      <rect x="11" y="11" width="6" height="6" rx="1.6" />
    </svg>
  );
}

function IconUsers() {
  return (
    <svg viewBox="0 0 20 20" width="16" height="16" fill="none" stroke="currentColor" strokeWidth="1.5">
      <circle cx="7.5" cy="7" r="2.6" />
      <path d="M3 16c0-2.5 2-4.2 4.5-4.2S12 13.5 12 16" />
      <path d="M13.4 4.8a2.4 2.4 0 0 1 0 4.5M15 15.6c0-1.9-.7-3.2-1.9-3.9" />
    </svg>
  );
}

function IconGear() {
  return (
    <svg viewBox="0 0 20 20" width="16" height="16" fill="none" stroke="currentColor" strokeWidth="1.5">
      <circle cx="10" cy="10" r="2.6" />
      <path d="M10 3.2v1.6M10 15.2v1.6M16.8 10h-1.6M4.8 10H3.2M14.8 5.2l-1.1 1.1M6.3 13.7l-1.1 1.1M14.8 14.8l-1.1-1.1M6.3 6.3 5.2 5.2" />
    </svg>
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
  return d.toLocaleTimeString("zh-CN", { hour: "2-digit", minute: "2-digit" });
}
