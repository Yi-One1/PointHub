import { invoke } from "@tauri-apps/api/core";
import type {
  ClientStatus,
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
  /** 改账号名；改过之后重复导入不再用平台昵称覆盖 */
  renameAccount: (id: string, label: string) =>
    invoke<string>("rename_account", { id, label }),
  /** 扫描本机客户端登录态（只读、不联网） */
  localImportScan: () => invoke<LocalImportStatus[]>("local_import_scan"),
  /** 从本机客户端登录态导入；后端先验证余额再落库。allowKeyFetch 只在用户明确同意后传 true。 */
  localImport: (vendor: string, allowKeyFetch = false) =>
    invoke<string>("local_import", { vendor, allowKeyFetch }),
  /** 打开该平台官方登录页；登录成功后后端自动建账号并发 ph://login 事件 */
  browserLogin: (vendor: string) => invoke<void>("browser_login", { vendor }),
  setAccountEnabled: (id: string, enabled: boolean) =>
    invoke<void>("set_account_enabled", { id, enabled }),
  saveSettings: (settings: Settings) => invoke<StateView>("save_settings", { settings }),
  /** 各分区的出厂默认值，用于「恢复默认设置」 */
  defaultSettings: () => invoke<Settings>("default_settings"),
  setBall: (enabled: boolean) => invoke<void>("set_ball", { enabled }),
  setBallExpanded: (expanded: boolean) => invoke<void>("set_ball_expanded", { expanded }),
  setBallAccount: (accountId: string | null) =>
    invoke<void>("set_ball_account", { accountId }),
  openDataDir: () => invoke<void>("open_data_dir"),
  /** 默认数据目录：发行版为程序目录下的 data，预览为项目里的 dist\data */
  defaultDataDir: () => invoke<string>("default_data_dir"),
  relocateData: (path: string) => invoke<string>("relocate_data", { path }),
  /** 把数据搬回默认目录，重启后生效并清理旧目录 */
  resetDataDir: () => invoke<string>("reset_data_dir"),
  /** 打开系统「选择文件夹」对话框；取消返回 null */
  pickDataDir: () => invoke<string | null>("pick_data_dir"),
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
  /** 下载并交给系统安装（安卓走应用内安装器） */
  updateApply: () => invoke<UpdateApply>("update_apply"),
  /** 手动下载兜底：打开 Releases 页面 */
  updateOpenPage: () => invoke<void>("update_open_page"),
};
