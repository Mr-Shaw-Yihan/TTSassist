// 插件清单数据层：已装 / 内置 / 在线索引三源状态与派生分类。
// 从 PluginPage 抽出，页面组件只做编排与渲染；在线索引不自动拉取（隐私设计，手动触发）。

import { useCallback, useEffect, useMemo, useState } from "react";
import {
  listPlugins,
  fetchPluginIndex,
  listBundledPlugins,
} from "../services/invoke";
import { isNewer } from "../utils/version";
import type { PluginInfo, PluginIndexEntry, BundledPluginInfo } from "../types";

/** 可安装条目：内置插件库与在线索引合并去重后的统一形态 */
export interface Candidate {
  id: string;
  name: string;
  version: string;
  description: string;
  requirements?: string | null;
  /** 安装来源：bundled=随安装包内置（离线即装） / online=官方在线下载 */
  source: "bundled" | "online";
}

/** 同 id 合并候选：优先内置（离线即装），同为内置/在线时保留更高版本 */
function mergeCandidates(
  bundled: BundledPluginInfo[],
  online: PluginIndexEntry[],
  type: string,
  typeOfOnline: (id: string) => string
): Candidate[] {
  const map = new Map<string, Candidate>();
  for (const b of bundled) {
    if ((b.plugin_type ?? "tts_engine") !== type) continue;
    map.set(b.id, {
      id: b.id,
      name: b.name,
      version: b.version,
      description: b.description,
      requirements: b.requirements,
      source: "bundled",
    });
  }
  for (const o of online) {
    if (typeOfOnline(o.id) !== type) continue;
    const prev = map.get(o.id);
    if (prev) {
      if (prev.source === "online" && isNewer(o.version, prev.version)) {
        map.set(o.id, {
          id: o.id,
          name: o.name,
          version: o.version,
          description: o.description,
          requirements: o.requirements,
          source: "online",
        });
      }
      // 已有内置候选：保留内置（离线即装），跳过在线
    } else {
      map.set(o.id, {
        id: o.id,
        name: o.name,
        version: o.version,
        description: o.description,
        requirements: o.requirements,
        source: "online",
      });
    }
  }
  return [...map.values()];
}

export function usePluginInventory() {
  const [plugins, setPlugins] = useState<PluginInfo[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);

  // 内置插件库（随安装包携带）
  const [bundled, setBundled] = useState<BundledPluginInfo[]>([]);

  // 在线插件索引（用户手动触发拉取，不自动联网）
  const [index, setIndex] = useState<PluginIndexEntry[] | null>(null);
  const [indexError, setIndexError] = useState<string | null>(null);
  const [indexLoading, setIndexLoading] = useState(false);

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
    // 在线索引不自动拉取：由页头「获取在线列表」按钮手动触发
  }, [reload, reloadBundled]);

  // ── 派生：已装分桶 + 可装候选 + 计数 ─────────────────────────
  const derived = useMemo(() => {
    // 老插件无 plugin_type 字段 → 默认归入语音合成（历史插件均为 TTS）
    const typeOf = (p: PluginInfo) => p.plugin_type ?? "tts_engine";
    // 在线条目类型：新索引自带 plugin_type；旧索引无此字段时回退到同 id 的内置条目，再无则按 TTS
    const typeOfOnline = (id: string): string => {
      const entry = index?.find((o) => o.id === id);
      if (entry?.plugin_type) return entry.plugin_type;
      const b = bundled.find((x) => x.id === id);
      return b?.plugin_type ?? "tts_engine";
    };

    const installedTts = plugins.filter((p) => typeOf(p) === "tts_engine");
    const installedAsr = plugins.filter((p) => typeOf(p) === "asr_engine");
    // 服务插件（type=service）：不参与合成/识别的后台能力（如手机遥控）
    const installedService = plugins.filter((p) => typeOf(p) === "service");
    // 效果器插件（type=audio_effect）：插件页照常可见与可卸载（管理/排序在语音中心）
    const installedFx = plugins.filter((p) => typeOf(p) === "audio_effect");

    const notInstalled = (c: Candidate) => !plugins.some((p) => p.id === c.id);
    const candidatesTts = mergeCandidates(bundled, index ?? [], "tts_engine", typeOfOnline).filter(notInstalled);
    const candidatesAsr = mergeCandidates(bundled, index ?? [], "asr_engine", typeOfOnline).filter(notInstalled);
    const candidatesFx = mergeCandidates(bundled, index ?? [], "audio_effect", typeOfOnline).filter(notInstalled);

    // 在线条目相对已装插件（用于判断可更新版本）
    const onlineEntryOf = (id: string): PluginIndexEntry | undefined =>
      index?.find((o) => o.id === id);

    // 索引获取结果提示用：可更新插件数 + 可新装条目数（避免「获取成功但无变化」的困惑）
    const updateCount = plugins.filter((p) => {
      const o = onlineEntryOf(p.id);
      return o && isNewer(o.version, p.version);
    }).length;
    const freshCount = candidatesTts.length + candidatesAsr.length + candidatesFx.length;

    return {
      typeOf,
      installedTts,
      installedAsr,
      installedService,
      installedFx,
      candidatesTts,
      candidatesAsr,
      candidatesFx,
      onlineEntryOf,
      updateCount,
      freshCount,
    };
  }, [plugins, bundled, index]);

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
