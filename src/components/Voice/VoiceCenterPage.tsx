// 语音中心：顶层「语音」页的壳。顶部两个子标签【语音合成 / 语音识别】，分别渲染 TTS / ASR 面板。
// 把原先散落在设置页的引擎与音色配置集中于此；设置页仅保留系统偏好。

import { useState } from "react";
import { VoiceSynthPanel } from "./VoiceSynthPanel";
import { VoiceRecognizePanel } from "./VoiceRecognizePanel";

type VoiceSubTab = "synth" | "recognize";

const SUB_TABS: { id: VoiceSubTab; label: string }[] = [
  { id: "synth", label: "语音合成" },
  { id: "recognize", label: "语音识别" },
];

export function VoiceCenterPage() {
  const [subTab, setSubTab] = useState<VoiceSubTab>("synth");

  return (
    <div className="flex h-full flex-col">
      {/* 子标签栏 */}
      <div className="shrink-0 border-b border-[var(--ink-200)] px-4">
        <div className="mx-auto flex max-w-xl gap-1 pt-2.5">
          {SUB_TABS.map((t) => {
            const active = subTab === t.id;
            return (
              <button
                key={t.id}
                onClick={() => setSubTab(t.id)}
                aria-pressed={active}
                className={[
                  "rounded-t-lg border-b-2 px-3.5 pb-2 pt-1.5 text-sm transition-colors",
                  active
                    ? "border-[var(--amber-500)] font-medium text-[var(--ink-900)]"
                    : "border-transparent text-[var(--ink-300)] hover:text-[var(--ink-700)]",
                ].join(" ")}
              >
                {t.label}
              </button>
            );
          })}
        </div>
      </div>

      {/* 内容区 */}
      <div className="scrollbar-thin flex-1 overflow-y-auto px-4 py-5 text-sm">
        <div className="mx-auto max-w-xl">
          {subTab === "synth" ? <VoiceSynthPanel /> : <VoiceRecognizePanel />}
        </div>
      </div>
    </div>
  );
}
