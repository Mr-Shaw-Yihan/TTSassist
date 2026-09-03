// 通用插件配置面板（manifest 声明驱动）：设置页「插件配置」区按插件渲染卡片。
// 字段控件按声明 type 渲染；secret 值只在提交时单向发送，前端不缓存明文；
// display 为只读展示（插件经宿主能力桥回写，如配对码），设置变化时自动刷新。

import { useEffect, useState } from "react";
import { openUrl } from "@tauri-apps/plugin-opener";
import { getPluginConfig, setPluginConfig, clearPluginConfig } from "../../services/invoke";
import { useSettingsStore } from "../../stores/settingsStore";
import { useTauriListen } from "../../hooks/useTauriListen";
import { SectionHeading } from "../common/SettingsSection";
import type { PluginConfigFieldView } from "../../types";

const SECRET_MASK = "已设置";
const inputCls =
  "w-full rounded-xl border border-[var(--ink-200)] bg-[var(--paper-card)] px-3 py-2 text-sm outline-none transition-colors placeholder:text-[var(--ink-300)] focus:border-[var(--amber-500)]";

// ── 移动端遥控器（lan-remote 专属） ──
// 安卓 App 固定名下载地址（随本体 Release 发布，latest 永远指最新非预发版）
const REMOTE_APK_URL =
  "https://github.com/Mr-Shaw-Yihan/TTSassist/releases/latest/download/voiceassist-remote-latest-arm64.apk";
// 用户交流群号（点击复制）
const REMOTE_QQ_GROUP = "690907648";

async function copyText(text: string) {
  try {
    await navigator.clipboard.writeText(text);
  } catch {
    // WebView 剪贴板 API 不可用时退回 execCommand
    const ta = document.createElement("textarea");
    ta.value = text;
    document.body.appendChild(ta);
    ta.select();
    document.execCommand("copy");
    ta.remove();
  }
}

/** lan-remote 专属面板：移动端遥控器的两个入口（安卓 App / 浏览器网页端） */
function RemotePanel({ fields }: { fields: PluginConfigFieldView[] | null }) {
  const [copied, setCopied] = useState<string | null>(null);
  const hostAddr = fields?.find((f) => f.key === "host_addr")?.value ?? "";
  const webUrl = hostAddr ? `http://${hostAddr}` : "";

  function copy(text: string, tag: string) {
    void copyText(text);
    setCopied(tag);
    setTimeout(() => setCopied(null), 1500);
  }

  return (
    <div className="rounded-xl border border-[var(--ink-200)] bg-[var(--paper-card)] px-3 py-2.5">
      {/* 卡片头 */}
      <div className="flex items-center justify-between gap-2">
        <p className="text-xs font-medium text-[var(--ink-900)]">移动端遥控器</p>
        <span className="rounded-lg bg-[var(--ink-100)] px-2 py-0.5 text-[10px] text-[var(--ink-500)]">
          已启用
        </span>
      </div>
      <p className="mt-1 text-[11px] leading-relaxed text-[var(--ink-400)]">
        用手机遥控这台电脑上的电子声带：游戏内发言、点收藏、开关麦克风，全程不抢焦点。
        首次连接时在电脑弹窗上点「允许」即可。
      </p>

      {/* 网页端：免安装，iOS 仅此入口 */}
      <div className="mt-2.5 rounded-xl border border-dashed border-[var(--ink-200)] bg-[var(--ink-100)]/40 px-3 py-2.5">
        <p className="text-[11px] font-medium text-[var(--ink-700)]">
          网页端 · 免安装（iPhone / iPad 仅支持此方式）
        </p>
        <p className="mt-1 text-[10px] leading-relaxed text-[var(--ink-400)]">
          手机与电脑连同一个 Wi-Fi，用手机浏览器打开下面地址即可，无需安装任何 App。
        </p>
        <div className="mt-1.5 flex items-center gap-1.5">
          <code className="min-w-0 flex-1 truncate rounded-lg bg-[var(--paper)] px-2 py-1.5 font-mono text-xs text-[var(--ink-900)]">
            {webUrl || "等待插件上报地址…"}
          </code>
          <button
            type="button"
            disabled={!webUrl}
            onClick={() => copy(webUrl, "web")}
            className="shrink-0 rounded-lg border border-[var(--ink-200)] px-2.5 py-1.5 text-[11px] text-[var(--ink-700)] transition-colors hover:border-[var(--amber-500)] hover:text-[var(--amber-600)] disabled:opacity-50"
          >
            {copied === "web" ? "已复制 ✓" : "复制"}
          </button>
        </div>
      </div>

      {/* 安卓端：App 下载 + 群号 */}
      <div className="mt-2 rounded-xl border border-dashed border-[var(--ink-200)] bg-[var(--ink-100)]/40 px-3 py-2.5">
        <p className="text-[11px] font-medium text-[var(--ink-700)]">
          安卓 App · 推荐（自动发现电脑、桌面图标直达）
        </p>
        <div className="mt-1.5 flex flex-wrap items-center gap-1.5">
          <button
            type="button"
            onClick={() => openUrl(REMOTE_APK_URL).catch(() => {})}
            className="rounded-lg bg-[var(--ink-900)] px-2.5 py-1.5 text-[11px] font-medium text-[var(--paper)] transition-colors hover:bg-[var(--ink-700)]"
          >
            下载安卓 App ↗
          </button>
          <button
            type="button"
            onClick={() => copy(REMOTE_QQ_GROUP, "qq")}
            className="rounded-lg border border-[var(--ink-200)] px-2.5 py-1.5 text-[11px] text-[var(--ink-700)] transition-colors hover:border-[var(--amber-500)] hover:text-[var(--amber-600)]"
          >
            {copied === "qq" ? "群号已复制 ✓" : `复制群号：${REMOTE_QQ_GROUP}`}
          </button>
        </div>
        <p className="mt-1 text-[10px] leading-relaxed text-[var(--ink-400)]">
          下载后直接安装（允许「未知来源」）；群文件内也有安装包与使用说明。
        </p>
      </div>
    </div>
  );
}

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
  // 监听 settings:changed 重拉声明与值，保证上屏内容即时更新
  useTauriListen("settings:changed", () => {
    void reload();
  }, []);

  if (fields === null || fields.length === 0) return null;

  // display 字段为插件回写的只读展示，不参与编辑与必填校验
  const missingRequired = fields.filter(
    (f) => f.type !== "display" && f.required && !(draft[f.key] ?? "").trim(),
  );
  // 全部为 display 时无用户可编辑项，不渲染保存/清空（纯上屏卡）
  const hasEditable = fields.some((f) => f.type !== "display");

  async function save() {
    if (saving) return;
    setSaving(true);
    setMsg(null);
    try {
      const settings = await setPluginConfig(pluginId, draft);
      setSettings(settings);
      // 保存后刷新：secret 回到「留空保持不变」状态
      await reload();
      setMsg({ ok: true, text: "已保存，立即生效（无需重启）" });
    } catch (e) {
      setMsg({ ok: false, text: String(e) });
    } finally {
      setSaving(false);
    }
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

  // lan-remote 走专属「移动端遥控器」卡片（App/网页端入口），不渲染通用配置表单
  if (pluginId === "lan-remote") {
    return <RemotePanel fields={fields} />;
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
                    onChange={(e) => setDraft((d) => ({ ...d, [f.key]: e.target.value }))}
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
                      onChange={(e) => setDraft((d) => ({ ...d, [f.key]: e.target.value }))}
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

      {/* 保存 + 提示（无用户可编辑字段时整行不渲染） */}
      {hasEditable && (
        <div className="mt-2.5 flex items-center gap-2">
          <button
            onClick={save}
            disabled={saving}
            className="rounded-lg bg-[var(--ink-900)] px-3 py-1.5 text-[11px] font-medium text-[var(--paper)] transition-colors hover:bg-[var(--ink-700)] disabled:opacity-50"
          >
            {saving ? "保存中…" : "保存"}
          </button>
          {missingRequired.length > 0 && (
            <span className="text-[10px] text-[var(--amber-600)]">
              「{missingRequired[0].label}」未填写，合成时插件会提示缺少配置
            </span>
          )}
        </div>
      )}
      {msg && (
        <p className={`mt-1.5 text-[11px] leading-relaxed ${msg.ok ? "text-[var(--ink-500)]" : "text-red-500"}`}>
          {msg.text}
        </p>
      )}
    </div>
  );
}
