// 与 Rust 侧 `StateView` 一一对应。改 Rust 结构时这里必须同步，否则运行期静默变 undefined。

export type ThemeMode = "light" | "dark" | "system";
export type BallContent = "total_credits" | "pending_count";

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

export interface Settings {
  theme: ThemeMode;
  autostart: boolean;
  minimize_to_tray: boolean;
  notify_on_checkin: boolean;
  notify_on_failure: boolean;
  ball: BallSettings;
  sync: SyncSettings;
  scheduler_enabled: boolean;
  catch_up_missed: boolean;
}

export interface Aggregate {
  total_balance: number;
  account_count: number;
  pending_count: number;
  balance_known: number;
  last_sampled_at: number | null;
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
  settings: Settings;
  platforms: PlatformCap[];
}

export interface OutcomeView {
  account_id: string;
  label: string;
  status: string;
  status_text: string;
  message: string;
  credit: number | null;
}
