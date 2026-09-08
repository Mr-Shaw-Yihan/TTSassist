// 移动端遥控页（侧边栏「遥控」Tab 的右侧内容区）。
// 本体功能（原 lan-remote 插件迁入本体后常驻）：实时连接态 + 连接自检 + 网页端 / 安卓 App 两个入口。
// - 实时状态卡：挂载时拉一次 remote_session_info，之后监听 remote:status 事件（本体在连接/断开/配对/顶替时推送）；
// - 连接自检：端口监听 / 防火墙 chips + 刷新 + 一键放行（复用 remote_lan_status / remote_firewall_open）；
// - 入口：网页端（免安装，iOS 仅此方式）与安卓 App（下载 + 群号）。

import { useEffect, useState } from "react";
import { openUrl } from "@tauri-apps/plugin-opener";
import {
  getRemoteLanStatus,
  openRemoteFirewall,
  getRemoteSessionInfo,
} from "../../services/invoke";
import type { LanStatus, RemoteSessionInfo } from "../../services/invoke";
import { useTauriListen } from "../../hooks/useTauriListen";

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

/** ISO8601 → 「YYYY-MM-DD HH:mm」本地时间（解析失败原样返回） */
function fmtTime(iso: string | null | undefined): string {
  if (!iso) return "";
  const d = new Date(iso);
  if (isNaN(d.getTime())) return iso;
  const pad = (n: number) => String(n).padStart(2, "0");
  return `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())} ${pad(d.getHours())}:${pad(d.getMinutes())}`;
}

/** 状态芯片：undefined=未知 / true=正常 / false=异常 */
function StatusChip({ ok, label }: { ok?: boolean; label: string }) {
  const state = ok === undefined ? "unknown" : ok ? "ok" : "bad";
  return (
    <span
      className={[
        "rounded-md px-1.5 py-0.5 text-[10px]",
        state === "ok"
          ? "bg-emerald-600/10 text-emerald-700"
          : state === "bad"
            ? "bg-[var(--seal)]/10 text-[var(--seal)]"
            : "bg-[var(--ink-100)] text-[var(--ink-500)]",
      ].join(" ")}
    >
      {label}
    </span>
  );
}

export function RemotePage() {
  const [session, setSession] = useState<RemoteSessionInfo | null>(null);
  const [copied, setCopied] = useState<string | null>(null);
  const [status, setStatus] = useState<LanStatus | null>(null);
  const [checking, setChecking] = useState(false);
  const [fwBusy, setFwBusy] = useState(false);
  const [fwMsg, setFwMsg] = useState<{ ok: boolean; text: string } | null>(null);

  async function refresh() {
    setChecking(true);
    try {
      setStatus(await getRemoteLanStatus());
    } catch {
      setStatus(null);
    } finally {
      setChecking(false);
    }
  }

  // 挂载时拉一次自检 + 会话信息
  useEffect(() => {
    void refresh();
    getRemoteSessionInfo().then(setSession).catch(() => {});
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  // 实时连接态：本体在连接/断开/配对/顶替时推送 remote:status（payload 即 SessionInfo）
  useTauriListen<RemoteSessionInfo>("remote:status", (payload) => {
    setSession(payload);
  }, []);

  async function openFw() {
    if (fwBusy) return;
    setFwBusy(true);
    setFwMsg(null);
    try {
      const msg = await openRemoteFirewall();
      setFwMsg({ ok: true, text: msg });
      await refresh();
    } catch (e) {
      setFwMsg({ ok: false, text: String(e) });
    } finally {
      setFwBusy(false);
    }
  }

  function copy(text: string, tag: string) {
    void copyText(text);
    setCopied(tag);
    setTimeout(() => setCopied(null), 1500);
  }

  const port = status?.port ?? 45271;
  // 候选地址：自检到的网卡（默认出口在前）
  const candidates =
    status?.ips.map((x) => ({
      iface: x.iface,
      url: `http://${x.ip}:${port}`,
      isDefault: x.default,
    })) ?? [];
  const primaryUrl = candidates[0]?.url ?? "";

  const connected = session?.connected ?? false;
  const paired = session?.paired ?? false;
  const deviceName = session?.device?.trim() || "已配对设备";

  return (
    <div className="flex h-full flex-col">
      <div className="scrollbar-thin flex-1 overflow-y-auto px-4 py-5 text-sm">
        <div className="mx-auto max-w-xl space-y-2.5">
          {/* 页头 */}
          <div>
            <div className="flex items-center gap-2">
              <span className="h-4 w-[3px] shrink-0 rounded-full bg-[var(--amber-500)]" aria-hidden />
              <h2 className="font-display text-base font-semibold tracking-wide text-[var(--ink-900)]">
                移动端遥控器
              </h2>
              <span className="rounded-lg bg-[var(--ink-100)] px-2 py-0.5 text-[10px] text-[var(--ink-500)]">
                已启用
              </span>
            </div>
            <p className="mt-1.5 text-[11px] leading-relaxed text-[var(--ink-400)]">
              用手机遥控这台电脑上的电子声带：游戏内发言、点收藏、开关麦克风，全程不抢焦点。
              首次连接时在电脑弹窗上点「允许」即可。
            </p>
          </div>

          {/* 实时状态卡：当前连接 + 已配对设备 */}
          <div className="rounded-xl border border-[var(--ink-200)] bg-[var(--paper-card)] px-3.5 py-3">
            <div className="flex items-center justify-between">
              <p className="text-xs font-medium text-[var(--ink-900)]">连接状态</p>
              <span className="flex items-center gap-1.5 text-[10px] text-[var(--ink-400)]">
                <span
                  className={[
                    "h-1.5 w-1.5 rounded-full",
                    connected ? "animate-pulse bg-emerald-500" : "bg-[var(--ink-300)]",
                  ].join(" ")}
                />
                {connected ? "实时在线" : "待机中"}
              </span>
            </div>

            {/* 当前连接 */}
            <div className="mt-2.5 rounded-xl border border-[var(--ink-200)]/70 bg-[var(--ink-100)]/40 px-3 py-2.5">
              <div className="flex items-center gap-2">
                <span
                  className={[
                    "h-2 w-2 shrink-0 rounded-full",
                    connected ? "bg-emerald-500" : "bg-[var(--ink-300)]",
                  ].join(" ")}
                />
                <p className="text-[11px] font-medium text-[var(--ink-700)]">
                  {connected ? "已连接" : "未连接"}
                </p>
                {connected && (
                  <span className="min-w-0 flex-1 truncate text-[11px] text-[var(--ink-500)]">
                    {deviceName}
                    {session?.peer ? ` · ${session.peer}` : ""}
                  </span>
                )}
              </div>
              {!connected && (
                <p className="mt-1 pl-4 text-[10px] leading-relaxed text-[var(--ink-400)]">
                  手机 App 或网页端连上后，这里会实时显示设备名与对端地址。
                </p>
              )}
            </div>

            {/* 已配对设备 */}
            <div className="mt-2 rounded-xl border border-[var(--ink-200)]/70 bg-[var(--ink-100)]/40 px-3 py-2.5">
              <div className="flex items-center gap-2">
                <span
                  className={[
                    "h-2 w-2 shrink-0 rounded-full",
                    paired ? "bg-[var(--amber-500)]" : "bg-[var(--ink-300)]",
                  ].join(" ")}
                />
                <p className="text-[11px] font-medium text-[var(--ink-700)]">
                  {paired ? "已配对设备" : "未配对"}
                </p>
                {paired && (
                  <span className="min-w-0 flex-1 truncate text-[11px] text-[var(--ink-500)]">
                    {deviceName}
                    {session?.last_seen ? ` · 最近连接 ${fmtTime(session.last_seen)}` : ""}
                  </span>
                )}
              </div>
              {!paired && (
                <p className="mt-1 pl-4 text-[10px] leading-relaxed text-[var(--ink-400)]">
                  首次在手机端点连接时，电脑会弹窗询问，点「允许」即完成配对（免输入配对码）。
                </p>
              )}
            </div>
          </div>

          {/* 连接自检 */}
          <div className="rounded-xl border border-[var(--ink-200)] bg-[var(--paper-card)] px-3.5 py-3">
            <div className="flex items-center justify-between">
              <p className="text-xs font-medium text-[var(--ink-900)]">连接自检</p>
              <button
                type="button"
                onClick={() => void refresh()}
                disabled={checking}
                className="rounded-lg border border-[var(--ink-200)] px-2 py-0.5 text-[10px] text-[var(--ink-500)] transition-colors hover:border-[var(--amber-500)] hover:text-[var(--amber-600)] disabled:opacity-50"
              >
                {checking ? "检查中…" : "刷新"}
              </button>
            </div>
            <div className="mt-2 flex flex-wrap gap-1.5">
              <StatusChip ok={status?.listening} label={status?.listening ? `端口 ${port} 监听中` : "端口未监听"} />
              <StatusChip ok={status?.firewall} label={status?.firewall ? "防火墙已放行" : "防火墙未放行"} />
            </div>

            {/* 网卡 IP 候选（网页端地址） */}
            <div className="mt-2 space-y-1">
              {candidates.length === 0 && (
                <p className="text-[10px] text-[var(--ink-400)]">
                  未检测到局域网 IP——请确认电脑已连 Wi-Fi/网线，且未仅启用 VPN/热点。
                </p>
              )}
              {candidates.map((c) => (
                <div key={c.url} className="flex items-center gap-1.5">
                  <span className="min-w-0 flex-1 truncate">
                    <code className="rounded bg-[var(--paper)] px-2 py-1 font-mono text-[11px] text-[var(--ink-900)]">{c.url}</code>
                    {c.isDefault && (
                      <span className="ml-1.5 rounded bg-[var(--amber-200)]/50 px-1 py-0.5 text-[9px] text-[var(--amber-600)]">
                        主网卡{c.iface ? ` · ${c.iface}` : ""}
                      </span>
                    )}
                    {!c.isDefault && c.iface && <span className="ml-1.5 text-[9px] text-[var(--ink-300)]">{c.iface}</span>}
                  </span>
                  <button
                    type="button"
                    onClick={() => copy(c.url, c.url)}
                    className="shrink-0 rounded-lg border border-[var(--ink-200)] px-2 py-1 text-[10px] text-[var(--ink-700)] transition-colors hover:border-[var(--amber-500)] hover:text-[var(--amber-600)]"
                  >
                    {copied === c.url ? "已复制 ✓" : "复制"}
                  </button>
                </div>
              ))}
            </div>

            {/* 防火墙未放行 → 一键放行 */}
            {status && !status.firewall && (
              <div className="mt-2">
                <button
                  type="button"
                  onClick={() => void openFw()}
                  disabled={fwBusy}
                  className="w-full rounded-lg bg-[var(--ink-900)] px-3 py-1.5 text-[11px] font-medium text-[var(--paper)] transition-colors hover:bg-[var(--ink-700)] disabled:opacity-50"
                >
                  {fwBusy ? "等待授权…" : "一键放行防火墙（需管理员）"}
                </button>
                <p className="mt-1 text-[10px] leading-relaxed text-[var(--ink-400)]">
                  免安装版 / 之前取消了 UAC 授权时，点这里补上防火墙规则（弹一次 UAC，同意即可）。
                </p>
              </div>
            )}
            {fwMsg && (
              <p className={`mt-1.5 text-[10px] leading-relaxed ${fwMsg.ok ? "text-[var(--ink-500)]" : "text-red-500"}`}>
                {fwMsg.text}
              </p>
            )}
          </div>

          {/* 排障引导 */}
          <p className="px-1 text-[10px] leading-relaxed text-[var(--ink-400)]">
            连不上时依次确认：① 手机与电脑连<b>同一个 Wi-Fi</b>（勿用访客网络 / 路由器隔离网络）；② 电脑暂时<b>关闭 VPN 与移动热点</b>；③ 用上面标“主网卡”的地址。
          </p>

          {/* 网页端：免安装，iOS 仅此入口 */}
          <div className="rounded-xl border border-dashed border-[var(--ink-200)] bg-[var(--paper-card)] px-3.5 py-3">
            <p className="text-[11px] font-medium text-[var(--ink-700)]">
              网页端 · 免安装（iPhone / iPad 仅支持此方式）
            </p>
            <p className="mt-1 text-[10px] leading-relaxed text-[var(--ink-400)]">
              手机与电脑连同一个 Wi-Fi，用手机浏览器打开下面地址即可，无需安装任何 App。
            </p>
            <div className="mt-1.5 flex items-center gap-1.5">
              <code className="min-w-0 flex-1 truncate rounded-lg bg-[var(--paper)] px-2 py-1.5 font-mono text-xs text-[var(--ink-900)]">
                {primaryUrl || "等待局域网地址…"}
              </code>
              <button
                type="button"
                disabled={!primaryUrl}
                onClick={() => copy(primaryUrl, "web")}
                className="shrink-0 rounded-lg border border-[var(--ink-200)] px-2.5 py-1.5 text-[11px] text-[var(--ink-700)] transition-colors hover:border-[var(--amber-500)] hover:text-[var(--amber-600)] disabled:opacity-50"
              >
                {copied === "web" ? "已复制 ✓" : "复制"}
              </button>
            </div>
          </div>

          {/* 安卓端：App 下载 + 群号 */}
          <div className="rounded-xl border border-dashed border-[var(--ink-200)] bg-[var(--paper-card)] px-3.5 py-3">
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
      </div>
    </div>
  );
}
