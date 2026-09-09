// 极简 Markdown 渲染（零依赖）：只支持发布说明实际用到的语法，避免为此引入
// 一整条 remark/rehype 依赖链，也让深浅色样式继续吃「安墨」token。
// 支持：## / ### / #### 标题、- 与 * 无序列表（两级）、> 引用、--- 分隔线、
//       **加粗**、`行内代码`、[文本](链接)（走系统浏览器，绝不在 webview 内导航）。

import { useMemo, type ReactNode } from "react";
import { openUrl } from "@tauri-apps/plugin-opener";

/** 行内标记：加粗 | 行内代码 | 链接 */
const INLINE_RE = /(\*\*[^*]+\*\*|`[^`]+`|\[[^\]]+\]\([^)\s]+\))/g;

function renderInline(text: string, keyBase: string): ReactNode[] {
  const out: ReactNode[] = [];
  let last = 0;
  let i = 0;
  for (const m of text.matchAll(INLINE_RE)) {
    const idx = m.index ?? 0;
    if (idx > last) out.push(text.slice(last, idx));
    const tok = m[0];
    const key = `${keyBase}-${i++}`;
    if (tok.startsWith("**")) {
      out.push(
        <strong key={key} className="font-medium text-[var(--ink-900)]">
          {tok.slice(2, -2)}
        </strong>,
      );
    } else if (tok.startsWith("`")) {
      out.push(
        <code
          key={key}
          className="rounded bg-[var(--ink-100)] px-1 py-px font-mono text-[0.92em] text-[var(--ink-700)]"
        >
          {tok.slice(1, -1)}
        </code>,
      );
    } else {
      const link = /^\[([^\]]+)\]\(([^)\s]+)\)$/.exec(tok);
      if (link) {
        out.push(
          <button
            key={key}
            type="button"
            onClick={() => {
              void openUrl(link[2]).catch(() => {});
            }}
            className="text-[var(--amber-600)] underline underline-offset-2 transition-colors hover:text-[var(--amber-700)]"
          >
            {link[1]}
          </button>,
        );
      } else {
        out.push(tok);
      }
    }
    last = idx + tok.length;
  }
  if (last < text.length) out.push(text.slice(last));
  return out;
}

type Block =
  | { kind: "h"; level: number; text: string }
  | { kind: "p"; text: string }
  | { kind: "quote"; text: string }
  | { kind: "ul"; items: { text: string; sub: boolean }[] };

/** 按行切块；列表项连续归组，两空格以上缩进算子项 */
function parseBlocks(src: string): Block[] {
  const blocks: Block[] = [];
  let list: { text: string; sub: boolean }[] | null = null;
  const flushList = () => {
    if (list) {
      blocks.push({ kind: "ul", items: list });
      list = null;
    }
  };

  for (const raw of src.split(/\r?\n/)) {
    const line = raw.trimEnd();
    if (!line.trim()) {
      flushList();
      continue;
    }
    if (/^\s*[-*_]\s*[-*_]\s*[-*_][\s-*]*$/.test(line)) {
      flushList();
      continue; // 分隔线：轻量渲染下直接忽略，段落间距已够分隔感
    }
    const h = /^(#{1,6})\s+(.*)$/.exec(line);
    if (h) {
      flushList();
      blocks.push({ kind: "h", level: Math.min(h[1].length, 4), text: h[2] });
      continue;
    }
    const q = /^>\s?(.*)$/.exec(line);
    if (q) {
      flushList();
      blocks.push({ kind: "quote", text: q[1] });
      continue;
    }
    const li = /^(\s*)[-*+]\s+(.*)$/.exec(line);
    if (li) {
      if (!list) list = [];
      list.push({ text: li[2], sub: li[1].length >= 2 });
      continue;
    }
    flushList();
    blocks.push({ kind: "p", text: line.trim() });
  }
  flushList();
  return blocks;
}

const HEAD_CLS: Record<number, string> = {
  1: "font-display text-sm font-medium text-[var(--ink-900)]",
  2: "font-display text-[13px] font-medium text-[var(--ink-900)]",
  3: "text-xs font-medium text-[var(--ink-700)]",
  4: "text-xs font-medium text-[var(--ink-600)]",
};

/** 渲染一段 markdown 文本（Release 说明等） */
export function MarkdownLite({ text, className = "" }: { text: string; className?: string }) {
  const blocks = useMemo(() => parseBlocks(text ?? ""), [text]);

  return (
    <div className={`space-y-2 text-xs leading-relaxed text-[var(--ink-500)] ${className}`}>
      {blocks.map((b, i) => {
        switch (b.kind) {
          case "h":
            return (
              <div key={i} className={`${HEAD_CLS[b.level] ?? HEAD_CLS[4]} ${i > 0 ? "pt-1" : ""}`}>
                {renderInline(b.text, `h${i}`)}
              </div>
            );
          case "quote":
            return (
              <div
                key={i}
                className="rounded-r-lg border-l-2 border-[var(--amber-500)] bg-[var(--ink-100)]/50 px-2.5 py-1.5 text-[var(--ink-600)]"
              >
                {renderInline(b.text, `q${i}`)}
              </div>
            );
          case "ul":
            return (
              <ul key={i} className="space-y-1">
                {b.items.map((it, j) => (
                  <li key={j} className={it.sub ? "ml-4 flex gap-1.5" : "flex gap-1.5"}>
                    <span
                      className={
                        it.sub
                          ? "mt-[7px] h-[3px] w-[3px] shrink-0 rounded-full bg-[var(--ink-300)]"
                          : "mt-[7px] h-[5px] w-[5px] shrink-0 rounded-full bg-[var(--amber-500)]"
                      }
                    />
                    <span className="min-w-0 flex-1">{renderInline(it.text, `l${i}-${j}`)}</span>
                  </li>
                ))}
              </ul>
            );
          default:
            return <div key={i}>{renderInline(b.text, `p${i}`)}</div>;
        }
      })}
    </div>
  );
}
