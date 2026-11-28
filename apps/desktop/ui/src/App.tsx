import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import type { ReactNode } from "react";
import { api } from "./api";
import { BRAND_MARK, BRAND_MARK_DARK, VENDOR_NAME } from "./assets";
import { useTheme } from "./theme";
import { PointsIcon } from "./PointsIcon";
import { VendorIcon } from "./VendorIcon";
import { IconAccounts, IconGear, IconOverview, IconTrend } from "./icons";
import SettingsScreen from "./SettingsScreen";
import { TrendModal } from "./TrendModal";
import { TrendsScreen } from "./TrendsScreen";
import { TrendChart, TrendLegend, dayTrendPoints, useTrendVisible, type TrendPoint } from "./TrendChart";
import { credits } from "./format";
import type { SettingsSectionId } from "./settings";
import { Modal } from "./ui";
import type {
  AccountView,
  DayPoint,
  LocalImportStatus,
  Settings,
  StateView,
  UpdateCheck,
} from "./types";

type Tab = "overview" | "accounts" | "trends";

export default function App() {
  const [view, setView] = useState<StateView | null>(null);
  const [tab, setTab] = useState<Tab>("overview");
  // 统计页看哪个平台；null = 全部平台。受控在这一层，总览与弹窗都能把用户送过去。
  const [trendScope, setTrendScope] = useState<string | null>(null);
  const [settingsOpen, setSettingsOpen] = useState(false);
  const [settingsSection, setSettingsSection] = useState<SettingsSectionId | null>(null);
  const [settingsAnchor, setSettingsAnchor] = useState<"data-location" | null>(null);
  const [busy, setBusy] = useState<string | null>(null);
  // 更新检查结果放在 App 层：总览右侧的「有新版本」提示和设置页共用同一份。
  const [update, setUpdate] = useState<UpdateCheck | null>(null);
  const [updateChecking, setUpdateChecking] = useState(false);
  const updateChecked = useRef(false);
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

  /** 检查更新；silent=true 时失败不打扰（启动自动检查用）。 */
  const checkUpdate = useCallback(
    async (silent = false) => {
      setUpdateChecking(true);
      try {
        setUpdate(await api.updateCheck());
      } catch (e) {
        if (!silent) notify(`检查更新失败：${e}`, "warn");
      } finally {
        setUpdateChecking(false);
      }
    },
    [notify],
  );

  const updateEnabled = view?.settings.update.enabled ?? false;

  // 启动时按设置自动检查一次：只提示，不自动安装；保存其他设置不会重复请求。
  useEffect(() => {
    if (!view) return;
    if (!updateEnabled) {
      updateChecked.current = false;
      setUpdate(null);
      return;
    }
    if (updateChecked.current) return;
    updateChecked.current = true;
    void checkUpdate(true);
  }, [view?.version, updateEnabled, checkUpdate]);

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

  /// 跳到统计页并指定平台（null = 全部平台）。总览卡片与平台弹窗共用这一条路径。
  const openTrends = useCallback((vendor: string | null) => {
    setTrendScope(vendor);
    setTab("trends");
  }, []);

  // 统计页在远程模式下没有入口也取不到数：配对成功时把还停在该页的用户送回
  // 总览，否则主区会只剩一片空白。
  useEffect(() => {
    if (view?.mode === "remote" && tab === "trends") setTab("overview");
  }, [view?.mode, tab]);

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

  const settingsSeq = useRef(0);

  const saveSettings = async (next: Settings, silent = false): Promise<boolean> => {
    const seq = ++settingsSeq.current;
    setBusy("settings");
    try {
      const v = await api.saveSettings(next);
      if (seq === settingsSeq.current) setView(v);
      if (!silent) notify("设置已保存");
      return true;
    } catch (e) {
      notify(String(e), "warn");
      return false;
    } finally {
      if (seq === settingsSeq.current) setBusy(null);
    }
  };

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
            <div className="bname-row">
              <div className="bname">积分枢纽</div>
              <span className="bver">v{view.version}</span>
            </div>
            <div className="bsub">POINTHUB</div>
          </div>
        </div>

        <nav>
          <NavItem
            active={tab === "overview"}
            onClick={() => setTab("overview")}
            label="总览"
            icon={<IconOverview />}
          />
          <NavItem
            active={tab === "accounts"}
            onClick={() => setTab("accounts")}
            label="账号"
            badge={view.accounts.length}
            icon={<IconAccounts />}
          />
          {/* 远程模式下统计命令在电脑端，本机会直接拒绝，入口干脆不出现 */}
          {view.mode !== "remote" && (
            <NavItem
              active={tab === "trends"}
              onClick={() => setTab("trends")}
              label="统计"
              icon={<IconTrend />}
            />
          )}
          <button
            className="nav rail-settings"
            onClick={() => {
              setSettingsSection(null);
              setSettingsAnchor(null);
              setSettingsOpen(true);
            }}
            aria-label="设置"
            title="设置"
          >
            <span className="nav-ico">
              <IconGear />
            </span>
            <span>设置</span>
          </button>
        </nav>
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
            onOpenTrends={openTrends}
            updateVersion={update?.latest?.version ?? null}
            onOpenUpdate={() => {
              setSettingsSection("update");
              setSettingsAnchor(null);
              setSettingsOpen(true);
            }}
            onOpenCheckinSettings={() => {
              setSettingsSection("checkin");
              setSettingsAnchor(null);
              setSettingsOpen(true);
            }}
          />
        )}

        {tab === "accounts" && (
          <AccountsPage
            view={view}
            busy={busy}
            onChanged={setView}
            notify={notify}
            onOpenSettings={(section) => {
              setSettingsSection(section ?? null);
              setSettingsAnchor(section === "data" ? "data-location" : null);
              setSettingsOpen(true);
            }}
          />
        )}

        {tab === "trends" && view.mode !== "remote" && (
          <TrendsScreen view={view} scope={trendScope} onScope={setTrendScope} />
        )}
      </main>

      {settingsOpen && (
        <SettingsScreen
          view={view}
          onSave={saveSettings}
          notify={notify}
          onClose={() => {
            setSettingsOpen(false);
            setSettingsAnchor(null);
          }}
          initialSection={settingsSection ?? undefined}
          initialAnchor={settingsAnchor ?? undefined}
          update={update}
          checkingUpdate={updateChecking}
          onCheckUpdate={() => void checkUpdate()}
          onUpdateApplied={() => setUpdate(null)}
        />
      )}

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
  onOpenTrends,
  updateVersion,
  onOpenUpdate,
  onOpenCheckinSettings,
}: {
  view: StateView;
  busy: string | null;
  onRefresh: () => void;
  onCheckinAll: () => void;
  onCheckinOne: (a: AccountView) => void;
  onTravel: (a: AccountView) => void;
  onToggle: (a: AccountView, enabled: boolean) => void;
  onAdd: () => void;
  /** 跳到统计页；参数是该图要看的平台（null = 全部平台） */
  onOpenTrends: (vendor: string | null) => void;
  /** 发现的新版本号；null 表示没有可更新的版本 */
  updateVersion: string | null;
  onOpenUpdate: () => void;
  /** 打开设置并落在「签到与通知」分区（各平台签到时间在同一处） */
  onOpenCheckinSettings: () => void;
}) {
  const enabled = view.accounts.filter((a) => a.enabled);
  const done = enabled.filter((a) => a.checked_in_today === true).length;
  const pending = Math.max(enabled.length - done, 0);
  // 「今日新增 / 今日消耗」直接取走势的最后一天，两处口径天然一致。
  const todayPoint = view.trend.length > 0 ? view.trend[view.trend.length - 1] : null;
  const todayUp = todayPoint?.up ?? null;
  const todayDown = todayPoint?.down ?? null;
  // 小于半分的变化当成 0：浮点残差不该让卡片显示「-0」。
  const upPositive = todayUp !== null && todayUp >= 0.005;
  const downPositive = todayDown !== null && todayDown >= 0.005;

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

  const lastSample = view.aggregate.last_sampled_at ? clock(view.aggregate.last_sampled_at) : null;
  // null 表示没开弹窗；vendor 为 null 时看全部平台。弹窗只放近 7 日，
  // 本月/本年度在左侧「统计」页（`onOpenTrends`）。
  const [modalScope, setModalScope] = useState<{ vendor: string | null; display: string } | null>(null);

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
          {updateVersion && (
            <button
              className="btn update-hint"
              onClick={onOpenUpdate}
              title="前往设置 → 更新"
            >
              有新版本 v{updateVersion} 可更新
            </button>
          )}
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
          <div className="klabel">积分总额</div>
          <div className="kval">
            {credits(view.aggregate.total_balance)}
            <PointsIcon tone="bronze" size={18} />
          </div>
          <div className="kfoot">
            {enabled.length === 0
              ? "还没有账号"
              : `已签 ${done}/${enabled.length}${lastSample ? ` · 最近采样 ${lastSample}` : ""}`}
          </div>
        </div>
        <div className="kpi">
          <div className="klabel">今日新增</div>
          <div className={`kval ${upPositive ? "up" : ""}`}>
            {todayUp === null ? (
              "—"
            ) : (
              <>
                <span className="ksign">{upPositive ? "+" : ""}</span>
                {credits(upPositive ? (todayUp ?? 0) : 0)}
                <PointsIcon tone={upPositive ? "jade" : "ink"} size={18} />
              </>
            )}
          </div>
          <div className="kfoot">签到实际到账</div>
        </div>
        <div className="kpi">
          <div className="klabel">今日消耗</div>
          <div className={`kval ${downPositive ? "down" : ""}`}>
            {todayDown === null ? (
              "—"
            ) : (
              <>
                <span className="ksign">{downPositive ? "-" : ""}</span>
                {credits(downPositive ? (todayDown ?? 0) : 0)}
                <PointsIcon tone={downPositive ? "ruby" : "ink"} size={18} />
              </>
            )}
          </div>
          <div className="kfoot">含当天已用掉的</div>
        </div>
        <div className="kpi">
          <div className="klabel">本月领取总积分</div>
          <div className="kval">
            {view.aggregate.claimed_this_month === null ? (
              "—"
            ) : (
              <>
                {credits(view.aggregate.claimed_this_month)}
                <PointsIcon tone="jade" size={18} />
              </>
            )}
          </div>
          <div className="kfoot">签到实际到账</div>
        </div>
        <div className="kpi">
          <div className="klabel">今日已签</div>
          <div className="kval">
            {done}
            <small>/ {enabled.length}</small>
          </div>
          <div className="kfoot">{pending > 0 ? `${pending} 个待签` : "全部完成"}</div>
        </div>
      </div>

      {(view.checkin_alerts?.length ?? 0) > 0 && (
        <div className="alert-bar" role="alert">
          <div className="alert-main">
            <b>今日签到异常（{view.checkin_alerts.length}）</b>
            <span className="alert-text">
              {view.checkin_alerts
                .map((a) => `${a.label}：${a.message}`)
                .join("；")}
            </span>
          </div>
          <button className="btn" onClick={onCheckinAll} disabled={busy !== null}>
            立即重试
          </button>
        </div>
      )}

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
                {view.mode !== "remote" && (
                  <button
                    className="phead-btn"
                    onClick={() => setModalScope({ vendor: g.vendor, display: g.display })}
                    title={`查看 ${g.display} 的近 7 日积分变化`}
                  >
                    近 7 日
                  </button>
                )}
                {view.mode !== "remote" && (
                  <button
                    className="phead-btn"
                    onClick={onOpenCheckinSettings}
                    title={`设置 ${g.display} 的自动签到时间`}
                  >
                    签到时间
                  </button>
                )}
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
                      <div className="rhint" title={accountMetaTitle(a)}>
                        {[importSourceText(a.source), a.schedule_text]
                          .filter(Boolean)
                          .join(" · ")}
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
        <TrendCard
          trend={view.trend}
          onOpenMonths={view.mode === "remote" ? undefined : () => onOpenTrends(null)}
        />
        <ActivityCard events={view.events} />
      </div>

      {modalScope && (
        <TrendModal
          vendor={modalScope.vendor}
          display={modalScope.display}
          onClose={() => setModalScope(null)}
          onOpenTrends={() => {
            const vendor = modalScope.vendor;
            setModalScope(null);
            onOpenTrends(vendor);
          }}
        />
      )}
    </>
  );
}

function TrendCard({ trend, onOpenMonths }: { trend: DayPoint[]; onOpenMonths?: () => void }) {
  // 这一张图自己一份图例 state，与统计页的两张图互不影响。
  const [visible, toggle] = useTrendVisible();
  // 总额、新增、消耗都画成曲线；当天没有采样就断线，不补 0（口径见 dayTrendPoints）。
  const points: TrendPoint[] = dayTrendPoints(trend);
  const hasAny = trend.some((d) => d.known > 0);

  return (
    <div className="bcard">
      <div className="btitle">
        <span>近 7 日积分变化</span>
        {onOpenMonths && (
          <button className="btitle-btn" onClick={onOpenMonths} title="去统计页看本月每日与本年度 12 个月">
            月度 ›
          </button>
        )}
      </div>
      {!hasAny ? (
        <div className="ev-empty">还没有采样数据，点右上「刷新积分」跑一次就有</div>
      ) : (
        <>
          <TrendLegend visible={visible} onToggle={toggle} />
          <TrendChart points={points} visible={visible} height={140} />
        </>
      )}
    </div>
  );
}

function ActivityCard({ events }: { events: StateView["events"] }) {
  const list = events.slice(0, 4);
  const [open, setOpen] = useState(false);
  return (
    <div className="bcard">
      <div className="btitle">
        最近活动
        {events.length > list.length && (
          <button className="btitle-btn" onClick={() => setOpen(true)}>
            更多
          </button>
        )}
      </div>
      {list.length === 0 ? (
        <div className="ev-empty">还没有签到记录，跑一次就有</div>
      ) : (
        list.map((e, i) => <EventRow key={`${e.at}-${e.account_id}-${i}`} e={e} />)
      )}
      {open && (
        <Modal title="最近活动" className="modal-ev" onClose={() => setOpen(false)}>
          <div className="ev-list">
            {events.map((e, i) => (
              <EventRow key={`${e.at}-${e.account_id}-${i}`} e={e} />
            ))}
          </div>
        </Modal>
      )}
    </div>
  );
}

/// 一条活动流水：卡片只取前 4 条，弹窗里复用同一行，避免两处口径不一致。
function EventRow({ e }: { e: StateView["events"][number] }) {
  return (
    <div className="ev">
      <b>{e.label}</b>
      <span className="ev-vendor">{VENDOR_NAME[e.vendor] ?? e.vendor}</span>
      <span className="ev-text">{e.text}</span>
      {e.delta !== null && e.delta > 0 && (
        <span className="ev-delta">
          +{e.delta}
          <PointsIcon tone="jade" size={8} />
        </span>
      )}
      <time>{clock(e.at)}</time>
    </div>
  );
}

/// 账号副标题的悬停提示：指纹只在提示里出现，不摆在行内。
///
/// 两页共用一份文案——令牌指纹本身不是身份标识（续期后会变），
/// 所以必须说明它「只用于区分凭据」，否则又会被当成账号 id。
function accountMetaTitle(a: AccountView): string {
  return [
    `令牌指纹 ${a.hint}｜仅用于区分同一平台的多份凭据，重新导入后会变`,
    `签到时间：${a.schedule_text}`,
  ].join("\n");
}

/// 导入来源的界面用词；老数据没有来源时返回空串（界面只显示签到时间）。
function importSourceText(source: AccountView["source"]): string {
  switch (source) {
    case "local":
      return "本机导入";
    case "browser":
      return "浏览器授权";
    case "token":
      return "手动令牌";
    default:
      return "";
  }
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
  onOpenSettings: (section?: SettingsSectionId) => void;
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
  // 就地改名：editingId 指向正在编辑的行；cancelRename 用来吞掉 Esc 之后的 blur 提交。
  const [editingId, setEditingId] = useState<string | null>(null);
  const cancelRename = useRef(false);
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

  const startLocalImport = async (allowKeyFetch = false) => {
    setImporting(true);
    try {
      const msg = await api.localImport(vendor, allowKeyFetch);
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
        // 留空交给后端：先试平台昵称，拿不到才用「平台 账号」。
        // 传默认名会被当成「用户自定义」，昵称就永远覆盖不上。
        label,
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

  // 按平台分组，顺序跟能力表——平台顺序是产品决定，不该被导入顺序带跑。
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
        accounts: list,
      };
    });
  }, [view.accounts, view.platforms]);

  const saveName = async (a: AccountView, raw: string) => {
    setEditingId(null);
    const next = raw.trim();
    if (!next || next === a.label) return;
    try {
      await api.renameAccount(a.id, next);
      onChanged(await api.bootstrap());
      notify(`已改名为「${next}」`);
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
          <h1>
            账号
            <span className="head-count">{view.accounts.length} 个</span>
          </h1>
          <p className="sub">同一平台下可以有多个账号，各自独立定时、独立凭据</p>
        </div>
      </div>

      <div className="split">
        <div className="panels">
          {view.accounts.length === 0 ? (
            <section className="panel">
              <p className="panel-empty">还没有账号。右侧导入第一个。</p>
            </section>
          ) : (
            groups.map((g) => (
              <section className="panel" key={g.vendor}>
                <div className="phead">
                  <VendorIcon vendor={g.vendor} size={20} />
                  <span className="pname">{g.display}</span>
                  <span className="psub">{g.accounts.length} 账号</span>
                </div>
                <ul className="acct-list">
                  {g.accounts.map((a) => (
                    <li key={a.id} className="acct-row">
                      <div className="acct-main">
                        <div className="acct-title">
                          {editingId === a.id ? (
                            <input
                              className="acct-name-input"
                              defaultValue={a.label}
                              maxLength={24}
                              autoFocus
                              onKeyDown={(e) => {
                                if (e.key === "Enter") e.currentTarget.blur();
                                if (e.key === "Escape") {
                                  cancelRename.current = true;
                                  e.currentTarget.blur();
                                }
                              }}
                              onBlur={(e) => {
                                if (cancelRename.current) {
                                  cancelRename.current = false;
                                  setEditingId(null);
                                  return;
                                }
                                void saveName(a, e.currentTarget.value);
                              }}
                            />
                          ) : (
                            <button
                              className="acct-name"
                              title="点击改名"
                              onClick={() => setEditingId(a.id)}
                            >
                              {a.label}
                            </button>
                          )}
                          {!a.enabled && <span className="tag tag-muted">已停用</span>}
                          {!a.has_credential && <span className="tag tag-warn">缺凭据</span>}
                        </div>
                        <div
                          className="acct-sub"
                          title={accountMetaTitle(a)}
                        >
                          {[importSourceText(a.source), a.schedule_text]
                            .filter(Boolean)
                            .join(" · ")}
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
              </section>
            ))
          )}
        </div>

        <section className="panel">
          <div className="phead">
            <span className="pname">导入账号</span>
          </div>

          <div className="pbody">
            <div className="field">
              <label>平台</label>
              {/* 平台会越来越多：等宽网格自动换行，不依赖固定 3 格滑轨 */}
              <div className="seg seg-platforms">
                {view.platforms.map((p) => (
                  <button
                    key={p.vendor}
                    className={`seg-item ${vendor === p.vendor ? "active" : ""}`}
                    onClick={() => setVendor(p.vendor)}
                    title={p.display}
                  >
                    <VendorIcon vendor={p.vendor} size={15} />
                    <span className="seg-label">{p.display}</span>
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
                  <li>
                    <strong>打开授权页</strong>：点击下方按钮，系统默认浏览器会新开一个标签页
                    （不会新开窗口）。
                  </li>
                  <li>
                    <strong>完成授权</strong>：在该页面登录，并点击「授权」；看到成功提示后即可关闭标签页。
                  </li>
                  <li>
                    <strong>确认结果</strong>：返回本应用，导入会自动完成并显示账户余额，
                    用于确认令牌确实可用。
                  </li>
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
                  <>
                    <p
                      className={
                        localStatus.available || localStatus.needs_key ? "hint" : "hint warn"
                      }
                    >
                      {localStatus.detail}
                    </p>
                    {localStatus.needs_key && (
                      <p className="hint">
                        导入时会<strong>无窗口、不联网</strong>地短暂启动一次{" "}
                        {cap?.display ?? "该平台"}客户端读取本地密钥，读完立即退出。
                      </p>
                    )}
                    {(localStatus.available || localStatus.needs_key) && (
                      <>
                        <LocalImportSteps />
                        <button
                          className="btn primary block"
                          onClick={() => void startLocalImport(Boolean(localStatus.needs_key))}
                          disabled={importing || busy !== null}
                        >
                          {importing ? "正在导入…" : `从本机导入 ${cap?.display ?? vendor}`}
                        </button>
                      </>
                    )}
                  </>
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

            <button className="link-btn" onClick={() => onOpenSettings("data")}>
              凭据存在哪？去「设置 → 数据与安全 → 数据位置」查看
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

/// 本机导入的说明步骤。三个平台共用同一份，避免某个平台因为“需要先授权”
/// 就退化成一行警告，面板结构和其它平台对不上。
function LocalImportSteps() {
  return (
    <ol className="steps">
      <li>
        只读取本机已登录客户端的登录态，<strong>不改动</strong>客户端文件
      </li>
      <li>导入前会联网验证一次余额；验证不过不会留下账号</li>
      <li>导入后由本应用负责签到，同一账号不要在两处同时签到</li>
      <li>同一个账号重复导入会自动更新凭据，不会出现两条记录</li>
    </ol>
  );
}

function NavItem({
  active,
  onClick,
  label,
  badge,
  icon,
}: {
  active: boolean;
  onClick: () => void;
  label: string;
  badge?: number;
  icon: ReactNode;
}) {
  return (
    <button className={`nav ${active ? "on" : ""}`} onClick={onClick}>
      <span className="nav-ico">{icon}</span>
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
