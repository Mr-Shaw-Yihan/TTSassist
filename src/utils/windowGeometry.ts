// 窗口尺寸记忆的纯逻辑（B3）。
// 分工：尺寸存在 settings.json 的 window_width / window_height（物理像素，0 = 无记忆），
// 启动时由 Rust 在窗口显示**之前**应用并夹到 [最小尺寸, 屏幕尺寸]；前端只负责保存。
// 下限过滤留在前端，是因为主窗以 visible=false 创建，未布局时 innerSize 会读到接近 1×1 的
// 占位小值——一旦记进 settings，下次冷启动就是半个看不见的窗（历史踩过，见 useWindowState 头注）。

/** 认可边长下限（物理像素）。配置最小尺寸为逻辑 360×540，正常缩放下物理边远超此值 */
export const MIN_SIDE_PHYSICAL = 200;

/** 尺寸是否可信（两边都不低于下限） */
export function plausibleSize(width: number, height: number): boolean {
  return (
    Number.isFinite(width) &&
    Number.isFinite(height) &&
    width >= MIN_SIDE_PHYSICAL &&
    height >= MIN_SIDE_PHYSICAL
  );
}

/** 产出要写进 settings 的键值对；不可信时返回空数组（一个都不写） */
export function windowSizeSettings(width: number, height: number): Array<["window_width", number] | ["window_height", number]> {
  if (!plausibleSize(width, height)) return [];
  return [
    ["window_width", width],
    ["window_height", height],
  ];
}
