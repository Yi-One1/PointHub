import { invoke } from "@tauri-apps/api/core";
import type {
  ClientStatus,
  FirewallStatus,
  LanInfo,
  LocalImportStatus,
  OutcomeView,
  Settings,
  StateView,
  UpdateApply,
  UpdateCheck,
} from "./types";

// 所有后端调用集中在这里：前端组件不直接写命令名，
// 命令改名时只改这一处。

export const api = {
  bootstrap: () => invoke<StateView>("bootstrap"),
  lanInfo: () => invoke<LanInfo>("lan_info"),
  /** 手机访问端口的防火墙放行状态（只读检测） */
  firewallStatus: () => invoke<FirewallStatus>("firewall_status"),
  /** 添加只开当前端口的入站放行规则（会弹 UAC） */
  firewallAllow: () => invoke<FirewallStatus>("firewall_allow"),
  /** 清理旧版本残留的 PointHub 防火墙规则（会弹 UAC） */
  firewallCleanup: () => invoke<FirewallStatus>("firewall_cleanup"),
  refreshAll: () => invoke<StateView>("refresh_all"),
  checkin: (ids?: string[]) => invoke<OutcomeView[]>("checkin", { ids: ids ?? null }),
  travel: (id: string, action: "auto" | "status" | "claim" | "depart" = "auto", location?: number) =>
    invoke<string>("travel", { id, action, location: location ?? null }),
  addAccount: (payload: {
    vendor: string;
    label: string;
    secret: string;
    secret_kind: "refresh" | "access";
    device_id?: string;
    domain?: string;
    cookie?: string;
  }) => invoke<string>("add_account", { payload }),
  removeAccount: (id: string) => invoke<void>("remove_account", { id }),
  /** 扫描本机客户端登录态（只读、不联网） */
  localImportScan: () => invoke<LocalImportStatus[]>("local_import_scan"),
  /** 从本机客户端登录态导入；后端先验证余额再落库 */
  localImport: (vendor: string) => invoke<string>("local_import", { vendor }),
  /** 打开该平台官方登录页；登录成功后后端自动建账号并发 ph://login 事件 */
  browserLogin: (vendor: string) => invoke<void>("browser_login", { vendor }),
  setAccountEnabled: (id: string, enabled: boolean) =>
    invoke<void>("set_account_enabled", { id, enabled }),
  saveSettings: (settings: Settings) => invoke<StateView>("save_settings", { settings }),
  setBall: (enabled: boolean) => invoke<void>("set_ball", { enabled }),
  setBallExpanded: (expanded: boolean) => invoke<void>("set_ball_expanded", { expanded }),
  setBallAccount: (accountId: string | null) =>
    invoke<void>("set_ball_account", { accountId }),
  openDataDir: () => invoke<void>("open_data_dir"),
  relocateData: (path: string) => invoke<string>("relocate_data", { path }),
  showMain: () => invoke<void>("show_main"),
  quit: () => invoke<void>("quit_app"),
  /** 远程模式：当前连接状态 */
  clientStatus: () => invoke<ClientStatus>("client_status"),
  /** 远程模式：用配对码连接电脑 */
  clientPair: (baseUrl: string, code: string) =>
    invoke<ClientStatus>("client_pair", { baseUrl, code }),
  /** 远程模式：断开并回到本机模式 */
  clientUnpair: () => invoke<ClientStatus>("client_unpair"),
  /** 查最新版本；latest 为 null 表示已是最新 */
  updateCheck: () => invoke<UpdateCheck>("update_check"),
  /** 下载并交给系统安装（安卓走浏览器） */
  updateApply: () => invoke<UpdateApply>("update_apply"),
  /** 手动下载兜底：打开 Releases 页面 */
  updateOpenPage: () => invoke<void>("update_open_page"),
};
