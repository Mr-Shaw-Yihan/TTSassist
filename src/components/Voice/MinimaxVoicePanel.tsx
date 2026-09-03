// MiniMax 国际版音色面板：账号内「音色克隆」+「音色管理（查询/使用/删除）」。
// 自含状态与逻辑（直连 settings store 与 minimax_global_* invoke），仅在 minimax-tts-global 引擎下渲染。
// 从设置页「语音合成」区抽入语音中心，行为与原实现一致。

import { useState, type ReactNode } from "react";
import { useSettingsStore } from "../../stores/settingsStore";
import { SectionHeading } from "../common/SettingsSection";
import {
  minimaxGlobalVoiceClone,
  minimaxGlobalGetVoices,
  minimaxGlobalDeleteVoice,
  pickAudioFile,
} from "../../services/invoke";
import type { PluginInfo } from "../../types";

// MiniMax 国际版克隆/音色管理 API 端点（T2A 由插件走 api-uw 加速端点）
const MM_GLOBAL_BASE = "https://api.minimax.io";

/** get_voice 返回的音色条目（克隆/设计组） */
interface MmAccountVoice {
  voice_id: string;
  voice_name?: string;
  created_time?: string;
}

export function MinimaxVoicePanel({ plugin, voiceSelect }: { plugin: PluginInfo; voiceSelect?: ReactNode }) {
  const settings = useSettingsStore((s) => s.settings);
  const patch = useSettingsStore((s) => s.patch);

  // 克隆：文件/voice_id/高级选项/进行中/结果提示
  const [mmCloneFile, setMmCloneFile] = useState("");
  const [mmCloneVoiceId, setMmCloneVoiceId] = useState("");
  const [mmShowAdvanced, setMmShowAdvanced] = useState(false);
  const [mmPromptFile, setMmPromptFile] = useState("");
  const [mmPromptText, setMmPromptText] = useState("");
  const [mmCloning, setMmCloning] = useState(false);
  const [mmCloneMsg, setMmCloneMsg] = useState<{ ok: boolean; text: string } | null>(null);

  // 账号音色管理：查询/删除
  const [mmAccountVoices, setMmAccountVoices] = useState<{
    cloning: MmAccountVoice[];
    generation: MmAccountVoice[];
    systemCount: number;
  } | null>(null);
  const [mmVoicesLoading, setMmVoicesLoading] = useState(false);
  const [mmManageMsg, setMmManageMsg] = useState<{ ok: boolean; text: string } | null>(null);
  const [mmDeleteTarget, setMmDeleteTarget] = useState<{ type: string; id: string } | null>(null);

  /** MiniMax 国际版 API Key：通用插件配置机制迁移后从 plugin_config 读取 */
  const mmGlobalKey = settings?.plugin_config?.[plugin.id]?.["api_key"] ?? "";

  /** 音色克隆：上传音频（+可选样本）→ voice_clone → 持久化并切换 */
  async function handleMinimaxClone() {
    const vid = mmCloneVoiceId.trim();
    if (!mmCloneFile || !vid || mmCloning) return;
    if (!mmGlobalKey) {
      setMmCloneMsg({ ok: false, text: "请先在上方配置卡中填写 MiniMax 国际版 API Key" });
      return;
    }
    setMmCloning(true);
    setMmCloneMsg({ ok: true, text: "正在上传音频并克隆音色，请稍候…" });
    try {
      const clonedId = await minimaxGlobalVoiceClone(
        mmCloneFile,
        vid,
        mmGlobalKey,
        MM_GLOBAL_BASE,
        mmShowAdvanced && mmPromptFile ? mmPromptFile : undefined,
        mmShowAdvanced && mmPromptText.trim() ? mmPromptText.trim() : undefined,
      );
      // 持久化克隆音色（首次合成前 get_voice 查不到，本地记录保证下拉可用）
      const cloned = settings?.minimax_global_cloned_voices ?? [];
      if (!cloned.includes(clonedId)) {
        await patch("minimax_global_cloned_voices", [...cloned, clonedId]);
      }
      // 克隆音色无需下载，直接切换
      await patch("plugin_voices", {
        ...(settings?.plugin_voices ?? {}),
        [plugin.id]: clonedId,
      });
      setMmCloneMsg({ ok: true, text: `克隆成功，已切换到音色 ${clonedId}（7 天不使用会被平台回收）` });
      setMmCloneFile("");
      setMmCloneVoiceId("");
      setMmPromptFile("");
      setMmPromptText("");
    } catch (e) {
      setMmCloneMsg({ ok: false, text: String(e) });
    } finally {
      setMmCloning(false);
    }
  }

  /** 刷新账号音色列表（克隆音色须先合成过一次才会出现） */
  async function handleMmRefreshVoices() {
    if (!mmGlobalKey) {
      setMmManageMsg({ ok: false, text: "请先在上方配置卡中填写 MiniMax 国际版 API Key" });
      return;
    }
    setMmVoicesLoading(true);
    setMmManageMsg(null);
    try {
      const raw = await minimaxGlobalGetVoices(mmGlobalKey, MM_GLOBAL_BASE);
      const j = JSON.parse(raw) as {
        system_voice?: MmAccountVoice[];
        voice_cloning?: MmAccountVoice[];
        voice_generation?: MmAccountVoice[];
      };
      setMmAccountVoices({
        cloning: j.voice_cloning ?? [],
        generation: j.voice_generation ?? [],
        systemCount: (j.system_voice ?? []).length,
      });
    } catch (e) {
      setMmManageMsg({ ok: false, text: String(e) });
    } finally {
      setMmVoicesLoading(false);
    }
  }

  /** 确认删除账号音色（删除后 voice_id 不可复用） */
  async function confirmMmDelete() {
    if (!mmDeleteTarget) return;
    const { type, id } = mmDeleteTarget;
    setMmDeleteTarget(null);
    if (!mmGlobalKey) {
      setMmManageMsg({ ok: false, text: "请先在上方配置卡中填写 MiniMax 国际版 API Key" });
      return;
    }
    try {
      await minimaxGlobalDeleteVoice(mmGlobalKey, MM_GLOBAL_BASE, type, id);
      setMmAccountVoices((v) =>
        v
          ? {
              ...v,
              cloning: type === "voice_cloning" ? v.cloning.filter((x) => x.voice_id !== id) : v.cloning,
              generation: type === "voice_generation" ? v.generation.filter((x) => x.voice_id !== id) : v.generation,
            }
          : v,
      );
      // 同步本地克隆记录
      if (type === "voice_cloning") {
        const cloned = settings?.minimax_global_cloned_voices ?? [];
        if (cloned.includes(id)) {
          await patch("minimax_global_cloned_voices", cloned.filter((x) => x !== id));
        }
      }
      // 若删的是当前选中音色，回退插件默认音色
      if (settings?.plugin_voices?.[plugin.id] === id) {
        const fallback = plugin.voices[0]?.id ?? "";
        await patch("plugin_voices", {
          ...(settings?.plugin_voices ?? {}),
          [plugin.id]: fallback,
        });
      }
      setMmManageMsg({ ok: true, text: `音色 ${id} 已删除（该 ID 不可再复用）` });
    } catch (e) {
      setMmManageMsg({ ok: false, text: String(e) });
    }
  }

  /** 把账号音色设为当前引擎音色 */
  async function handleMmUseVoice(voiceId: string) {
    await patch("plugin_voices", {
      ...(settings?.plugin_voices ?? {}),
      [plugin.id]: voiceId,
    });
    setMmManageMsg({ ok: true, text: `已切换到音色 ${voiceId}` });
  }

  // 音色克隆卡（现置于「音色管理」卡下方）
  const clonePanel = (
    <div className="rounded-xl border border-[var(--ink-200)]/70 bg-[var(--ink-100)]/25 px-3.5 py-3">
        <SectionHeading
          title="音色克隆"
          desc="上传 10s~5min 的清晰人声音频（mp3/m4a/wav，≤20MB），克隆出的音色 7 天不使用会被平台回收。"
        />
        {/* 音频文件选择（克隆唯一音源，API 必填） */}
        <div className="mt-2 flex items-center gap-2">
          <button
            onClick={async () => {
              const p = await pickAudioFile();
              if (p) setMmCloneFile(p);
            }}
            className="shrink-0 rounded-lg border border-[var(--ink-200)] px-3 py-1.5 text-[11px] text-[var(--ink-700)] transition-colors hover:border-[var(--amber-500)]"
          >
            选择音频
          </button>
          <span className="truncate text-[11px] text-[var(--ink-500)]">
            {mmCloneFile ? mmCloneFile.split(/[/\\]/).pop() : "未选择（必填，克隆的唯一音源）"}
          </span>
        </div>
        {/* 自定义 voice_id */}
        <input
          value={mmCloneVoiceId}
          onChange={(e) => setMmCloneVoiceId(e.target.value)}
          placeholder="自定义音色 ID（8~256 字符，字母开头）"
          className="mt-2 w-full rounded-xl border border-[var(--ink-200)] bg-transparent px-3 py-1.5 text-[12px] outline-none transition-colors placeholder:text-[var(--ink-300)] focus:border-[var(--amber-500)]"
        />
        {/* 高级选项：样本音频 + 对应文字稿（可选，提升克隆相似度） */}
        <button
          onClick={() => setMmShowAdvanced((v) => !v)}
          className="mt-2 text-[11px] text-[var(--ink-500)] underline underline-offset-2 transition-colors hover:text-[var(--amber-600)]"
        >
          {mmShowAdvanced ? "收起高级选项 ▲" : "高级选项（可选）▼"}
        </button>
        {mmShowAdvanced && (
          <div className="mt-2 space-y-2 rounded-lg border border-dashed border-[var(--ink-200)] p-2.5">
            <p className="text-[10px] leading-relaxed text-[var(--ink-500)]">
              提供一段 8 秒以内的样本音频及其对应文字稿，可提升克隆相似度。
              注意：本项仅增强效果，不能替代上方必填的主音频（平台接口要求两者同时提供）。
            </p>
            <div className="flex items-center gap-2">
              <button
                onClick={async () => {
                  const p = await pickAudioFile();
                  if (p) setMmPromptFile(p);
                }}
                className="shrink-0 rounded-lg border border-[var(--ink-200)] px-3 py-1.5 text-[11px] text-[var(--ink-700)] transition-colors hover:border-[var(--amber-500)]"
              >
                选择样本音频
              </button>
              <span className="truncate text-[11px] text-[var(--ink-500)]">
                {mmPromptFile ? mmPromptFile.split(/[/\\]/).pop() : "未选择"}
              </span>
            </div>
            <input
              value={mmPromptText}
              onChange={(e) => setMmPromptText(e.target.value)}
              placeholder="样本音频对应的文字稿（以标点结尾）"
              className="w-full rounded-xl border border-[var(--ink-200)] bg-transparent px-3 py-1.5 text-[12px] outline-none transition-colors placeholder:text-[var(--ink-300)] focus:border-[var(--amber-500)]"
            />
          </div>
        )}
        {/* 开始克隆（主音频 + 音色 ID 必填；高级选项仅作增强） */}
        <button
          onClick={() => void handleMinimaxClone()}
          disabled={mmCloning || !mmCloneFile || !mmCloneVoiceId.trim()}
          className="mt-2 rounded-lg bg-[var(--amber-500)] px-3 py-1.5 text-[11px] font-medium text-white transition-opacity hover:opacity-90 disabled:opacity-50"
        >
          {mmCloning ? "克隆中…" : "开始克隆"}
        </button>
        {!mmCloning && (!mmCloneFile || !mmCloneVoiceId.trim()) && (
          <p className="mt-1.5 text-[11px] leading-relaxed text-[var(--ink-300)]">
            {!mmCloneFile && !mmCloneVoiceId.trim()
              ? "请先选择主音频并填写音色 ID"
              : !mmCloneFile
                ? "请先选择主音频（10s~5min，高级选项的样本音频不能替代）"
                : "请填写音色 ID"}
          </p>
        )}
        {mmCloneMsg && (
          <p className={`mt-1.5 text-[11px] leading-relaxed ${mmCloneMsg.ok ? "text-[var(--ink-500)]" : "text-red-500"}`}>
            {mmCloneMsg.text}
          </p>
        )}
    </div>
  );

  // 音色管理卡（含「当前音色」下拉插槽，置于克隆之上）
  const managePanel = (
    <div className="rounded-xl border border-[var(--ink-200)]/70 bg-[var(--ink-100)]/25 px-3.5 py-3">
        <SectionHeading
          title="音色管理"
          desc="克隆音色需先成功合成一次，才会出现在 MiniMax 账号列表中（本地下拉不受影响）。"
          right={
            <button
              onClick={handleMmRefreshVoices}
              disabled={mmVoicesLoading}
              className="rounded-lg border border-[var(--ink-200)] px-2.5 py-1 text-[11px] text-[var(--ink-700)] transition-colors hover:border-[var(--amber-500)] disabled:opacity-50"
            >
              {mmVoicesLoading ? "刷新中…" : "刷新账号音色"}
            </button>
          }
        />
        {voiceSelect && <div className="mt-3">{voiceSelect}</div>}
        {mmAccountVoices && (
          <div className="mt-2 space-y-2">
            {[
              { title: "克隆音色", type: "voice_cloning", list: mmAccountVoices.cloning },
              { title: "设计音色", type: "voice_generation", list: mmAccountVoices.generation },
            ].map((g) => (
              <div key={g.type}>
                <p className="text-[10px] font-medium text-[var(--ink-500)]">{g.title}（{g.list.length}）</p>
                {g.list.length === 0 ? (
                  <p className="mt-1 text-[10px] text-[var(--ink-300)]">暂无</p>
                ) : (
                  <div className="mt-1 space-y-1">
                    {g.list.map((v) => (
                      <div key={v.voice_id} className="flex items-center justify-between rounded-lg border border-[var(--ink-200)] px-2.5 py-1.5">
                        <div className="min-w-0 flex-1">
                          <div className="truncate font-mono text-[11px] text-[var(--ink-900)]">{v.voice_id}</div>
                          {v.created_time && (
                            <div className="text-[10px] text-[var(--ink-300)]">{v.created_time}</div>
                          )}
                        </div>
                        <div className="ml-2 flex shrink-0 gap-1">
                          <button
                            onClick={() => handleMmUseVoice(v.voice_id)}
                            className="rounded-md border border-[var(--ink-200)] px-2 py-0.5 text-[10px] text-[var(--ink-700)] transition-colors hover:border-[var(--amber-500)]"
                          >
                            使用
                          </button>
                          <button
                            onClick={() => setMmDeleteTarget({ type: g.type, id: v.voice_id })}
                            className="rounded-md border border-[var(--ink-200)] px-2 py-0.5 text-[10px] text-[var(--seal)] transition-colors hover:border-[var(--seal)]"
                          >
                            删除
                          </button>
                        </div>
                      </div>
                    ))}
                  </div>
                )}
              </div>
            ))}
            <p className="text-[10px] text-[var(--ink-300)]">系统音色：{mmAccountVoices.systemCount} 个（插件已内置静态列表）</p>
          </div>
        )}
        {/* 行内删除确认（删除后 voice_id 不可复用） */}
        {mmDeleteTarget && (
          <div className="mt-2 rounded-lg border border-[var(--amber-200)] bg-[var(--amber-200)]/15 px-3 py-2">
            <p className="text-[11px] leading-relaxed text-[var(--ink-700)]">
              确认删除音色「{mmDeleteTarget.id}」？<b className="text-[var(--seal)]">删除后该 ID 不可复用</b>。
            </p>
            <div className="mt-1.5 flex gap-2">
              <button
                onClick={confirmMmDelete}
                className="rounded-lg bg-[var(--seal)] px-3 py-1 text-[11px] font-medium text-white transition-opacity hover:opacity-90"
              >
                确认删除
              </button>
              <button
                onClick={() => setMmDeleteTarget(null)}
                className="rounded-lg border border-[var(--ink-200)] px-3 py-1 text-[11px] text-[var(--ink-500)] hover:border-[var(--ink-300)]"
              >
                取消
              </button>
            </div>
          </div>
        )}
        {mmManageMsg && (
          <p className={`mt-1.5 text-[11px] leading-relaxed ${mmManageMsg.ok ? "text-[var(--ink-500)]" : "text-red-500"}`}>
            {mmManageMsg.text}
          </p>
        )}
    </div>
  );

  return (
    <>
      {managePanel}
      {clonePanel}
    </>
  );
}
