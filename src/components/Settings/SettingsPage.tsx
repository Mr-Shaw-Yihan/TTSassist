// 设置页面（主界面侧边栏"设置"项的右侧内容区）：分类收纳（手风琴）。
// 分类：虚拟麦克风 / 插件服务（service 型插件配置） / 快捷键 / 悬浮球 / 外观 / 关于。
// 注：TTS「语音合成」与 ASR「语音输入」已析出至顶层「语音」中心页（VoiceCenterPage），此处不再承载引擎/音色配置。

import { useState, useEffect } from "react";
import { getVersion } from "@tauri-apps/api/app";
import { openUrl } from "@tauri-apps/plugin-opener";
import { useSettingsStore } from "../../stores/settingsStore";
import { useUpdateStore, shouldShowUpdateDot } from "../../stores/updateStore";
import { listPlugins, setHotkey, setVoiceInputHotkey, setPlayLastHotkey, setMicToggleHotkey } from "../../services/invoke";
import { resetFloatingBallPos } from "../../services/invoke";
import type { PluginInfo } from "../../types";
import { HotkeyRecorder } from "./HotkeyRecorder";
import { MicSettings } from "./MicSettings";
import { PluginConfigPanel } from "./PluginConfigPanel";
import { Section } from "../common/SettingsSection";

const THEMES = [
  { id: "light", label: "安墨（浅色）", desc: "宣纸暖白 · 墨色 · 暖琥珀" },
  { id: "dark",  label: "夜窗（深色）", desc: "深炭灰 · 琥珀高光 · 夜间友好" },
] as const;

export function SettingsPage() {
  const settings = useSettingsStore((s) => s.settings);
  const patch = useSettingsStore((s) => s.patch);

  // 版本更新：关于区红点 + 新版本入口
  const updateLatest = useUpdateStore((s) => s.latest);
  const updateChecked = useUpdateStore((s) => s.checked);
  const checkUpdate = useUpdateStore((s) => s.check);
  const resetDialog = useUpdateStore((s) => s.resetDialog);
  const updateDot = useUpdateStore(shouldShowUpdateDot);
  const markAboutSeen = useUpdateStore((s) => s.markAboutSeen);

  // 关于：当前版本号 + 手动检查更新
  const [appVersion, setAppVersion] = useState<string>("");
  const [checkingUpdate, setCheckingUpdate] = useState(false);
  useEffect(() => {
    getVersion().then(setAppVersion).catch(() => {});
  }, []);

  async function handleCheckUpdate() {
    setCheckingUpdate(true);
    try {
      await checkUpdate();
      // 手动检查后重置弹窗状态，使新版本弹窗可以再次弹出
      resetDialog();
    } finally {
      setCheckingUpdate(false);
    }
  }

  // service 型插件配置（如手机遥控）需插件列表
  const [plugins, setPlugins] = useState<PluginInfo[]>([]);
  useEffect(() => {
    listPlugins().then(setPlugins).catch(() => {});
  }, []);

  return (
    <div className="flex h-full flex-col">
      <div className="scrollbar-thin flex-1 space-y-2.5 overflow-y-auto px-4 py-5 text-sm">
        <div className="mx-auto max-w-xl space-y-2.5">
          {/* 虚拟麦克风 */}
          <Section title="虚拟麦克风">
            <MicSettings />
          </Section>

          {/* 插件服务（service 类型插件，如手机遥控）：通用配置卡上屏（含 display 只读字段） */}
          {plugins.some((p) => p.plugin_type === "service" && p.config) && (
            <Section title="插件服务">
              {plugins
                .filter((p) => p.plugin_type === "service" && p.config)
                .map((p) => (
                  <PluginConfigPanel key={p.id} pluginId={p.id} pluginName={p.name} />
                ))}
            </Section>
          )}

          {/* 快捷键：浮窗呼出 / 语音输入 / 播放最近一条消息 / 开关发送到麦克风 */}
          <Section title="快捷键">
            <div className="space-y-4">
              <HotkeyRow
                label="呼出浮窗"
                value={settings?.hotkey_show_window ?? "Alt+V"}
                onApply={setHotkey}
                hint="按下显示/收起快速输入浮窗。点「录入」后按下想要的组合键（如 Alt+V、Ctrl+Shift+F1）。"
              />
              <HotkeyRow
                label="语音输入（按住说话）"
                value={settings?.voice_input_hotkey ?? ""}
                onApply={setVoiceInputHotkey}
                clearable
                hint="按住快捷键开始录音，松开自动识别并填入输入框（需已安装识别插件）。"
              />
              <HotkeyRow
                label="播放最近一条消息"
                value={settings?.hotkey_play_last ?? ""}
                onApply={setPlayLastHotkey}
                clearable
                hint="按下即播最近一条消息的语音；麦克风发送开关开启时同时发到虚拟麦克风。"
              />
              <HotkeyRow
                label="开关发送到麦克风"
                value={settings?.hotkey_mic_toggle ?? ""}
                onApply={setMicToggleHotkey}
                clearable
                hint="按下切换「语音是否发送到虚拟麦克风」开关，无需鼠标操作。"
              />
            </div>
          </Section>

          {/* 悬浮球：显隐由主窗标题栏 logo 接管；本栏管理大小/位置/（皮肤预留） */}
          <Section title="悬浮球">
            <div className="space-y-4">
              <p className="text-[11px] leading-relaxed text-[var(--ink-300)]">
                常驻置顶的角色球，适合快捷键无法触发的游戏。显隐点主窗标题栏左侧小球 logo 切换：
                放出唤醒常驻、收回播放退场动画归位 logo。单击球展开输入浮窗，拖拽移动，右键开菜单。
              </p>

              {/* 球体大小：三档固定大小（固定档位比滑块更不易出 bug）。
                  标准 56 = 初始档位；小 44 / 大 72 为缩小、放大档，均在后端 40~96 安全范围内 */}
              <div>
                <div className="text-[11px] text-[var(--ink-300)]">球体大小</div>
                <div className="mt-1.5 grid grid-cols-3 gap-1 rounded-xl bg-[var(--ink-100)]/60 p-1">
                  {([
                    { label: "小", size: 44 },
                    { label: "标准", size: 56 },
                    { label: "大", size: 72 },
                  ] as const).map((tier) => {
                    const cur = settings?.floating_ball_size ?? 56;
                    const active = cur === tier.size;
                    return (
                      <button
                        key={tier.label}
                        type="button"
                        onClick={() => { void patch("floating_ball_size", tier.size).catch(() => {}); }}
                        aria-pressed={active}
                        className={[
                          "rounded-lg py-1.5 text-xs transition-colors",
                          active
                            ? "bg-[var(--paper-card)] font-semibold text-[var(--ink-900)] shadow-sm"
                            : "text-[var(--ink-400)] hover:text-[var(--ink-700)]",
                        ].join(" ")}
                      >
                        {tier.label}
                      </button>
                    );
                  })}
                </div>
              </div>

              {/* 性能策略：标准（跟随+波纹+全帧率）/ 性能（关跟随波纹、30fps 封顶） */}
              <div>
                <div className="text-[11px] text-[var(--ink-300)]">性能策略</div>
                <div className="mt-1.5 grid grid-cols-2 gap-1 rounded-xl bg-[var(--ink-100)]/60 p-1">
                  {([
                    { id: "standard", label: "标准模式" },
                    { id: "performance", label: "性能模式" },
                  ] as const).map((m) => {
                    const active = (settings?.floating_ball_perf_mode ?? "standard") === m.id;
                    return (
                      <button
                        key={m.id}
                        type="button"
                        onClick={() => { void patch("floating_ball_perf_mode", m.id).catch(() => {}); }}
                        aria-pressed={active}
                        className={[
                          "rounded-lg py-1.5 text-xs transition-colors",
                          active
                            ? "bg-[var(--paper-card)] font-semibold text-[var(--ink-900)] shadow-sm"
                            : "text-[var(--ink-400)] hover:text-[var(--ink-700)]",
                        ].join(" ")}
                      >
                        {m.label}
                      </button>
                    );
                  })}
                </div>
                <p className="mt-1 text-[10px] leading-relaxed text-[var(--ink-300)]">
                  标准模式：指针跟随 + 音频波纹 + 全帧率；性能模式：关闭指针跟随与波纹、动画 30fps 封顶，游戏内或低配机器推荐。
                </p>
              </div>

              {/* 还原位置：球意外出屏/找不到时的恢复入口 */}
              <div className="flex items-center justify-between gap-3">
                <div className="min-w-0">
                  <div className="text-xs font-medium text-[var(--ink-700)]">还原悬浮球位置</div>
                  <p className="mt-0.5 text-[11px] text-[var(--ink-300)]">球被拖出屏幕或找不到时，点击重置到屏幕中央。</p>
                </div>
                <button
                  type="button"
                  onClick={() => {
                    void resetFloatingBallPos().catch((e) => window.alert(`还原位置失败：${e}`));
                  }}
                  className="shrink-0 rounded-lg border border-[var(--ink-200)] bg-[var(--paper-card)] px-3 py-1.5 text-xs text-[var(--ink-700)] transition-colors hover:border-[var(--amber-500)] hover:text-[var(--amber-600)]"
                >
                  还原
                </button>
              </div>

              {/* 角色皮肤：内置墨黑 / 素白两款 */}
              <div>
                <div className="text-[11px] text-[var(--ink-300)]">角色皮肤</div>
                <div className="mt-1.5 grid grid-cols-2 gap-1 rounded-xl bg-[var(--ink-100)]/60 p-1">
                  {([
                    { id: "ink", label: "墨黑" },
                    { id: "white", label: "素白" },
                  ] as const).map((s) => {
                    const active = (settings?.floating_ball_skin ?? "ink") === s.id;
                    return (
                      <button
                        key={s.id}
                        type="button"
                        onClick={() => { void patch("floating_ball_skin", s.id).catch(() => {}); }}
                        aria-pressed={active}
                        className={[
                          "flex items-center justify-center gap-1.5 rounded-lg py-1.5 text-xs transition-colors",
                          active
                            ? "bg-[var(--paper-card)] font-semibold text-[var(--ink-900)] shadow-sm"
                            : "text-[var(--ink-400)] hover:text-[var(--ink-700)]",
                        ].join(" ")}
                      >
                        <span
                          aria-hidden
                          className={[
                            "h-3 w-3 rounded-full",
                            s.id === "ink" ? "bg-[#0a0a0a]" : "bg-[#f7f4ec] ring-1 ring-[var(--ink-200)]",
                          ].join(" ")}
                        />
                        {s.label}
                      </button>
                    );
                  })}
                </div>
              </div>
            </div>
          </Section>

          {/* 外观 */}
          <Section title="外观">
            <div className="grid grid-cols-2 gap-2">
              {THEMES.map((t) => {
                const active = (settings?.theme ?? "light") === t.id;
                return (
                  <button
                    key={t.id}
                    onClick={() => patch("theme", t.id)}
                    className={[
                      "rounded-xl border px-3 py-2.5 text-left transition-all",
                      active
                        ? "border-[var(--amber-500)] bg-[var(--amber-200)]/30 ring-1 ring-[var(--amber-500)]/40"
                        : "border-[var(--ink-200)] bg-[var(--paper-card)] hover:border-[var(--ink-300)]",
                    ].join(" ")}
                  >
                    <div className={["text-xs font-medium", active ? "text-[var(--amber-600)]" : "text-[var(--ink-700)]"].join(" ")}>
                      {t.label}
                    </div>
                    <div className="mt-0.5 text-[10px] leading-relaxed text-[var(--ink-300)]">
                      {t.desc}
                    </div>
                  </button>
                );
              })}
            </div>
          </Section>

          {/* 关于 */}
          <Section
            title={
              <>
                关于
                {updateDot && (
                  <span className="ml-1.5 inline-block h-1.5 w-1.5 rounded-full bg-[var(--seal)] align-middle" />
                )}
              </>
            }
            defaultOpen={!!updateLatest}
            onOpen={markAboutSeen}
          >
            <div className="text-xs leading-relaxed text-[var(--ink-500)]">
              <div className="font-display text-sm font-medium text-[var(--ink-900)]">电子声带 TTSassist</div>
              <div className="mt-1.5">为语言障碍者打造的文本转语音沟通助手。</div>

              {/* 项目链接 & QQ 群 */}
              <div className="mt-2.5 flex flex-col gap-1.5 text-[11px]">
                <div className="flex items-center gap-2">
                  <span className="text-[var(--ink-400)]">GitHub</span>
                  <button
                    onClick={() => openUrl("https://github.com/Mr-Shaw-Yihan/TTSassist").catch(() => {})}
                    className="text-[var(--amber-600)] underline underline-offset-2 hover:text-[var(--amber-700)]"
                  >
                    github.com/Mr-Shaw-Yihan/TTSassist
                  </button>
                </div>
                <div className="flex items-center gap-2">
                  <span className="text-[var(--ink-400)]">QQ 群</span>
                  <button
                    onClick={() => {
                      navigator.clipboard.writeText("690907648");
                      const el = document.getElementById("qq-copied-tip");
                      if (el) { el.style.opacity = "1"; setTimeout(() => el.style.opacity = "0", 1500); }
                    }}
                    className="relative font-mono text-[var(--ink-600)] underline decoration-dashed underline-offset-2 hover:text-[var(--amber-600)]"
                    title="点击复制群号"
                  >
                    690907648
                    <span
                      id="qq-copied-tip"
                      className="pointer-events-none absolute -top-6 left-1/2 -translate-x-1/2 rounded bg-[var(--ink-700)] px-1.5 py-0.5 text-[10px] text-[var(--paper)] opacity-0 transition-opacity"
                    >
                      已复制
                    </span>
                  </button>
                </div>
              </div>

              {/* 当前版本 + 检查更新 */}
              <div className="mt-3 flex items-center gap-2">
                <span className="rounded-md bg-[var(--ink-100)] px-2 py-0.5 font-mono text-[11px] text-[var(--ink-500)]">
                  {appVersion ? `v${appVersion}` : "…"}
                </span>
                <button
                  onClick={handleCheckUpdate}
                  disabled={checkingUpdate}
                  className="rounded-lg border border-[var(--ink-200)] px-2.5 py-1 text-[11px] text-[var(--ink-500)] transition-colors hover:border-[var(--amber-500)] hover:text-[var(--amber-600)] disabled:opacity-50"
                >
                  {checkingUpdate ? "检查中…" : "检查更新"}
                </button>
              </div>

              {/* 检查/启动检查结果 */}
              {updateLatest ? (
                <div className="mt-2.5 rounded-lg border border-[var(--amber-200)] bg-[var(--amber-200)]/20 px-3 py-2 text-[11px] leading-relaxed text-[var(--amber-600)]">
                  发现新版本 <span className="font-mono font-medium">v{updateLatest.version}</span>
                  ，建议更新以获得新功能与修复。
                  <button
                    onClick={() => openUrl(updateLatest.url).catch(() => {})}
                    className="ml-1.5 font-medium underline underline-offset-2 hover:text-[var(--ink-700)]"
                  >
                    前往下载
                  </button>
                </div>
              ) : (
                updateChecked && (
                  <div className="mt-2.5 text-[11px] text-[var(--ink-300)]">已是最新版本</div>
                )
              )}

              {/* 诊断日志（支持模式）：默认关，开启后运行日志落本地文件，便于反馈问题 */}
              <div className="mt-3 rounded-lg border border-[var(--ink-200)] bg-[var(--ink-100)]/40 px-3 py-2">
                <div className="flex items-center justify-between gap-2">
                  <div className="text-[11px] font-medium text-[var(--ink-600)]">诊断日志（支持模式）</div>
                  <button
                    type="button"
                    onClick={() => { void patch("diagnostics_log_enabled", !(settings?.diagnostics_log_enabled ?? false)).catch(() => {}); }}
                    aria-pressed={settings?.diagnostics_log_enabled ?? false}
                    className={[
                      "shrink-0 rounded-full px-3 py-1 text-[11px] font-medium transition-colors",
                      (settings?.diagnostics_log_enabled ?? false)
                        ? "bg-[var(--amber-500)] text-[var(--paper)]"
                        : "border border-[var(--ink-200)] text-[var(--ink-400)] hover:text-[var(--ink-700)]",
                    ].join(" ")}
                  >
                    {(settings?.diagnostics_log_enabled ?? false) ? "已开启" : "已关闭"}
                  </button>
                </div>
                <div className="mt-1 text-[10px] leading-relaxed text-[var(--ink-300)]">
                  开启后会把运行日志额外保存到本地 <span className="font-mono">…/logs/app.log</span>，仅用于排查问题；日志不含你合成的文本，也不会上传。反馈问题时把该文件发给开发者即可。
                </div>
              </div>

              {/* 免责声明 */}
              <div className="mt-3 rounded-lg border border-[var(--ink-200)] bg-[var(--ink-100)]/40 px-3 py-2 text-[10px] leading-relaxed text-[var(--ink-300)]">
                <div className="mb-1 font-medium text-[var(--ink-500)]">免责声明</div>
                本软件为开源项目，仅供学习与个人使用。软件仅提供本地服务功能，语音合成能力由第三方运营商服务提供，API Key 由用户自行申请，相关条款与资费以运营商为准。本软件不收集、不上传任何用户个人信息，所有数据仅存储于本地。使用产生的任何后果由用户自行承担。
              </div>
            </div>
          </Section>
        </div>
      </div>
    </div>
  );
}

/** 快捷键设置行：琥珀竖条 + 衬线标题（与插件页分类标题同款）+ 描述 + 录入器 + 清除按钮 */
function HotkeyRow({
  label,
  value,
  onApply,
  hint,
  clearable = false,
}: {
  label: string;
  value: string;
  onApply: (accel: string) => Promise<void>;
  hint?: string;
  clearable?: boolean;
}) {
  return (
    <div>
      {/* 标题行：与插件页分类头部同款（琥珀竖条 + 衬线标题） */}
      <div className="flex items-center gap-2">
        <span className="h-3.5 w-[3px] shrink-0 rounded-full bg-[var(--amber-500)]" aria-hidden />
        <h3 className="font-display text-sm font-semibold tracking-wide text-[var(--ink-900)]">
          {label}
        </h3>
      </div>
      {hint && (
        <p className="mt-1 pl-[11px] text-[11px] leading-relaxed text-[var(--ink-300)]">{hint}</p>
      )}
      <div className="mt-2 flex items-center gap-2 pl-[11px]">
        <div className="min-w-0 flex-1">
          <HotkeyRecorder value={value} onApply={onApply} />
        </div>
        {clearable && value && (
          <button
            onClick={async () => {
              try {
                await onApply("");
              } catch (e) {
                window.alert(`清除快捷键失败：${e}`);
              }
            }}
            className="shrink-0 rounded-lg border border-[var(--ink-200)] px-2.5 py-2 text-xs text-[var(--ink-300)] transition-colors hover:border-[var(--seal)] hover:text-[var(--seal)]"
          >
            清除
          </button>
        )}
      </div>
    </div>
  );
}
