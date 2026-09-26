// 插件管理动作层：安装/卸载/切换引擎/打开位置/拖入安装，统一接 confirm 弹窗与 toast 提示。
// 从 PluginPage 抽出；busy 为页面级单飞状态（操作期间禁用全部按钮 + 底部提示条）。

import { useCallback, useState } from "react";
import { revealItemInDir } from "@tauri-apps/plugin-opener";
import {
  uninstallPlugin,
  installPluginZip,
  installBundledPlugin,
  downloadInstallPlugin,
} from "../services/invoke";
import { useSettingsStore } from "../stores/settingsStore";
import { confirm } from "../components/common/ConfirmDialog";
import { toast } from "../components/common/Toast";
import type { Candidate } from "./usePluginInventory";
import type { PluginInfo } from "../types";

export function usePluginActions(reload: () => Promise<void>, reloadBundled: () => Promise<void>) {
  const [busy, setBusy] = useState<string | null>(null);
  const patch = useSettingsStore((s) => s.patch);

  /** 按来源安装候选条目（内置/在线两条安装通道统一入口） */
  const installCandidate = useCallback(
    async (c: Candidate) => {
      setBusy(`正在安装「${c.name}」…`);
      try {
        const msg =
          c.source === "bundled"
            ? await installBundledPlugin(c.id)
            : await downloadInstallPlugin(c.id);
        await reload();
        await reloadBundled();
        toast(msg);
      } catch (e) {
        toast(`安装失败：${e}`, "err");
      } finally {
        setBusy(null);
      }
    },
    [reload, reloadBundled]
  );

  const uninstall = useCallback(
    async (p: PluginInfo) => {
      const ok = await confirm({
        title: `卸载「${p.name}」`,
        danger: true,
        confirmText: "卸载",
        message:
          (p.loaded
            ? "该插件正在使用中，卸载后本次会话内仍可用，重启应用后彻底移除。"
            : "卸载后立即生效。") + "\n\n确定要卸载吗？",
      });
      if (!ok) return;
      setBusy("正在卸载…");
      try {
        const msg = await uninstallPlugin(p.id);
        await reload();
        toast(msg);
      } catch (e) {
        toast(`卸载失败：${e}`, "err");
      } finally {
        setBusy(null);
      }
    },
    [reload]
  );

  const setEngine = useCallback(
    async (p: PluginInfo) => {
      try {
        await patch("tts_engine", p.id);
      } catch (e) {
        toast(`切换引擎失败：${e}`, "err");
      }
    },
    [patch]
  );

  const openLocation = useCallback(async (p: PluginInfo) => {
    try {
      await revealItemInDir(p.path);
    } catch (e) {
      toast(`打开插件目录失败：${e}`, "err");
    }
  }, []);

  /** 拖入安装：本地 zip 非官方渠道，需用户确认来源可信 */
  const dropInstall = useCallback(
    async (zipPath: string) => {
      const ok = await confirm({
        title: "安装本地插件包",
        message:
          "检测到拖入的插件安装包。\n\n" +
          "提示：该插件来自本地文件，非官方索引渠道。系统会校验插件完整性（SHA-256），" +
          "但无法验证来源可信度，请确认你信任该插件的来源。\n\n" +
          `是否安装？\n${zipPath}`,
        confirmText: "安装",
      });
      if (!ok) return;
      setBusy("正在安装本地插件…");
      try {
        const msg = await installPluginZip(zipPath);
        await reload();
        toast(msg);
      } catch (e) {
        toast(`安装失败：${e}`, "err");
      } finally {
        setBusy(null);
      }
    },
    [reload]
  );

  return { busy, installCandidate, uninstall, setEngine, openLocation, dropInstall };
}

export type PluginActions = ReturnType<typeof usePluginActions>;
