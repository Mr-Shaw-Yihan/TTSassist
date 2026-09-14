// 语音效果器共享工具：预设排序（fx_order）与当前选择展示名。

import type { FxPresetInfo } from "../types";

/** 「原声」固定保留值（非插件预设，UI 固定渲染首项且不可删除） */
export const FX_PRESET_OFF = "off";

/**
 * 按 fx_order 排预设：order 中的 key 按其索引序在前，未列出的新预设排末尾、
 * 按字典序稳定（与设计 §五 fx_order 语义一致）。
 */
export function sortFxPresets(presets: FxPresetInfo[], order: string[]): FxPresetInfo[] {
  const idx = new Map(order.map((k, i) => [k, i]));
  return [...presets].sort((a, b) => {
    const ia = idx.get(a.key);
    const ib = idx.get(b.key);
    if (ia !== undefined && ib !== undefined) return ia - ib;
    if (ia !== undefined) return -1;
    if (ib !== undefined) return 1;
    return a.key.localeCompare(b.key);
  });
}

/** 当前预设的展示名（off / 空 / 非法值都显示「原声」——非法 fail-open 直通） */
export function currentFxLabel(preset: string | undefined, presets: FxPresetInfo[]): string {
  if (!preset || preset === FX_PRESET_OFF) return "原声";
  return presets.find((p) => p.key === preset)?.name ?? "原声";
}
