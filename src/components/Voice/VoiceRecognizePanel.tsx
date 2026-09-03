// 语音中心 · ASR 识别面板：列出已安装语音识别引擎（同构引擎卡 + 各自配置），
// 并内嵌现成的 VoiceInputSettings（识别语言 / 录音设备 / 端到端测试）——不重写其逻辑。
// 从设置页「语音输入」区析出。未来「字幕」插件复用同一套引擎选择，经 EngineCard.purpose 扩展。

import { useState, useEffect } from "react";
import { listPlugins } from "../../services/invoke";
import type { PluginInfo } from "../../types";
import { Field } from "../common/SettingsSection";
import { EngineCard } from "./EngineCard";
import { PluginConfigPanel } from "../Settings/PluginConfigPanel";
import { VoiceInputSettings } from "../Settings/VoiceInputSettings";

export function VoiceRecognizePanel() {
  // listPlugins 同时携带 asr_engine 的 loaded 状态与 config 声明（listAsrPlugins 无 config）
  const [plugins, setPlugins] = useState<PluginInfo[]>([]);
  useEffect(() => {
    listPlugins().then(setPlugins).catch(() => {});
  }, []);

  const asrEngines = plugins.filter((p) => p.plugin_type === "asr_engine");
  const hasLoaded = asrEngines.some((p) => p.loaded);

  return (
    <div className="space-y-3">
      {asrEngines.length === 0 ? (
        <div className="rounded-xl border border-[var(--ink-200)] bg-[var(--paper-card)] px-3.5 py-3 text-[11px] leading-relaxed text-[var(--ink-500)]">
          暂无语音识别引擎。请到「插件」页安装 ASR 插件（如 MiMo ASR），装好后即可在此选择与测试。
        </div>
      ) : (
        asrEngines.map((p) => (
          <EngineCard
            key={p.id}
            kind="asr"
            name={p.name}
            version={p.version}
            category={p.category}
            loaded={p.loaded}
            error={p.error}
            purpose="voice_input"
          >
            {p.config && <PluginConfigPanel pluginId={p.id} pluginName={p.name} title="API 密钥" />}
          </EngineCard>
        ))
      )}

      {hasLoaded && (
        <Field label="语音输入设置">
          <VoiceInputSettings />
        </Field>
      )}
    </div>
  );
}
