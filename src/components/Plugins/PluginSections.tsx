// 可安装候选条目行 + 分类区块（模块级组件，从 PluginPage 抽出以修复整页 remount）。

import { PluginCard } from "./PluginCard";
import type { Candidate, PluginInventory } from "../../hooks/usePluginInventory";
import type { PluginActions } from "../../hooks/usePluginActions";
import type { PluginInfo } from "../../types";

/** 可安装条目（内置/在线合并后的统一行） */
export function CandidateRow({
  c,
  disabled,
  onInstall,
}: {
  c: Candidate;
  disabled?: boolean;
  onInstall: (c: Candidate) => void;
}) {
  return (
    <div className="flex items-center gap-3 rounded-xl border border-[var(--ink-200)] bg-[var(--paper-card)] px-4 py-3">
      <div className="min-w-0 flex-1">
        <div className="flex flex-wrap items-center gap-2">
          <span className="text-sm font-medium text-[var(--ink-900)]">{c.name}</span>
          <span className="rounded-md bg-[var(--ink-100)] px-1.5 py-0.5 font-mono text-[10px] text-[var(--ink-500)]">
            v{c.version}
          </span>
          <span
            className="rounded-md bg-[var(--ink-100)]/60 px-1.5 py-0.5 text-[10px] text-[var(--ink-300)]"
            title={c.source === "bundled" ? "随安装包携带，无需联网即可安装" : "从官方渠道在线下载安装"}
          >
            {c.source === "bundled" ? "内置安装包" : "在线"}
          </span>
        </div>
        {c.description && (
          <p
            className="mt-1 truncate text-[11px] text-[var(--ink-500)]"
            title={c.description}
          >
            {c.description}
          </p>
        )}
        {c.requirements && (
          <p className="mt-1 text-[10px] leading-relaxed text-[var(--ink-300)]">
            资源需求：{c.requirements}
          </p>
        )}
      </div>
      <button
        onClick={() => onInstall(c)}
        disabled={disabled}
        className="shrink-0 rounded-lg bg-[var(--amber-500)] px-3 py-1.5 text-[11px] font-medium text-[var(--paper)] transition-opacity hover:opacity-90 disabled:opacity-40"
      >
        安装
      </button>
    </div>
  );
}

/** 分类区块：标题 + 说明 + 已装卡片 + 可装条目 */
export function CategorySection({
  inv,
  title,
  subtitle,
  installed,
  candidates,
  emptyHint,
  actions,
  busy,
  envPickId,
  setEnvPickId,
}: {
  inv: PluginInventory;
  title: string;
  subtitle: string;
  installed: PluginInfo[];
  candidates: Candidate[];
  emptyHint: string;
  actions: PluginActions;
  busy: string | null;
  envPickId: string | null;
  setEnvPickId: (id: string | null) => void;
}) {
  return (
    <section>
      {/* 分类头部：固定两行结构（标题行 + 描述行），两个分类格式统一 */}
      <div className="mb-3">
        <div className="flex items-center gap-2">
          <span className="h-3.5 w-[3px] shrink-0 rounded-full bg-[var(--amber-500)]" aria-hidden />
          <h2 className="font-display text-sm font-semibold tracking-wide text-[var(--ink-900)]">
            {title}
          </h2>
          <span className="rounded-md bg-[var(--ink-100)] px-1.5 py-0.5 text-[10px] text-[var(--ink-500)]">
            已装 {installed.length}
          </span>
        </div>
        <p className="mt-1 pl-[11px] text-[11px] leading-relaxed text-[var(--ink-300)]">
          {subtitle}
        </p>
      </div>

      {installed.length === 0 && candidates.length === 0 && !inv.loading && !inv.error && (
        <div className="rounded-xl border border-dashed border-[var(--ink-200)] px-4 py-6 text-center text-xs text-[var(--ink-300)]">
          {emptyHint}
        </div>
      )}

      <div className="space-y-3">
        {installed.map((p) => (
          <PluginCard
            key={p.id}
            p={p}
            online={inv.onlineEntryOf(p.id)}
            actions={actions}
            busy={busy}
            envPickId={envPickId}
            setEnvPickId={setEnvPickId}
            onRefresh={() => void inv.reload()}
          />
        ))}
        {candidates.map((c) => (
          <CandidateRow
            key={`${c.source}-${c.id}`}
            c={c}
            disabled={busy !== null}
            onInstall={(x) => void actions.installCandidate(x)}
          />
        ))}
      </div>
    </section>
  );
}
