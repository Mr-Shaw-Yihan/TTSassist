// 语音中心「效果器」管理分区（设计 §六 UI-3）。
//
// 复用纪律：官方下载 / 本地导入 / 删除走「插件」页同一套命令与 checksum 校验
// （downloadInstallPlugin / installPluginZip / uninstallPlugin），不另写一份。
// 排序用上下箭头按钮——不做拖拽（人类已拍定：v1 不引 dnd 库、不手写 pointer 事件）。
// 排序结果落 fx_order；预设选择落 fx_preset（与主窗抽屉同一数据源）。

import { useCallback, useEffect, useState } from "react";
import { open } from "@tauri-apps/plugin-dialog";
import {
  downloadInstallPlugin,
  fetchPluginIndex,
  installPluginZip,
  listFxPresets,
  listPlugins,
  uninstallPlugin,
} from "../../services/invoke";
import { useSettingsStore } from "../../stores/settingsStore";
import type { FxPresetInfo, PluginIndexEntry, PluginInfo } from "../../types";
import { FX_PRESET_OFF, sortFxPresets } from "../../utils/fx";
import { SubPanel } from "../common/SettingsSection";

export function FxManagePanel() {
  const settings = useSettingsStore((s) => s.settings);
  const patch = useSettingsStore((s) => s.patch);
  const [presets, setPresets] = useState<FxPresetInfo[]>([]);
  const [online, setOnline] = useState<PluginIndexEntry[]>([]);
  const [fxPlugins, setFxPlugins] = useState<PluginInfo[]>([]);
  const [busy, setBusy] = useState<string | null>(null);

  const reload = useCallback(() => {
    listFxPresets().then(setPresets).catch(() => {});
    listPlugins().then((all) => setFxPlugins(all.filter((p) => p.plugin_type === "audio_effect"))).catch(() => {});
  }, []);

  useEffect(() => {
    reload();
    fetchPluginIndex()
      .then((idx) => setOnline(idx.filter((e) => e.plugin_type === "audio_effect")))
      .catch(() => {});
  }, [reload]);

  const order = settings?.fx_order ?? [];
  const current = settings?.fx_preset ?? FX_PRESET_OFF;
  const sorted = sortFxPresets(presets, order);

  /** 上下移预设（写回完整 fx_order） */
  const move = (key: string, dir: -1 | 1) => {
    const cur = sorted.map((p) => p.key);
    const i = cur.indexOf(key);
    const j = i + dir;
    if (i < 0 || j < 0 || j >= cur.length) return;
    const next = [...cur];
    [next[i], next[j]] = [next[j], next[i]];
    void patch("fx_order", next);
  };

  /** 未安装的在线效果器插件条目（与已装对照去重） */
  const onlineCandidates = online.filter(
    (o) => !fxPlugins.some((p) => p.id === o.id) && (!o.plugin_type || o.plugin_type === "audio_effect"),
  );

  async function handleDownload(id: string) {
    setBusy(`正在安装「${id}」…`);
    try {
      window.alert(await downloadInstallPlugin(id));
    } catch (e) {
      window.alert(`安装失败：${e}`);
    } finally {
      setBusy(null);
      reload();
    }
  }

  async function handleImportZip() {
    const zipPath = await open({
      multiple: false,
      filters: [{ name: "效果器插件", extensions: ["zip"] }],
    });
    if (!zipPath) return;
    setBusy("正在导入…");
    try {
      window.alert(await installPluginZip(zipPath as string));
    } catch (e) {
      window.alert(`导入失败：${e}`);
    } finally {
      setBusy(null);
      reload();
    }
  }

  async function handleUninstall(id: string) {
    if (!window.confirm(`卸载效果器插件「${id}」？其预设将从列表移除，当前选择回退为「原声」。`)) return;
    setBusy(`正在卸载「${id}」…`);
    try {
      window.alert(await uninstallPlugin(id));
    } catch (e) {
      window.alert(`卸载失败：${e}`);
    } finally {
      setBusy(null);
      reload();
    }
  }

  return (
    <SubPanel
      title="效果器"
      desc="给合成语音套预设声线：选择即时生效，历史消息与收藏重播同样套用。"
      right={
        <button
          onClick={() => void handleImportZip()}
          disabled={busy !== null}
          className="rounded-lg border border-[var(--ink-200)] px-2.5 py-1 text-[11px] text-[var(--ink-500)] transition-colors hover:border-[var(--amber-500)] hover:text-[var(--amber-600)] disabled:opacity-40"
        >
          导入本地 zip
        </button>
      }
    >
      {/* 预设列表：原声固定首项；插件预设可排序、点选即生效 */}
      <ul className="space-y-1">
        <li>
          <button
            onClick={() => void patch("fx_preset", FX_PRESET_OFF)}
            className={[
              "flex w-full items-center gap-2 rounded-lg px-2.5 py-1.5 text-left text-sm transition-colors",
              current === FX_PRESET_OFF
                ? "bg-[var(--amber-600)]/10 font-medium text-[var(--amber-600)]"
                : "text-[var(--ink-700)] hover:bg-[var(--ink-100)]",
            ].join(" ")}
          >
            <span className="flex-1">原声</span>
            <span className="text-[10px] text-[var(--ink-300)]">默认 · 不可移除</span>
          </button>
        </li>
        {sorted.map((p, i) => {
          const selected = p.key === current;
          return (
            <li key={p.key} className="flex items-center gap-1">
              <button
                onClick={() => void patch("fx_preset", p.key)}
                title={p.description}
                className={[
                  "flex min-w-0 flex-1 items-center gap-2 rounded-lg px-2.5 py-1.5 text-left text-sm transition-colors",
                  selected
                    ? "bg-[var(--amber-600)]/10 font-medium text-[var(--amber-600)]"
                    : "text-[var(--ink-700)] hover:bg-[var(--ink-100)]",
                ].join(" ")}
              >
                <span className="shrink-0">{p.name}</span>
                <span className="truncate text-[10px] text-[var(--ink-300)]">{p.plugin_name}</span>
              </button>
              <span className="flex shrink-0 flex-col">
                <button
                  aria-label={`上移${p.name}`}
                  disabled={i === 0}
                  onClick={() => move(p.key, -1)}
                  className="h-4 px-1.5 text-[10px] leading-4 text-[var(--ink-300)] transition-colors hover:text-[var(--ink-700)] disabled:opacity-30"
                >
                  ▲
                </button>
                <button
                  aria-label={`下移${p.name}`}
                  disabled={i === sorted.length - 1}
                  onClick={() => move(p.key, 1)}
                  className="h-4 px-1.5 text-[10px] leading-4 text-[var(--ink-300)] transition-colors hover:text-[var(--ink-700)] disabled:opacity-30"
                >
                  ▼
                </button>
              </span>
            </li>
          );
        })}
        {sorted.length === 0 && (
          <li className="px-2.5 py-2 text-[11px] leading-relaxed text-[var(--ink-300)]">
            尚未安装效果器插件。内置「内置效果器包」会在安装后自动出现，也可从官方索引下载或导入本地 zip。
          </li>
        )}
      </ul>

      {/* 效果器插件管理行：已装（可卸载）+ 在线可装 */}
      {(fxPlugins.length > 0 || onlineCandidates.length > 0) && (
        <div className="mt-2 space-y-1 border-t border-[var(--ink-200)]/60 pt-2">
          {fxPlugins.map((p) => (
            <div key={p.id} className="flex items-center gap-2 rounded-lg px-2.5 py-1">
              <span className="min-w-0 flex-1 truncate text-[11px] text-[var(--ink-500)]">
                {p.name} v{p.version}
                {!p.loaded && <span className="ml-1 text-[var(--seal)]">（加载失败：{p.error ?? "未知原因"}）</span>}
              </span>
              <button
                onClick={() => void handleUninstall(p.id)}
                disabled={busy !== null}
                className="rounded-lg px-2 py-0.5 text-[11px] text-[var(--ink-300)] transition-colors hover:bg-[var(--seal)]/10 hover:text-[var(--seal)] disabled:opacity-40"
              >
                卸载
              </button>
            </div>
          ))}
          {onlineCandidates.map((o) => (
            <div key={o.id} className="flex items-center gap-2 rounded-lg px-2.5 py-1">
              <span className="min-w-0 flex-1 truncate text-[11px] text-[var(--ink-500)]">
                {o.name} v{o.version}
              </span>
              <button
                onClick={() => void handleDownload(o.id)}
                disabled={busy !== null}
                className="rounded-lg border border-[var(--amber-500)] px-2 py-0.5 text-[11px] font-medium text-[var(--amber-600)] transition-colors hover:bg-[var(--amber-200)]/30 disabled:opacity-40"
              >
                下载
              </button>
            </div>
          ))}
        </div>
      )}

      {busy && <p className="mt-2 text-[11px] text-[var(--amber-600)] animate-fade">{busy}</p>}
    </SubPanel>
  );
}
