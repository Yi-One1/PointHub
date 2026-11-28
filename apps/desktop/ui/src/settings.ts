import type { Settings } from "./types";

export type SettingsSectionId =
  | "general"
  | "appearance"
  | "checkin"
  | "phone"
  | "update"
  | "data"
  | "about";

export interface SettingsSection {
  id: SettingsSectionId;
  label: string;
  hint: string;
  /** 搜索关键词：用户可能输入的说法，不必与标题完全一致。 */
  keywords: string[];
}

/**
 * 设置页的固定分类（顺序即左侧导航顺序）。
 *
 * 以后新增设置项时：先看能不能放进已有分类；确实放不下才新增分类，
 * 并同步更新本表、字段归属表和 `test/settings.test.mjs`。
 */
export const SETTINGS_SECTIONS: SettingsSection[] = [
  { id: "general", label: "通用", hint: "启动与关闭程序时的行为", keywords: ["启动", "自启", "开机", "托盘", "关闭", "窗口"] },
  { id: "appearance", label: "外观与桌面", hint: "主题与桌面悬浮球", keywords: ["主题", "浅色", "深色", "悬浮球", "桌面", "外观"] },
  { id: "checkin", label: "签到与通知", hint: "自动签到、补签与提醒", keywords: ["签到", "补签", "刷新", "通知", "提醒", "定时"] },
  { id: "phone", label: "手机访问", hint: "局域网连接与设备配对", keywords: ["手机", "局域网", "Wi-Fi", "配对", "端口", "连接"] },
  { id: "update", label: "更新", hint: "版本检查与下载来源", keywords: ["更新", "版本", "升级", "下载", "镜像", "GitHub"] },
  { id: "data", label: "数据与安全", hint: "数据位置与凭据保护", keywords: ["数据", "目录", "备份", "凭据", "加密", "本机", "安全"] },
  { id: "about", label: "关于", hint: "版本信息与平台能力", keywords: ["关于", "版本", "平台", "能力", "退出", "协议"] },
];

/**
 * 每个可保存设置字段的唯一归属。
 *
 * 用 `Record<keyof Settings, ...>` 锁定：`Settings` 新增字段而这里漏填时，
 * TypeScript 编译会直接失败，避免出现「设置项不知道放哪」的漏网之鱼。
 */
export const SETTINGS_FIELD_SECTIONS: Record<keyof Settings, SettingsSectionId> = {
  mode: "phone",
  theme: "appearance",
  autostart: "general",
  minimize_to_tray: "general",
  notify_on_checkin: "checkin",
  notify_on_failure: "checkin",
  ball: "appearance",
  sync: "phone",
  scheduler_enabled: "checkin",
  catch_up_missed: "checkin",
  auto_refresh_minutes: "checkin",
  update: "update",
  platform_schedules: "checkin",
};
