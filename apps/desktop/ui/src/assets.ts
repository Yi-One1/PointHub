/**
 * 图标资产清单。
 *
 * 只做「路径 → URL」的集中登记，方便测试统一校验：
 * - 品牌图标来自用户提供的完整彩色图标，自带浅色圆角底；
 * - 积分图标四种色调，按位置语义选用（见 styles.css 的 .pts-*）；
 * - 平台 logo 只登记已实测的三个平台，未知平台由调用方回退成识别色圆点。
 */
import brandIcon from "./assets/brand/pointhub-icon.svg";
import brandMark from "./assets/brand/pointhub-mark.svg";
import pointsBronze from "./assets/points/points-bronze.png";
import pointsInk from "./assets/points/points-ink.png";
import pointsJade from "./assets/points/points-jade.png";
import vendorQoder from "./assets/vendors/qoder.png";
import vendorTrae from "./assets/vendors/trae.png";
import vendorWorkBuddy from "./assets/vendors/workbuddy.png";

/** 完整彩色应用图标：侧栏品牌位、启动页。 */
export const BRAND_ICON = brandIcon;
/** 透明底的品牌标记：需要单色/无底色时使用。 */
export const BRAND_MARK = brandMark;

export type PointsTone = "bronze" | "ink" | "jade";

/** 积分单位图标。 */
export const POINTS_ICON: Record<PointsTone, string> = {
  bronze: pointsBronze,
  ink: pointsInk,
  jade: pointsJade,
};

/** 平台 logo。查不到时返回 undefined，由调用方回退成 VENDOR_TONE 圆点。 */
export const VENDOR_ICON: Record<string, string> = {
  "workbuddy": vendorWorkBuddy,
  "trae-cn": vendorTrae,
  "qoder-cn": vendorQoder,
};
