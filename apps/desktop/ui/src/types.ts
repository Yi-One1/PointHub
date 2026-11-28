// 与 Rust 侧 `StateView` 一一对应。改 Rust 结构时这里必须同步，否则运行期静默变 undefined。

export type ThemeMode = "light" | "dark" | "system";
export type BallContent = "total_credits" | "pending_count";
/** 运行模式：安卓端可选「本机」或「连接电脑」；桌面端固定本机。 */
export type AppMode = "local" | "remote";

export interface BallSettings {
  enabled: boolean;
  x: number | null;
  y: number | null;
  size: number;
  opacity: number;
  show: BallContent;
  /** 球面绑定的账号；null = 全部合计 */
  selected: string | null;
}

export interface SyncSettings {
  enabled: boolean;
  port: number;
  allow_remote_checkin: boolean;
}

/** 更新来源：直连 GitHub / 走镜像前缀 */
export type UpdateSource = "github" | "mirror";

export interface UpdateSettings {
  enabled: boolean;
  source: UpdateSource;
  /** 镜像前缀，拼在完整 GitHub 地址前面 */
  mirror: string;
}

/** 发布里当前平台的安装包 */
export interface ReleaseAsset {
  name: string;
  url: string;
  size: number | null;
}

export interface ReleaseInfo {
  version: string;
  notes: string;
  page_url: string;
  published_at: string | null;
  asset: ReleaseAsset | null;
}

export interface UpdateCheck {
  current: string;
  /** null = 已是最新 */
  latest: ReleaseInfo | null;
  source: UpdateSource;
}

export interface UpdateApply {
  /** latest | installer | downloaded | permission */
  action: string;
  detail: string;
  version: string;
  path: string | null;
}

export interface LanInfo {
  enabled: boolean;
  running: boolean;
  url: string | null;
  port: number;
  code: string | null;
  code_left_secs: number;
  allow_checkin: boolean;
}

export interface Settings {
  mode: AppMode;
  theme: ThemeMode;
  autostart: boolean;
  minimize_to_tray: boolean;
  notify_on_checkin: boolean;
  notify_on_failure: boolean;
  ball: BallSettings;
  sync: SyncSettings;
  scheduler_enabled: boolean;
  catch_up_missed: boolean;
  auto_refresh_minutes: number;
  update: UpdateSettings;
}

export interface Aggregate {
  total_balance: number;
  account_count: number;
  pending_count: number;
  balance_known: number;
  last_sampled_at: number | null;
  /** 本月签到实际到账的积分；null = 数据来源没提供（例如旧版电脑端） */
  claimed_this_month: number | null;
}

export interface AccountView {
  id: string;
  vendor: string;
  vendor_display: string;
  label: string;
  hint: string;
  enabled: boolean;
  checkin_enabled: boolean;
  travel_enabled: boolean;
  schedule_text: string;
  balance: number | null;
  checked_in_today: boolean | null;
  streak_days: number | null;
  today_gain: number | null;
  travel_state: string | null;
  travel_location: string | null;
  captured_at: number | null;
  last_checkin_at: number | null;
  /** 失败后的自动重试最早时间（epoch 秒）；用于展示「稍后自动重试」。 */
  retry_after: number | null;
  last_error: string | null;
  has_credential: boolean;
  can_checkin: boolean;
  can_travel: boolean;
  verified: boolean;
}

export interface PlatformCap {
  vendor: string;
  display: string;
  checkin: boolean;
  travel: boolean;
  balance: boolean;
  token_refresh: boolean;
  verified: boolean;
}

/** 近 7 日里的一天 */
export interface DayPoint {
  date: string;
  total: number;
  known: number;
  /** 相对前一天的新增；前一天无数据时为 null */
  gain: number | null;
  /** 各账号正增长之和；与后端 DayPoint.up 同口径 */
  up: number | null;
  /** 各账号下降之和（正数，表示消耗了多少）；与后端 DayPoint.down 同口径 */
  down: number | null;
}

/** 一条活动流水 */
export interface ActivityEvent {
  at: number;
  account_id: string;
  label: string;
  vendor: string;
  kind: string;
  delta: number | null;
  text: string;
}

export interface StateView {
  version: string;
  data_dir: string;
  vault_kind: string;
  history_count: number;
  accounts: AccountView[];
  aggregate: Aggregate;
  trend: DayPoint[];
  events: ActivityEvent[];
  next_run_at: number | null;
  /** 今天到点但仍未签成功、且有错误信息的账号。 */
  checkin_alerts: CheckinAlert[];
  settings: Settings;
  platforms: PlatformCap[];
  mode: AppMode;
  remote: RemoteLink | null;
}

/** 今日签到异常（总览页提示用）。 */
export interface CheckinAlert {
  account_id: string;
  label: string;
  message: string;
}

/** 本机导入：某平台在当前电脑上的可用性 */
export interface LocalImportStatus {
  vendor: string;
  display: string;
  available: boolean;
  /** 登录态已找到，但读取它需要用户允许一次额外的本地操作 */
  needs_key: boolean;
  detail: string;
}

/** 远程模式下的连接信息 */
export interface RemoteLink {
  paired: boolean;
  base_url: string | null;
  server_name: string | null;
  device_id: string | null;
  error: string | null;
}

export interface ClientStatus {
  mode: AppMode;
  paired: boolean;
  base_url: string | null;
  server_name: string | null;
  device_id: string | null;
}

export interface OutcomeView {
  account_id: string;
  label: string;
  status: string;
  status_text: string;
  message: string;
  credit: number | null;
}
