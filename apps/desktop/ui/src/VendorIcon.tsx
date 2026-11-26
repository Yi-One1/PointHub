import { VENDOR_ICON } from "./assets";

/**
 * 平台 logo：统一放在平台名称前面。
 * 没收录的平台回退成识别色圆点，不至于出现空白占位。
 */
export function VendorIcon({ vendor, size = 16 }: { vendor: string; size?: number }) {
  const src = VENDOR_ICON[vendor];
  if (!src) return <span className="dot" />;
  return (
    <img
      className="vicon"
      src={src}
      alt=""
      aria-hidden
      width={size}
      height={size}
      style={{ width: size, height: size }}
    />
  );
}
