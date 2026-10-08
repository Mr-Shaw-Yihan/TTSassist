// 通用插件配置面板（manifest 声明驱动）：设置页「插件配置」区按插件渲染卡片。
// 字段控件按声明 type 渲染；secret 值只在提交时单向发送，前端不缓存明文；
// display 为只读展示（插件经宿主能力桥回写，如配对码），设置变化时自动刷新。
//
// 保存模型（2026-10-08 改版）：改动即自动保存，无「保存」按钮。
// - 防抖 700ms 触发一次 setPluginConfig，避免逐字符狂刷后端；
// - 自动保存成功后【不】reload（reload 会把正在输入的 secret 打回空态、打断续接输入），
//   仅更新全局 settings + 轻提示；初次挂载仍 reload 拉取声明与现值。

import { useEffect, useRef, useState } from "react";
import { openUrl } from "@tauri-apps/plugin-opener";
import { getPluginConfig, setPluginConfig, clearPluginConfig } from "../../services/invoke";
import { useSettingsStore } from "../../stores/settingsStore";
import { useTauriListen } from "../../hooks/useTauriListen";
import { SectionHeading } from "../common/SettingsSection";
import type { PluginConfigFieldView } from "../../types";

const SECRET_MASK = "已设置";
const AUTOSAVE_DEBOUNCE_MS = 700;
const inputCls =
  "w-full rounded-xl border border-[var(--ink-200)] bg-[var(--paper-card)] px-3 py-2 text-sm outline-none transition-colors placeholder:text-[var(--ink-300)] focus:border-[var(--amber-500)]";

/** 单个插件的配置卡片 */
export function PluginConfigPanel({
  pluginId,
  pluginName,
  title,
}: {
  pluginId: string;
  pluginName: string;
  /** 卡片标题；默认用插件名。语音中心传「API 密钥」与内置引擎密钥卡风格统一 */
  title?: string;
}) {
  const setSettings = useSettingsStore((s) => s.setSettings);
  const [fields, setFields] = useState<PluginConfigFieldView[] | null>(null);
  const [helpUrl, setHelpUrl] = useState<string | null>(null);
  // 编辑态：key → 输入值（secret 初值为空，靠 placeholder 提示已有值）
  const [draft, setDraft] = useState<Record<string, string>>({});
  const [hadSecret, setHadSecret] = useState<Record<string, boolean>>({});
  const [showSecret, setShowSecret] = useState<Record<string, boolean>>({});
  const [saving, setSaving] = useState(false);
  const [msg, setMsg] = useState<{ ok: boolean; text: string } | null>(null);
  // 是否有待保存的改动（用户编辑置位；自动保存成功清零）。初次加载不置位。
  const [dirty, setDirty] = useState(false);
  // 保存时读取最新 draft，避免闭包捕获旧值
  const draftRef = useRef(draft);
  draftRef.current = draft;

  async function reload() {
    try {
      const info = await getPluginConfig(pluginId);
      const d: Record<string, string> = {};
      const h: Record<string, boolean> = {};
      for (const f of info.fields) {
        if (f.type === "secret") {
          d[f.key] = "";
          h[f.key] = f.value === SECRET_MASK;
        } else {
          d[f.key] = f.value;
        }
      }
      setFields(info.fields);
      setHelpUrl(info.help_url ?? null);
      setDraft(d);
      setHadSecret(h);
      setDirty(false);
    } catch (e) {
      // 声明拉不到（插件刚卸载等）直接隐藏卡片
      setFields([]);
    }
  }

  useEffect(() => {
    void reload();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [pluginId]);

  // 插件可经宿主能力桥回写 display 字段（如遥控配对码刷新）：
  // 监听 settings:changed 重拉声明与值，保证上屏内容即时更新。
  // 注意：仅当本卡没有未保存改动时才重拉，避免覆盖用户正在编辑的内容。
  useTauriListen("settings:changed", () => {
    if (!dirty) void reload();
  }, [dirty]);

  // 防抖自动保存：dirty 置位后静置一段时间落盘（不 reload，见文件头说明）
  useEffect(() => {
    if (!dirty || saving) return;
    const t = window.setTimeout(async () => {
      try {
        setSaving(true);
        setMsg(null);
        const settings = await setPluginConfig(pluginId, draftRef.current);
        setSettings(settings);
        setDirty(false);
        setMsg({ ok: true, text: "已自动保存，立即生效（无需重启）" });
      } catch (e) {
        setMsg({ ok: false, text: String(e) });
      } finally {
        setSaving(false);
      }
    }, AUTOSAVE_DEBOUNCE_MS);
    return () => window.clearTimeout(t);
  }, [draft, dirty, saving, pluginId, setSettings]);

  if (fields === null || fields.length === 0) return null;

  // display 字段为插件回写的只读展示，不参与编辑与必填校验
  const missingRequired = fields.filter(
    (f) => f.type !== "display" && f.required && !(draft[f.key] ?? "").trim(),
  );
  // 全部为 display 时无用户可编辑项（纯上屏卡）
  const hasEditable = fields.some((f) => f.type !== "display");

  /** 编辑任一字段：写 draft 并标记为待自动保存 */
  function updateField(key: string, value: string) {
    setDraft((d) => ({ ...d, [key]: value }));
    setDirty(true);
  }

  async function clear() {
    if (!window.confirm(`确认清空「${pluginName}」的全部配置？清空后需重新填写。`)) return;
    try {
      const settings = await clearPluginConfig(pluginId);
      setSettings(settings);
      await reload();
      setMsg({ ok: true, text: "已清空配置" });
    } catch (e) {
      setMsg({ ok: false, text: String(e) });
    }
  }

  return (
    <div className="rounded-xl border border-[var(--ink-200)]/70 bg-[var(--ink-100)]/25 px-3.5 py-3">
      {/* 卡片头：标题（默认插件名；语音中心传「API 密钥」与内置引擎统一）+ 获取链接 + 清空 */}
      <SectionHeading
        title={title ?? pluginName}
        right={
          <div className="flex shrink-0 items-center gap-1.5">
            {helpUrl && (
              <button
                onClick={() => openUrl(helpUrl).catch(() => {})}
                className="rounded-lg border border-[var(--ink-200)] px-2 py-1 text-[11px] text-[var(--ink-700)] transition-colors hover:border-[var(--amber-500)] hover:text-[var(--amber-600)]"
              >
                获取 API Key ↗
              </button>
            )}
            {hasEditable && (
              <button
                onClick={clear}
                className="rounded-lg border border-[var(--ink-200)] px-2 py-1 text-[11px] text-[var(--ink-500)] transition-colors hover:border-[var(--seal)] hover:text-[var(--seal)]"
              >
                清空
              </button>
            )}
          </div>
        }
      />

      {/* 字段控件 */}
      <div className="mt-2.5 space-y-2.5">
        {fields.map((f) => (
          <div key={f.key}>
            <label className="mb-1 flex items-baseline gap-1.5 text-xs text-[var(--ink-700)]">
              <span className="font-medium">{f.label}</span>
              {f.required && <span className="text-[10px] text-[var(--seal)]">必填</span>}
            </label>
            {f.description && (
              <p className="mb-1 text-[10px] leading-relaxed text-[var(--ink-400)]">{f.description}</p>
            )}
            {f.type === "display" ? (
              // 只读展示字段（插件回写）：空值给占位提示
              <div className="rounded-xl border border-dashed border-[var(--ink-200)] bg-[var(--ink-100)]/40 px-3 py-2 font-mono text-sm tracking-widest text-[var(--ink-900)]">
                {f.value || "—"}
              </div>
            ) : (
              <div className="flex items-center gap-1.5">
                {f.type === "select" ? (
                  <select
                    value={draft[f.key] ?? ""}
                    onChange={(e) => updateField(f.key, e.target.value)}
                    className={inputCls}
                  >
                    {(f.options ?? []).map((o) => (
                      <option key={o.value} value={o.value}>{o.label}</option>
                    ))}
                  </select>
                ) : (
                  <>
                    <input
                      type={f.type === "secret" && !showSecret[f.key] ? "password" : "text"}
                      value={draft[f.key] ?? ""}
                      onChange={(e) => updateField(f.key, e.target.value)}
                      placeholder={
                        f.type === "secret" && hadSecret[f.key]
                          ? `${SECRET_MASK}（留空保持不变）`
                          : f.placeholder
                      }
                      className={inputCls}
                    />
                    {f.type === "secret" && (
                      <button
                        type="button"
                        onClick={() => setShowSecret((s) => ({ ...s, [f.key]: !s[f.key] }))}
                        title={showSecret[f.key] ? "隐藏" : "显示"}
                        className="shrink-0 rounded-lg border border-[var(--ink-200)] px-2 py-1 text-[11px] text-[var(--ink-500)] hover:border-[var(--ink-300)]"
                      >
                        {showSecret[f.key] ? "🙈" : "👁"}
                      </button>
                    )}
                  </>
                )}
              </div>
            )}
          </div>
        ))}
      </div>

      {/* 自动保存状态行（无用户可编辑字段时不渲染） */}
      {hasEditable && (
        <div className="mt-2.5 flex items-center gap-2 text-[11px]">
          {saving ? (
            <span className="inline-flex items-center gap-1.5 text-[var(--ink-500)]">
              <span className="inline-block h-2 w-2 animate-pulse rounded-full bg-[var(--amber-500)]" />
              保存中…
            </span>
          ) : dirty ? (
            <span className="text-[var(--ink-400)]">编辑后将自动保存…</span>
          ) : msg ? (
            <span className={msg.ok ? "text-[var(--ink-500)]" : "text-[var(--seal)]"}>{msg.text}</span>
          ) : (
            <span className="text-[var(--ink-400)]">改动自动保存</span>
          )}
          {missingRequired.length > 0 && (
            <span className="text-[var(--amber-600)]">
              「{missingRequired[0].label}」未填写，合成时插件会提示缺少配置
            </span>
          )}
        </div>
      )}
    </div>
  );
}
