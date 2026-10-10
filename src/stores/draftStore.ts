// 输入区草稿 store：让未发送的文字跨标签页保留（只活在本次会话）。
// 为什么必须在 store 里：App.tsx 用 `{tab === "messages" && …}` 条件渲染，切到语音/设置
// 再切回来时 InputBox 会被卸载，组件内的 useState 归零 —— 用户表现为「切页面文字就没了」。
// 刻意不落磁盘：重启后还留着上次没发的内容，容易被误当本次要朗读的文本直接发出去。

import { create } from "zustand";

interface DraftState {
  draft: string;
  /** 支持直接赋值或函数式更新（与 useState 的 setter 用法对齐，调用方无需改写） */
  setDraft: (next: string | ((prev: string) => string)) => void;
}

export const useDraftStore = create<DraftState>((set) => ({
  draft: "",
  setDraft: (next) =>
    set((s) => ({ draft: typeof next === "function" ? next(s.draft) : next })),
}));
