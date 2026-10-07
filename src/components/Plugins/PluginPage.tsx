// 插件库（双页签）：已安装（默认，管理启用/更新/卸载）｜插件商店（进入即自动获取在线列表）。
// 两视图共用子分类（全部/语音合成/语音识别/扩展）与搜索框；内容按类型分组成阵列网格。
// 安全：在线安装 zip SHA-256 对照官方索引；拖入安装 zip 校验完整性，来源可信度由确认弹窗把关。

import { useEffect, useMemo, useState } from "react";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { usePluginInventory, catOf, supportsClone, type PluginCat, type StoreItem } from "../../hooks/usePluginInventory";
import { usePluginActions } from "../../hooks/usePluginActions";
import { InstalledCard } from "./InstalledCard";
import { StoreCard } from "./StoreCard";
import { ToastHost } from "../common/Toast";

type MainTab = "inst" | "store";
type Sub = "" | "cloud" | "local" | "clone";
const CATS: { id: "all" | PluginCat; label: string }[] = [
  { id: "all", label: "全部" },
  { id: "tts", label: "语音合成" },
  { id: "asr", label: "语音识别" },
  { id: "ext", label: "扩展" },
];
const CAT_LABEL: Record<PluginCat, string> = { tts: "语音合成", asr: "语音识别", ext: "扩展" };
/** 二级子分类：TTS 多一项「支持克隆」（克隆与云/本地交叉，作为独立视图） */
const SUBS: Partial<Record<PluginCat, { id: Exclude<Sub, "">; label: string }[]>> = {
  tts: [
    { id: "cloud", label: "云端" },
    { id: "local", label: "本地" },
    { id: "clone", label: "支持克隆" },
  ],
  asr: [
    { id: "cloud", label: "云端" },
    { id: "local", label: "本地" },
  ],
};

/** 搜索匹配：名称 / 描述 / id（大小写不敏感） */
function matches(p: { id: string; name: string; description: string }, q: string): boolean {
  if (!q) return true;
  const s = q.toLowerCase();
  return p.name.toLowerCase().includes(s) || p.description.toLowerCase().includes(s) || p.id.toLowerCase().includes(s);
}

export function PluginPage() {
  const inv = usePluginInventory();
  const actions = usePluginActions(inv.reload, inv.reloadBundled);
  const { busy } = actions;

  const [mainTab, setMainTab] = useState<MainTab>("inst");
  const [cat, setCat] = useState<"all" | PluginCat>("all");
  const [sub, setSub] = useState<Sub>("");
  const [q, setQ] = useState("");
  // 拖入 zip 时的浮层开关
  const [dragOver, setDragOver] = useState(false);
  // 正在展示「安装方式二选一」面板的插件 id（在线下载 / 离线导入）
  const [envPickId, setEnvPickId] = useState<string | null>(null);

  // 二级子分类过滤：云端/本地按引擎类别，支持克隆按宿主克隆链路绑定
  const subOk = (item: { id: string; category?: string }): boolean => {
    if (!sub) return true;
    if (sub === "clone") return supportsClone(item.id);
    if (sub === "local") return item.category === "local";
    return item.category !== "local";
  };

  // 商店惰性联网：首次进入商店页签时自动获取在线索引（应用启动不联网）
  useEffect(() => {
    if (mainTab === "store" && inv.index === null && !inv.indexLoading && !inv.indexError) {
      void inv.reloadIndex();
    }
  }, [mainTab, inv.index, inv.indexLoading, inv.indexError, inv.reloadIndex]);

  // 拖入安装：监听窗口拖放事件（dropInstall 为稳定引用，不随数据刷新重订阅）
  useEffect(() => {
    let unlisten: (() => void) | null = null;
    let cancelled = false;
    (async () => {
      const u = await getCurrentWindow().onDragDropEvent(async (event) => {
        const payload = event.payload;
        if (payload.type === "enter") {
          if (payload.paths.some((p) => p.toLowerCase().endsWith(".zip"))) setDragOver(true);
        } else if (payload.type === "leave") {
          setDragOver(false);
        } else if (payload.type === "drop") {
          setDragOver(false);
          const zip = payload.paths.find((p) => p.toLowerCase().endsWith(".zip"));
          if (zip) await actions.dropInstall(zip);
        }
      });
      if (cancelled) {
        u();
      } else {
        unlisten = u;
      }
    })();
    return () => {
      cancelled = true;
      unlisten?.();
    };
  }, [actions.dropInstall]);

  // 当前视图的分组渲染数据：分类过滤 + 二级子分类 + 搜索
  const cats: PluginCat[] = cat === "all" ? ["tts", "asr", "ext"] : [cat];

  const instSections = useMemo(
    () =>
      cats.map((c) => ({
        cat: c,
        items: inv.installedGroups[c].filter((p) => matches(p, q) && subOk(p)),
      })),
    // eslint-disable-next-line react-hooks/exhaustive-deps
    [inv.installedGroups, cat, sub, q],
  );
  const storeSections = useMemo(
    () =>
      cats.map((c) => ({
        cat: c,
        items: inv.storeGroups[c].filter((it) => matches(it, q) && subOk(it)),
      })),
    // eslint-disable-next-line react-hooks/exhaustive-deps
    [inv.storeGroups, cat, sub, q],
  );
  const instVisible = instSections.reduce((n, s) => n + s.items.length, 0);
  const storeVisible = storeSections.reduce((n, s) => n + s.items.length, 0);

  return (
    <div className="relative flex h-full flex-col">
      <div className="scrollbar-thin flex-1 overflow-y-auto px-4 pb-8 pt-4">
        {/* 页头：全局计数（无「获取在线列表」按钮——联网由商店页签进入时自动完成） */}
        <div className="flex flex-wrap items-baseline gap-2">
          <h1 className="font-display text-base text-[var(--ink-900)]">插件库</h1>
          <span className="text-[11px] text-[var(--ink-300)]">
            已安装页管理启用与卸载 · 商店页浏览与安装；引擎切换、音色与效果器配置在「语音」
          </span>
          <span className="ml-auto inline-flex items-center gap-3">
            <span className="rounded-full border border-[var(--ink-200)] bg-[var(--paper-card)] px-2.5 py-0.5 text-[10.5px] text-[var(--ink-500)]">
              已装 <b className="text-[var(--ink-900)]">{inv.plugins.length}</b>
            </span>
            {inv.updateCount > 0 && (
              <button
                onClick={() => {
                  setMainTab("inst");
                  setCat("all");
                }}
                className="rounded-full border border-[var(--amber-500)]/45 bg-[var(--paper-card)] px-2.5 py-0.5 text-[10.5px] text-[var(--amber-600)] transition-colors hover:bg-[var(--amber-200)]/25"
                title="去已安装页查看可更新项"
              >
                可更新 <b>{inv.updateCount}</b>
              </button>
            )}
          </span>
        </div>

        {/* 主页签 */}
        <div className="mt-3 flex gap-5 border-b border-[var(--ink-200)]">
          {(
            [
              { id: "inst" as MainTab, label: "已安装", n: inv.plugins.length },
              { id: "store" as MainTab, label: "插件商店", n: inv.storeGroups.tts.length + inv.storeGroups.asr.length + inv.storeGroups.ext.length },
            ]
          ).map((t) => (
            <button
              key={t.id}
              onClick={() => setMainTab(t.id)}
              className={[
                "-mb-px inline-flex items-center gap-1.5 border-b-2 pb-2 font-display text-[13px] transition-colors",
                mainTab === t.id
                  ? "border-[var(--amber-500)] font-semibold text-[var(--ink-900)]"
                  : "border-transparent text-[var(--ink-300)] hover:text-[var(--ink-700)]",
              ].join(" ")}
            >
              {t.label}
              <span
                className={[
                  "rounded-md px-1.5 py-px font-mono text-[10px]",
                  mainTab === t.id
                    ? "bg-[var(--amber-200)]/60 text-[var(--amber-600)]"
                    : "bg-[var(--ink-100)] text-[var(--ink-500)]",
                ].join(" ")}
              >
                {t.n}
              </span>
            </button>
          ))}
        </div>

        {/* 工具行：子分类 + 搜索（两视图共用）；选中语音合成/语音识别时出现二级子分类 */}
        <div className="flex flex-wrap items-center gap-1.5 py-3">
          {CATS.map((c) => (
            <button
              key={c.id}
              onClick={() => {
                setCat(c.id);
                setSub("");
              }}
              className={[
                "rounded-full border px-3 py-1 text-[11px] transition-colors",
                cat === c.id
                  ? "border-[var(--amber-500)]/55 bg-[var(--amber-200)]/50 font-medium text-[var(--amber-600)]"
                  : "border-[var(--ink-200)] bg-[var(--paper-card)] text-[var(--ink-500)] hover:text-[var(--ink-700)]",
              ].join(" ")}
            >
              {c.label}
            </button>
          ))}
          <label className="ml-auto flex min-w-[150px] items-center gap-1.5 rounded-full border border-[var(--ink-200)] bg-[var(--paper-card)] px-3 py-1 transition-colors focus-within:border-[var(--amber-500)]">
            <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2.2" strokeLinecap="round" className="shrink-0 text-[var(--ink-300)]" aria-hidden>
              <circle cx="11" cy="11" r="7" />
              <path d="m20 20-3.5-3.5" />
            </svg>
            <input
              value={q}
              onChange={(e) => setQ(e.target.value)}
              placeholder="搜索插件名称 / 描述…"
              className="w-full border-none bg-transparent text-[11.5px] text-[var(--ink-900)] outline-none placeholder:text-[var(--ink-300)]"
            />
          </label>
        </div>
        {(cat === "tts" || cat === "asr") && (
          <div className="-mt-1.5 flex flex-wrap items-center gap-1.5 pb-3">
            <span className="text-[10px] text-[var(--ink-300)]">筛选</span>
            {(SUBS[cat] ?? []).map((s) => (
              <button
                key={s.id}
                onClick={() => setSub(sub === s.id ? "" : s.id)}
                className={[
                  "rounded-full border px-2.5 py-0.5 text-[10.5px] transition-colors",
                  sub === s.id
                    ? "border-[var(--amber-500)] bg-[var(--amber-500)] font-medium text-[var(--paper)]"
                    : "border-[var(--ink-200)] bg-transparent text-[var(--ink-500)] hover:text-[var(--ink-700)]",
                ].join(" ")}
              >
                {s.label}
              </button>
            ))}
          </div>
        )}

        {/* ══ 已安装视图 ══ */}
        {mainTab === "inst" && (
          <>
            {inv.loading && <div className="py-6 text-center text-sm text-[var(--ink-300)]">加载中…</div>}
            {!inv.loading && inv.error && (
              <div className="rounded-lg border border-[var(--seal)]/30 bg-[var(--seal)]/5 px-3 py-2 text-xs text-[var(--seal)]">
                读取插件列表失败：{inv.error}
              </div>
            )}
            {!inv.loading && !inv.error && instVisible === 0 && (
              <div className="mt-6 rounded-xl border border-dashed border-[var(--ink-200)] px-4 py-8 text-center text-xs text-[var(--ink-300)]">
                {q
                  ? `没有找到与「${q}」相关的已安装插件`
                  : "此分类下暂无已安装插件 · 切到「插件商店」看看有什么可装的"}
              </div>
            )}
            {instSections.map((sec) =>
              sec.items.length === 0 ? null : (
                <section key={sec.cat} className="mb-5">
                  {cat === "all" && (
                    <div className="mb-2 flex items-baseline gap-2">
                      <span className="h-3.5 w-[3px] rounded-full bg-[var(--amber-500)]" aria-hidden />
                      <h2 className="font-display text-[13px] font-semibold text-[var(--ink-900)]">{CAT_LABEL[sec.cat]}</h2>
                      <span className="ml-auto text-[10.5px] text-[var(--ink-300)]">{sec.items.length}</span>
                    </div>
                  )}
                  <div className="grid grid-cols-[repeat(auto-fill,minmax(228px,1fr))] gap-2.5">
                    {sec.items.map((p) => (
                      <InstalledCard
                        key={p.id}
                        p={p}
                        cat={catOf(inv.typeOf(p))}
                        online={inv.index?.find((o) => o.id === p.id)}
                        actions={actions}
                        busy={busy}
                        envPickId={envPickId}
                        setEnvPickId={setEnvPickId}
                        onRefresh={() => void inv.reload()}
                      />
                    ))}
                  </div>
                </section>
              ),
            )}
          </>
        )}

        {/* ══ 插件商店视图 ══ */}
        {mainTab === "store" && (
          <>
            {inv.indexLoading && (
              <div className="flex items-center gap-2 py-2 text-[11px] text-[var(--ink-300)]">
                <span className="h-3 w-3 animate-spin rounded-full border-[1.5px] border-[var(--ink-200)] border-t-[var(--amber-500)]" />
                正在获取在线插件列表…
              </div>
            )}
            {!inv.indexLoading && inv.indexError && (
              <div className="mb-3 rounded-lg border border-[var(--ink-200)] bg-[var(--ink-100)]/40 px-3 py-2.5 text-xs leading-relaxed text-[var(--ink-500)]">
                无法获取在线插件列表：{inv.indexError}
                <button
                  onClick={() => void inv.reloadIndex()}
                  className="ml-2 text-[var(--amber-600)] underline underline-offset-2"
                >
                  重试
                </button>
                <span className="ml-2 text-[var(--ink-300)]">（离线时仍可安装内置插件）</span>
              </div>
            )}
            {!inv.indexLoading && !inv.indexError && storeVisible === 0 && (
              <div className="mt-6 rounded-xl border border-dashed border-[var(--ink-200)] px-4 py-8 text-center text-xs text-[var(--ink-300)]">
                {q ? `没有找到与「${q}」相关的插件 · 换个关键词试试` : "此分类下暂无可获取的插件"}
              </div>
            )}
            {storeSections.map((sec) =>
              sec.items.length === 0 ? null : (
                <section key={sec.cat} className="mb-5">
                  {cat === "all" && (
                    <div className="mb-2 flex items-baseline gap-2">
                      <span className="h-3.5 w-[3px] rounded-full bg-[var(--amber-500)]" aria-hidden />
                      <h2 className="font-display text-[13px] font-semibold text-[var(--ink-900)]">{CAT_LABEL[sec.cat]}</h2>
                      <span className="ml-auto text-[10.5px] text-[var(--ink-300)]">{sec.items.length}</span>
                    </div>
                  )}
                  <div className="grid grid-cols-[repeat(auto-fill,minmax(228px,1fr))] gap-2.5">
                    {sec.items.map((it: StoreItem) => (
                      <StoreCard
                        key={`${it.source}-${it.id}`}
                        item={it}
                        busy={busy}
                        onInstall={(x) => void actions.installStoreItem(x)}
                      />
                    ))}
                  </div>
                </section>
              ),
            )}
            <p className="pt-1 text-center text-[10.5px] text-[var(--ink-300)]">
              ── 更多插件持续上架 · 将插件 zip 拖入窗口可离线安装 ──
            </p>
          </>
        )}
      </div>

      {/* 拖入提示浮层 */}
      {dragOver && (
        <div className="pointer-events-none absolute inset-2 z-10 flex items-center justify-center rounded-2xl border-2 border-dashed border-[var(--amber-500)] bg-[var(--amber-200)]/20">
          <span className="rounded-xl bg-[var(--paper-card)] px-4 py-2 text-sm font-medium text-[var(--amber-600)] shadow-sm">
            松开以安装插件 zip
          </span>
        </div>
      )}

      {/* 操作中提示条 */}
      {busy && (
        <div className="border-t border-[var(--ink-200)] bg-[var(--paper-card)] px-4 py-2 text-xs text-[var(--amber-600)] animate-fade">
          {busy}
        </div>
      )}

      <ToastHost />
    </div>
  );
}
