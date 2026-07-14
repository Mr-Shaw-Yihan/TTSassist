// 设置面板（最小入口）：填 MiMo API Key
// 首版只放 API Key 的输入和持久化。完整设置界面留到 P1 阶段 9.x。

import { useState } from "react";
import { useSettingsStore } from "../../stores/settingsStore";

interface Props {
  onClose: () => void;
}

export function ApiKeyModal({ onClose }: Props) {
  const settings = useSettingsStore((s) => s.settings);
  const patch = useSettingsStore((s) => s.patch);
  const [key, setKey] = useState(settings?.mimo_api_key ?? "");

  async function save() {
    await patch("mimo_api_key", key.trim());
    onClose();
  }

  return (
    <div className="fixed inset-0 z-[60] flex items-center justify-center bg-black/40">
      <div className="w-80 rounded-xl bg-white p-5 shadow-xl">
        <h2 className="mb-3 text-base font-semibold text-gray-800">MiMo API Key</h2>
        <p className="mb-3 text-xs leading-relaxed text-gray-500">
          前往{" "}
          <a
            href="https://platform.xiaomimimo.com"
            target="_blank"
            className="text-blue-500 hover:underline"
            rel="noreferrer"
          >
            platform.xiaomimimo.com
          </a>{" "}
          注册并领取 API Key，粘贴到此处。
        </p>
        <input
          type="text"
          value={key}
          onChange={(e) => setKey(e.target.value)}
          placeholder="sk-..."
          className="mb-3 w-full rounded-lg border border-gray-200 px-3 py-2 text-sm outline-none focus:border-blue-400"
          autoFocus
        />
        <div className="flex justify-end gap-2">
          <button
            onClick={onClose}
            className="rounded-lg px-3 py-1.5 text-sm text-gray-500 hover:bg-gray-100"
          >
            取消
          </button>
          <button
            onClick={save}
            className="rounded-lg bg-blue-500 px-4 py-1.5 text-sm font-medium text-white hover:bg-blue-600"
          >
            保存
          </button>
        </div>
      </div>
    </div>
  );
}