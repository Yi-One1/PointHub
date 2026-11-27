import { useEffect, useRef, useState } from "react";
import type { ReactElement } from "react";
import { api } from "./api";
import {
  IconAbout,
  IconAppearance,
  IconBack,
  IconCheckin,
  IconData,
  IconGeneral,
  IconPhone,
  IconSearch,
  IconUpdate,
} from "./icons";
import {
  SETTINGS_FIELD_SECTIONS,
  SETTINGS_SECTIONS,
  type SettingsSectionId,
} from "./settings";
import { Modal, Switch } from "./ui";
import { VendorIcon } from "./VendorIcon";
import type {
  ClientStatus,
  LanInfo,
  Settings,
  StateView,
  ThemeMode,
  UpdateCheck,
} from "./types";

const SECTION_ICONS: Record<SettingsSectionId, () => ReactElement> = {
  general: IconGeneral,
  appearance: IconAppearance,
  checkin: IconCheckin,
  phone: IconPhone,
  update: IconUpdate,
  data: IconData,
  about: IconAbout,
};

/**
 * 独立设置页：左侧分类、右侧分区卡片（参考 Trae / QQ 电脑端）。
 *
 * 第 1 阶段只做入口与布局迁移，保存方式仍是底部的「还原 / 保存」；
 * 即时生效、分区恢复默认和二次确认在第 2 阶段处理。
 */
export default function SettingsScreen({
  view,
  onSave,
  notify,
  onClose,
}: {
  view: StateView;
  onSave: (s: Settings, silent?: boolean) => Promise<boolean>;
  notify: (t: string, k?: "ok" | "warn") => void;
  onClose: () => void;
}) {
  const [active, setActive] = useState(SETTINGS_SECTIONS[0].id);
  const [query, setQuery] = useState("");
  const [draft, setDraft] = useState<Settings>(view.settings);
  const [defaults, setDefaults] = useState<Settings | null>(null);
  const [pending, setPending] = useState<{
    title: string;
    message: string;
    confirmText: string;
    run: () => void;
  } | null>(null);
  const mirrorTimer = useRef<number | undefined>(undefined);
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
      void apply({ ...draft, mode: "remote" });
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
      void apply({ ...draft, mode: "local" });
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

  // Esc 退出设置页，和常见桌面软件一致。
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") onClose();
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [onClose]);

  useEffect(() => {
    api
      .defaultSettings()
      .then(setDefaults)
      .catch(() => undefined);
  }, []);

  useEffect(
    () => () => {
      if (mirrorTimer.current) window.clearTimeout(mirrorTimer.current);
    },
    [],
  );

  const apply = async (next: Settings) => {
    setDraft(next);
    await onSave(next, true);
  };

  const change = (next: Settings, confirm?: { title: string; message: string }) => {
    if (confirm) {
      setPending({
        ...confirm,
        confirmText: "确认",
        run: () => {
          void apply(next);
        },
      });
    } else {
      void apply(next);
    }
  };

  // 镜像前缀是输入框：先更新界面，停顿 500ms 再落盘，避免每敲一个字都保存。
  const patchMirror = (mirror: string) => {
    const next = { ...draft, update: { ...draft.update, mirror } };
    setDraft(next);
    if (mirrorTimer.current) window.clearTimeout(mirrorTimer.current);
    mirrorTimer.current = window.setTimeout(() => {
      void apply(next);
    }, 500);
  };

  const current = SETTINGS_SECTIONS.find((s) => s.id === active) ?? SETTINGS_SECTIONS[0];
  const sectionKeys = (Object.keys(SETTINGS_FIELD_SECTIONS) as (keyof Settings)[]).filter(
    (key) => SETTINGS_FIELD_SECTIONS[key] === active,
  );
  const canReset = sectionKeys.length > 0 && defaults !== null;

  const resetSection = () => {
    if (!defaults) return;
    setPending({
      title: `恢复「${current.label}」默认设置？`,
      message: "只重置本分区的设置，其他分区不受影响；恢复后立即生效。",
      confirmText: "恢复默认",
      run: () => {
        const next = { ...draft };
        const target = next as unknown as Record<string, unknown>;
        const source = defaults as unknown as Record<string, unknown>;
        for (const key of sectionKeys) target[key] = source[key];
        void apply(next);
      },
    });
  };

  const q = query.trim().toLowerCase();
  const visibleSections = q
    ? SETTINGS_SECTIONS.filter(
        (s) =>
          s.label.toLowerCase().includes(q) ||
          s.keywords.some((k) => k.toLowerCase().includes(q)),
      )
    : SETTINGS_SECTIONS;

  return (
    <div className="settings-screen" role="dialog" aria-modal="true" aria-label="设置">
      <div className="settings-main">
        <aside className="settings-nav" aria-label="设置分类">
          <button className="settings-back" onClick={onClose}>
            <IconBack />
            <span>返回应用</span>
          </button>

          <label className="settings-search">
            <IconSearch />
            <input
              value={query}
              onChange={(e) => setQuery(e.target.value)}
              placeholder="搜索设置…"
              spellCheck={false}
            />
          </label>

          <div className="settings-nav-list">
            {visibleSections.map((s) => {
              const SectionIcon = SECTION_ICONS[s.id];
              return (
                <button
                  key={s.id}
                  className={`settings-nav-item ${active === s.id ? "on" : ""}`}
                  onClick={() => setActive(s.id)}
                >
                  <span className="settings-nav-ico">
                    <SectionIcon />
                  </span>
                  <span>{s.label}</span>
                </button>
              );
            })}
            {visibleSections.length === 0 && (
              <div className="settings-nav-empty">没有匹配的分类</div>
            )}
          </div>
        </aside>

        <section className="settings-body">
          <div className="settings-body-head">
            <div>
              <h1>{current.label}</h1>
              <p className="sub">{current.hint}</p>
            </div>
            <div className="acts">
              <button
                className="btn"
                onClick={resetSection}
                disabled={!canReset}
                title={canReset ? undefined : "本分区没有可恢复的设置"}
              >
                恢复默认设置
              </button>
            </div>
          </div>

          {active === "general" && (
            <>
              <h2 className="settings-group">启动与关闭</h2>
              <section className="panel">
              {isPhone ? (
                <div className="row">
                  <div className="row-desc">
                    这一分区只在电脑端可用；手机端由系统负责启动与后台运行。
                  </div>
                </div>
              ) : (
                <>
                  <div className="row">
                    <div>
                      <div className="row-title">开机自启动</div>
                      <div className="row-desc">写入当前用户启动项，不需要管理员权限</div>
                    </div>
                    <Switch
                      checked={draft.autostart}
                      onChange={(v) => change({ ...draft, autostart: v })}
                      label="开机自启"
                    />
                  </div>

                  <div className="row">
                    <div>
                      <div className="row-title">关闭窗口时最小化到托盘</div>
                      <div className="row-desc">关掉后点 × 会直接退出程序，自动签到也会停止</div>
                    </div>
                    <Switch
                      checked={draft.minimize_to_tray}
                      onChange={(v) =>
                        change(
                          { ...draft, minimize_to_tray: v },
                          v
                            ? undefined
                            : {
                                title: "关闭「最小化到托盘」？",
                                message:
                                  "关闭后点窗口右上角 × 会直接退出程序，自动签到也会停止。",
                              },
                        )
                      }
                      label="最小化到托盘"
                    />
                  </div>
                </>
              )}
              </section>
            </>
          )}

          {active === "appearance" && (
            <>
              <h2 className="settings-group">主题与桌面</h2>
              <section className="panel">

              <div className="row">
                <div>
                  <div className="row-title">主题</div>
                  <div className="row-desc">
                    {isPhone
                      ? "「跟随系统」会随手机的深色模式切换"
                      : "「跟随系统」会随 Windows 的浅色/深色切换"}
                  </div>
                </div>
                <div className="seg">
                  {(["light", "dark", "system"] as ThemeMode[]).map((m) => (
                    <button
                      key={m}
                      className={`seg-item ${draft.theme === m ? "active" : ""}`}
                      onClick={() => change({ ...draft, theme: m })}
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
                      onChange={(v) => change({ ...draft, ball: { ...draft.ball, enabled: v } })}
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
                      onChange={(e) =>
                        change({ ...draft, ball: { ...draft.ball, selected: e.target.value || null } })
                      }
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
                        onClick={() => change({ ...draft, ball: { ...draft.ball, show: "total_credits" } })}
                      >
                        积分
                      </button>
                      <button
                        className={`seg-item ${draft.ball.show === "pending_count" ? "active" : ""}`}
                        onClick={() => change({ ...draft, ball: { ...draft.ball, show: "pending_count" } })}
                      >
                        待签数
                      </button>
                    </div>
                  </div>
                </>
              )}
              </section>
            </>
          )}

          {active === "checkin" && (
            <>
              <h2 className="settings-group">签到与提醒</h2>
              <section className="panel">

              <div className="row">
                <div>
                  <div className="row-title">定时自动签到</div>
                  <div className="row-desc">按每个账号的平台刷新时间执行，每 45 秒核对一次</div>
                </div>
                <Switch
                  checked={draft.scheduler_enabled}
                  onChange={(v) =>
                    change(
                      { ...draft, scheduler_enabled: v },
                      v
                        ? undefined
                        : {
                            title: "关闭定时自动签到？",
                            message: "关闭后所有账号都不再自动签到，只能手动点「签到」。",
                          },
                    )
                  }
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
                  onChange={(v) =>
                    change(
                      { ...draft, catch_up_missed: v },
                      v
                        ? undefined
                        : {
                            title: "关闭错过补签？",
                            message: "关闭后当天错过的签到窗口不再自动补签。",
                          },
                    )
                  }
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
                        onClick={() => change({ ...draft, auto_refresh_minutes: o.v })}
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
                    onClick={() =>
                      change({ ...draft, notify_on_checkin: !draft.notify_on_checkin })
                    }
                  >
                    成功 {draft.notify_on_checkin ? "开" : "关"}
                  </button>
                  <button
                    className={`seg-item ${draft.notify_on_failure ? "active" : ""}`}
                    onClick={() =>
                      change({ ...draft, notify_on_failure: !draft.notify_on_failure })
                    }
                  >
                    失败 {draft.notify_on_failure ? "开" : "关"}
                  </button>
                </div>
              </div>
            </section>
            </>
          )}

          {active === "phone" &&
            (isPhone ? (
              <>
                <h2 className="settings-group">连接方式</h2>
                <section className="panel">

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
                      onClick={() => change({ ...draft, mode: "local" })}
                    >
                      本机模式
                    </button>
                    <button
                      className={`seg-item ${draft.mode === "remote" ? "active" : ""}`}
                      onClick={() => change({ ...draft, mode: "remote" })}
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
                        <div className="row-desc">
                          断开连接会同时回到本机模式；账号仍留在电脑上。
                        </div>
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
              </>
            ) : (
              <>
                <h2 className="settings-group">手机访问</h2>
                <section className="panel">

                <div className="row">
                  <div>
                    <div className="row-title">局域网访问</div>
                    <div className="row-desc">
                      手机和电脑连同一个 Wi-Fi，用手机浏览器打开地址即可；开关即时生效
                    </div>
                  </div>
                  <Switch
                    checked={draft.sync.enabled}
                    onChange={(v) =>
                    change(
                      { ...draft, sync: { ...draft.sync, enabled: v } },
                      v
                        ? undefined
                        : {
                            title: "关闭局域网访问？",
                            message: "关闭后手机将立即无法连接这台电脑。",
                          },
                    )
                  }
                    label="局域网访问"
                  />
                </div>

                {lan?.running ? (
                  <>
                    <div className="row">
                      <div>
                        <div className="row-title">访问地址</div>
                        <div className="row-desc">
                          {lan.url ?? "没取到本机内网地址，检查是否连着 Wi-Fi"}
                        </div>
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
                        : "局域网访问已关闭，打开开关后立即生效"}
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
                    onChange={(v) =>
                    change(
                      { ...draft, sync: { ...draft.sync, allow_remote_checkin: v } },
                      v
                        ? undefined
                        : {
                            title: "关闭手机签到？",
                            message: "关闭后手机只能查看账本，不能替你点签到。",
                          },
                    )
                  }
                    label="允许手机签到"
                  />
                </div>
                </section>
              </>
            ))}

          {active === "update" && (
            <>
              <h2 className="settings-group">更新</h2>
              <section className="panel">

              <div className="row">
                <div>
                  <div className="row-title">启动时检查更新</div>
                  <div className="row-desc">发现新版本只弹通知，不会自动安装</div>
                </div>
                <Switch
                  checked={draft.update.enabled}
                  onChange={(v) => change({ ...draft, update: { ...draft.update, enabled: v } })}
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
                    onClick={() => change({ ...draft, update: { ...draft.update, source: "github" } })}
                  >
                    GitHub
                  </button>
                  <button
                    className={`seg-item ${draft.update.source === "mirror" ? "active" : ""}`}
                    onClick={() => change({ ...draft, update: { ...draft.update, source: "mirror" } })}
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
                    onChange={(e) => patchMirror(e.target.value)}
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
                    onClick={() =>
                      api.updateOpenPage().catch((e) => notify(String(e), "warn"))
                    }
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
                        ? "下载完成后由系统安装器确认安装"
                        : "下载完成后自动打开安装程序"}
                    </div>
                  </div>
                  <button
                    className="btn primary"
                    onClick={applyUpdate}
                    disabled={applying || !update.latest.asset}
                  >
                    {applying ? "处理中…" : "下载并安装"}
                  </button>
                </div>
              )}
              </section>
            </>
          )}

          {active === "data" && (
            <>
              <h2 className="settings-group">数据位置</h2>
              <section className="panel">
                <div className="row">
                <div>
                  <div className="row-title">数据目录</div>
                  <div className="row-desc mono">{view.data_dir}</div>
                </div>
                <button
                  className="btn"
                  onClick={() => api.openDataDir().catch((e) => notify(String(e), "warn"))}
                >
                  打开
                </button>
              </div>

              <div className="row">
                <div>
                  <div className="row-title">更换数据目录</div>
                  <div className="row-desc">会复制现有数据到新位置，重启后生效</div>
                </div>
                <div className="inline-form">
                  <input
                    value={newDir}
                    onChange={(e) => setNewDir(e.target.value)}
                    placeholder="D:\PointHub"
                  />
                  <button
                    className="btn"
                    disabled={!newDir.trim()}
                    onClick={() =>
                      setPending({
                        title: "更换数据目录？",
                        message: `会把现有数据复制到 ${newDir.trim()}，重启应用后生效。`,
                        confirmText: "开始搬迁",
                        run: () => {
                          void (async () => {
                            try {
                              notify(await api.relocateData(newDir.trim()));
                            } catch (e) {
                              notify(String(e), "warn");
                            }
                          })();
                        },
                      })
                    }
                  >
                    搬迁
                  </button>
                </div>
              </div>
              </section>

              <h2 className="settings-group">安全</h2>
              <section className="panel">
                <div className="row">
                  <div>
                    <div className="row-title">数据只在本机</div>
                    <div className="row-desc">
                      账号、凭据与历史只保存在这台设备，不上传任何服务器。
                    </div>
                  </div>
                  <span className="tag tag-ok">仅本机</span>
                </div>

                <div className="row">
                  <div>
                    <div className="row-title">凭据保护</div>
                  <div className="row-desc">
                    {isPhone
                      ? `${view.vault_kind}，卸载或清除数据后需重新导入`
                      : `${view.vault_kind}，绑定当前 Windows 用户；换机后需重新导入`}
                  </div>
                </div>
                <span className="tag tag-ok">已加密</span>
              </div>
              </section>
            </>
          )}

          {active === "about" && (
            <>
              <h2 className="settings-group">平台能力</h2>
              <section className="panel">
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
                          {p.verified ? (
                            <span className="tag tag-ok">已实机验证</span>
                          ) : (
                            <span className="tag tag-muted">待实测</span>
                          )}
                        </td>
                      </tr>
                    ))}
                  </tbody>
                </table>
              </section>

              <h2 className="settings-group">关于</h2>
              <section className="panel">
                <div className="row">
                  <div>
                    <div className="row-title">积分枢纽 PointHub v{view.version}</div>
                    <div className="row-desc">
                      使用非公开接口，可能因平台改版失效；请仅管理本人持有的账号。
                    </div>
                  </div>
                  <button
                    className="btn"
                    onClick={() => api.quit().catch(() => undefined)}
                  >
                    退出程序
                  </button>
                </div>
              </section>
            </>
          )}
        </section>
      </div>

      {pending && (
        <Modal title={pending.title} onClose={() => setPending(null)}>
          <p className="modal-text">{pending.message}</p>
          <div className="modal-actions">
            <button className="btn" onClick={() => setPending(null)}>
              取消
            </button>
            <button
              className="btn primary"
              onClick={() => {
                const run = pending.run;
                setPending(null);
                run();
              }}
            >
              {pending.confirmText}
            </button>
          </div>
        </Modal>
      )}
    </div>
  );
}

function yes(b: boolean) {
  return b ? "支持" : "—";
}
