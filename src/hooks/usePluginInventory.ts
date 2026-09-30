// 插件清单数据层：已装 / 内置 / 在线索引三源状态与派生分组。
// 供插件库双页签（已安装 / 插件商店）消费；在线索引不自动拉取（隐私设计），
// 由商店页签首次进入时触发（惰性联网）。

import { useCallback, useEffect, useMemo, useState } from "react";
import {
  listPlugins,
  fetchPluginIndex,
  listBundledPlugins,
} from "../services/invoke";
import { useSettingsStore } from "../stores/settingsStore";
import { isNewer } from "../utils/version";
import type { PluginInfo, PluginIndexEntry, BundledPluginInfo } from "../types";

/** 插件大类：语音合成 / 语音识别 / 扩展（效果器、服务、未来字幕等） */
export type PluginCat = "tts" | "asr" | "ext";

/** manifest.plugin_type → 页签分类（未知类型一律归扩展） */
export function catOf(pluginType: string | undefined | null): PluginCat {
  if (pluginType === "asr_engine") return "asr";
  if (pluginType === "tts_engine") return "tts";
  return "ext";
}

/** 商店条目：内置与在线索引合并去重后的统一形态（含已装态） */
export interface StoreItem {
  id: string;
  name: string;
  version: string;
  description: string;
  requirements?: string | null;
  /** manifest 类型（bundled 缺省按 tts_engine） */
  plugin_type?: string;
  /** 安装来源：bundled=随安装包内置（离线即装） / online=官方在线下载 */
  source: "bundled" | "online";
  /** 本机是否已安装 */
  installed: boolean;
  /** 已装版本（installed 时有值） */
  installedVersion?: string;
  /** 有比已装更高的可用版本 */
  hasUpdate: boolean;
}

/** 同 id 合并商店条目：优先内置（离线即装），同为内置/在线时保留更高版本 */
function mergeStoreItems(
  bundled: BundledPluginInfo[],
  online: PluginIndexEntry[],
  plugins: PluginInfo[]
): StoreItem[] {
  const map = new Map<string, StoreItem>();
  const put = (next: StoreItem) => {
    const prev = map.get(next.id);
    if (!prev) {
      map.set(next.id, next);
      return;
    }
    // 两者都在：内置优先；同为在线保留更高版本（继承已装态）
    if (prev.source === "online" && next.source === "bundled") {
      map.set(next.id, { ...next, installed: prev.installed, installedVersion: prev.installedVersion, hasUpdate: prev.hasUpdate });
    } else if (prev.source === "online" && next.source === "online" && isNewer(next.version, prev.version)) {
      map.set(next.id, next);
    }
  };
  for (const b of bundled) {
    const installed = plugins.find((p) => p.id === b.id);
    put({
      id: b.id,
      name: b.name,
      version: b.version,
      description: b.description,
      requirements: b.requirements,
      plugin_type: b.plugin_type,
      source: "bundled",
      installed: !!installed,
      installedVersion: installed?.version,
      hasUpdate: installed ? isNewer(b.version, installed.version) : false,
    });
  }
  for (const o of online) {
    const installed = plugins.find((p) => p.id === o.id);
    put({
      id: o.id,
      name: o.name,
      version: o.version,
      description: o.description,
      requirements: o.requirements,
      plugin_type: o.plugin_type,
      source: "online",
      installed: !!installed,
      installedVersion: installed?.version,
      hasUpdate: installed ? isNewer(o.version, installed.version) : false,
    });
  }
  return [...map.values()];
}

/** 已装货架内排序档：使用中(0) → 待配置(1) → 就绪(2) → 故障(3) */
function rankOf(
  p: PluginInfo,
  currentEngineId: string | undefined,
  currentAsrId: string | undefined,
  typeOf: (p: PluginInfo) => string,
  pendingConfig: (p: PluginInfo) => boolean,
): number {
  const cat = catOf(typeOf(p));
  if (cat === "tts" && p.id === currentEngineId) return 0;
  if (cat === "asr" && p.id && p.id === currentAsrId) return 0;
  if (!p.loaded) return 3;
  if (pendingConfig(p)) return 1;
  return 2;
}

export function usePluginInventory() {
  const [plugins, setPlugins] = useState<PluginInfo[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);

  // 内置插件库（随安装包携带）
  const [bundled, setBundled] = useState<BundledPluginInfo[]>([]);

  // 在线插件索引（惰性：由商店页签首次进入时触发）
  const [index, setIndex] = useState<PluginIndexEntry[] | null>(null);
  const [indexError, setIndexError] = useState<string | null>(null);
  const [indexLoading, setIndexLoading] = useState(false);

  const settings = useSettingsStore((s) => s.settings);

  const reload = useCallback(async () => {
    setLoading(true);
    try {
      setPlugins(await listPlugins());
      setError(null);
    } catch (e) {
      setError(String(e));
    } finally {
      setLoading(false);
    }
  }, []);

  const reloadIndex = useCallback(async () => {
    setIndexLoading(true);
    try {
      setIndex(await fetchPluginIndex());
      setIndexError(null);
    } catch (e) {
      setIndex(null);
      setIndexError(String(e));
    } finally {
      setIndexLoading(false);
    }
  }, []);

  const reloadBundled = useCallback(async () => {
    try {
      setBundled(await listBundledPlugins());
    } catch {
      setBundled([]);
    }
  }, []);

  useEffect(() => {
    void reload();
    void reloadBundled();
  }, [reload, reloadBundled]);

  // ── 派生：已装分组（状态排序） + 商店全量合并分组 + 计数 ─────
  const derived = useMemo(() => {
    // 老插件无 plugin_type 字段 → 默认归语音合成（历史插件均为 TTS）
    const typeOf = (p: PluginInfo) => p.plugin_type ?? "tts_engine";
    const currentEngineId = settings?.tts_engine;
    const currentAsrId = settings?.asr_plugin;

    const installedGroups: Record<PluginCat, PluginInfo[]> = { tts: [], asr: [], ext: [] };
    // 必填配置缺失 = 待配置（合成时插件会报缺配置，货架排序时前置）
    const pendingConfig = (p: PluginInfo) =>
      (p.config?.fields ?? []).some(
        (f) => f.required && !(settings?.plugin_config?.[p.id]?.[f.key] ?? "").trim(),
      );
    for (const p of plugins) installedGroups[catOf(typeOf(p))].push(p);
    for (const key of Object.keys(installedGroups) as PluginCat[]) {
      installedGroups[key].sort(
        (a, b) =>
          rankOf(a, currentEngineId, currentAsrId, typeOf, pendingConfig) -
            rankOf(b, currentEngineId, currentAsrId, typeOf, pendingConfig) ||
          a.name.localeCompare(b.name, "zh"),
      );
    }

    const storeItems = mergeStoreItems(bundled, index ?? [], plugins);
    const storeGroups: Record<PluginCat, StoreItem[]> = { tts: [], asr: [], ext: [] };
    for (const it of storeItems) storeGroups[catOf(it.plugin_type)].push(it);

    // 可更新计数（页头 chip）：比较已装版本与「内置/在线中的最高可用版本」
    const updateCount = plugins.filter((p) => {
      const candidates = [bundled.find((x) => x.id === p.id)?.version, (index ?? []).find((x) => x.id === p.id)?.version]
        .filter((v): v is string => !!v);
      const newest = candidates.reduce((acc, v) => (isNewer(v, acc) ? v : acc), p.version);
      return isNewer(newest, p.version);
    }).length;

    return { typeOf, installedGroups, storeGroups, updateCount };
  }, [plugins, bundled, index, settings]);

  return {
    plugins,
    loading,
    error,
    bundled,
    index,
    indexError,
    indexLoading,
    reload,
    reloadIndex,
    reloadBundled,
    ...derived,
  };
}

export type PluginInventory = ReturnType<typeof usePluginInventory>;
