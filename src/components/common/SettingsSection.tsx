// 通用设置呈现原语：可折叠分类（Section）与分类内单项（Field）。
// 从设置页抽出，供「设置页」与「语音中心」共用，保持「安墨」观感一致。

import { useState, useEffect } from "react";

/** 可折叠的分类（手风琴）。收起时不渲染 children → 减少渲染、消除卡顿。 */
export function Section({
  title,
  defaultOpen = false,
  onOpen,
  children,
}: {
  title: React.ReactNode;
  defaultOpen?: boolean;
  /** 展开时回调（如"关于"展开后清红点） */
  onOpen?: () => void;
  children: React.ReactNode;
}) {
  const [open, setOpen] = useState(defaultOpen);
  return (
    <section className="rounded-xl border border-[var(--ink-200)] bg-[var(--paper-card)]">
      <button
        onClick={() => {
          const next = !open;
          setOpen(next);
          if (next) onOpen?.();
        }}
        className="flex w-full items-center justify-between px-4 py-3 text-left"
      >
        <span className="text-sm font-medium text-[var(--ink-900)]">{title}</span>
        <span className="text-[var(--ink-300)] transition-transform">{open ? "▾" : "▸"}</span>
      </button>
      {open && <div className="space-y-4 px-4 pb-4">{children}</div>}
    </section>
  );
}

/** 分类内的一个设置项（标签 + 内容） */
export function Field({ label, children }: { label: string; children: React.ReactNode }) {
  return (
    <div>
      <h3 className="mb-2 text-[10px] font-medium uppercase tracking-[0.25em] text-[var(--ink-300)]">{label}</h3>
      {children}
    </div>
  );
}

/** 子区块标题：琥珀竖条 + 衬线标题（与 HotkeyRow / 插件页分类头部同款），
 *  用于「音色克隆」「音色管理」等需强存在的分组，比 Field 微标签更醒目。 */
export function SectionHeading({
  title,
  desc,
  right,
}: {
  title: React.ReactNode;
  desc?: React.ReactNode;
  /** 右侧动作区（如「刷新账号音色」按钮） */
  right?: React.ReactNode;
}) {
  return (
    <div className="flex items-start justify-between gap-3">
      <div className="flex min-w-0 items-start gap-2">
        <span className="mt-[3px] h-4 w-[3px] shrink-0 rounded-full bg-[var(--amber-500)]" aria-hidden />
        <div className="min-w-0">
          <h3 className="font-display text-sm font-semibold tracking-wide text-[var(--ink-900)]">{title}</h3>
          {desc && <p className="mt-0.5 text-[11px] leading-relaxed text-[var(--ink-300)]">{desc}</p>}
        </div>
      </div>
      {right && <div className="shrink-0">{right}</div>}
    </div>
  );
}

/** 分区卡：轻底 tint 容器 + SectionHeading 头部，用于引擎卡内「API 密钥 / 音色管理 / 音色克隆」等分组。 */
export function SubPanel({
  title,
  desc,
  right,
  children,
}: {
  title: React.ReactNode;
  desc?: React.ReactNode;
  right?: React.ReactNode;
  children?: React.ReactNode;
}) {
  return (
    <div className="rounded-xl border border-[var(--ink-200)]/70 bg-[var(--ink-100)]/25 px-3.5 py-3">
      <SectionHeading title={title} desc={desc} right={right} />
      {children && <div className="mt-2.5">{children}</div>}
    </div>
  );
}

/** 密钥输入框：密码 + 👁 显隐切换，失焦提交。样式与插件配置卡（PluginConfigPanel）一致，
 *  供内置引擎（MiMo / MOSS）填写 API Key，与插件引擎的密钥填写观感统一。 */
export function SecretInput({
  value,
  onCommit,
  placeholder,
}: {
  /** 当前已保存值（外部为单一数据源） */
  value: string;
  /** 失焦且有改动时提交最终值 */
  onCommit: (v: string) => void;
  placeholder?: string;
}) {
  const [show, setShow] = useState(false);
  const [draft, setDraft] = useState(value);
  // 外部值变化（设置异步加载完成 / 切换引擎）时同步草稿
  useEffect(() => setDraft(value), [value]);
  return (
    <div className="flex items-center gap-1.5">
      <input
        type={show ? "text" : "password"}
        value={draft}
        onChange={(e) => setDraft(e.target.value)}
        onBlur={() => {
          if (draft !== value) onCommit(draft);
        }}
        placeholder={placeholder}
        className="w-full rounded-xl border border-[var(--ink-200)] bg-[var(--paper-card)] px-3 py-2 text-sm outline-none transition-colors placeholder:text-[var(--ink-300)] focus:border-[var(--amber-500)]"
      />
      <button
        type="button"
        onMouseDown={(e) => e.preventDefault()}
        onClick={() => setShow((s) => !s)}
        title={show ? "隐藏" : "显示"}
        className="shrink-0 rounded-lg border border-[var(--ink-200)] px-2 py-1 text-[11px] text-[var(--ink-500)] transition-colors hover:border-[var(--ink-300)]"
      >
        {show ? "🙈" : "👁"}
      </button>
    </div>
  );
}
