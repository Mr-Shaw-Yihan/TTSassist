// 插件「可更新」全局计数（启动后台静默查一次，供侧栏「插件」红点）。
//
// 与 updateStore（应用本体自更新）同构：网络失败一律静默，绝不打扰启动。
// 页内权威列表仍由 PluginPage 的 usePluginInventory 负责（进商店页签惰性拉索引）；
// 这里只是把「有没有可更新」提前反映到侧栏，让用户不进插件页也能被动感知。
import { create } from "zustand";
import { fetchPluginIndex, listBundledPlugins, listPlugins } from "../services/invoke";
import { isNewer } from "../utils/version";

interface Versioned {
  id: string;
  version: string;
}

/**
 * 纯函数：有多少「已安装」插件存在更高可用版本（内置或在线索引中的最高者）。
 * 未安装的插件不计（那不是「更新」）。
 */
export function computeUpdateCount(
  installed: Versioned[],
  bundled: Versioned[],
  index: Versioned[],
): number {
  return installed.filter((p) => {
    const candidates = [
      bundled.find((x) => x.id === p.id)?.version,
      index.find((x) => x.id === p.id)?.version,
    ].filter((v): v is string => !!v);
    const newest = candidates.reduce((acc, v) => (isNewer(v, acc) ? v : acc), p.version);
    return isNewer(newest, p.version);
  }).length;
}

interface PluginUpdateState {
  /** 可更新的已装插件数（>0 时侧栏显示红点） */
  count: number;
  /** 是否已完成一次检查 */
  checked: boolean;
  check: () => Promise<void>;
}

export const usePluginUpdateStore = create<PluginUpdateState>((set) => ({
  count: 0,
  checked: false,
  check: async () => {
    try {
      // 本地枚举 + 在线索引并行；在线失败降级为空索引（仍能反映内置可更新），整体失败静默。
      const [plugins, bundled] = await Promise.all([listPlugins(), listBundledPlugins()]);
      let index: Versioned[] = [];
      try {
        index = await fetchPluginIndex();
      } catch {
        index = [];
      }
      set({ count: computeUpdateCount(plugins, bundled, index), checked: true });
    } catch {
      set({ count: 0, checked: true });
    }
  },
}));
