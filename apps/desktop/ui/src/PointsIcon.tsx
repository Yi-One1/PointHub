import { POINTS_ICON, type PointsTone } from "./assets";

/**
 * 积分单位图标：跟在数字后面，取代原来写死的「分」。
 * 色调按位置语义选：合计=bronze，次级小计=ink，余额/新增=jade。
 */
export function PointsIcon({ tone, size = 13 }: { tone: PointsTone; size?: number }) {
  return (
    <img
      className={`pts pts-${tone}`}
      src={POINTS_ICON[tone]}
      alt=""
      aria-hidden
      width={size}
      height={size}
      style={{ width: size, height: size }}
    />
  );
}
