// 流式「占位消息」的列表状态机（纯函数，便于单测）。
//
// 为什么需要它：宿主流式合成是「先出声、后落库」——音频边合成边播，Message 记录要等
// 整条流跑完才写盘。若前端只在命令返回后入列，用户就会遇到「已经听到了声音，界面上
// 却没有这条消息，因而无处可点停止」。后端因此在会话建立、即将出声那一刻发
// `tts:streaming`（载荷字段与最终 Message 一致），前端据此先插一条占位消息；
// 命令返回后再用正式消息按同一 id 原地替换。
//
// 关键不变量：**同一条流式合成在列表里永远只占一格**，且替换不能把位置换到最后
// （否则消息会在播完那一刻跳位）。事件丢失/时序倒置也不能丢消息。

import type { Message } from "../types";

/** `tts:streaming` 事件载荷（与后端 StreamingNotice 逐字段对应） */
export interface StreamingNotice {
  id: string;
  content: string;
  audio_path: string;
  engine: string;
  created_at: string;
}

/** 收到「已开始流式」通知：把占位消息插到末尾；同 id 重复通知保持幂等。 */
export function withStreamingPlaceholder(
  list: Message[],
  notice: StreamingNotice,
): Message[] {
  if (list.some((m) => m.id === notice.id)) return list;
  return [
    ...list,
    {
      id: notice.id,
      content: notice.content,
      audio_path: notice.audio_path,
      created_at: notice.created_at,
    },
  ];
}

/**
 * 合成完成：用正式消息替换同 id 的占位（保持原位置）。
 * 列表里没有该 id（事件丢失、或通知还没送达就先返回了）时追加，绝不丢消息。
 */
export function settleStreaming(list: Message[], message: Message): Message[] {
  const idx = list.findIndex((m) => m.id === message.id);
  if (idx < 0) return [...list, message];
  const next = list.slice();
  next[idx] = message;
  return next;
}

/** 合成失败/未产出的占位回收：只按 id 摘除，不动其它条目。 */
export function dropStreaming(list: Message[], id: string): Message[] {
  return list.filter((m) => m.id !== id);
}

/** 某条消息是否仍是占位（正在合成中，音频文件尚未落盘完成） */
export function isStreamingPlaceholder(
  streamingIds: string[],
  message: Message,
): boolean {
  return streamingIds.includes(message.id);
}
