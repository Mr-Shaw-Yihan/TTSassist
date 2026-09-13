// 加速键（快捷键）串构建工具：把键盘事件转成 "Ctrl+Alt+V" 这类字符串。
// HotkeyRecorder（浮窗快捷键）与 HotkeyCapture（收藏快捷键）共用。

/** 把键盘事件的特殊键名映射成加速键格式 */
export function mapKey(key: string): string {
  // 空格必须在单字符分支前判定（key.length===1 会提前 return，原来的 case " " 是死分支）
  if (key === " ") return "Space";
  if (key.length === 1) return key.toUpperCase(); // 单字母转大写
  switch (key) {
    case "ArrowUp": return "Up";
    case "ArrowDown": return "Down";
    case "ArrowLeft": return "Left";
    case "ArrowRight": return "Right";
    case "Escape": return "Escape";
    case "Enter": return "Enter";
    case "Tab": return "Tab";
    case "Backspace": return "Backspace";
    case "Delete": return "Delete";
    default: return key; // F1-F12 等保持原样
  }
}

/** 从键盘事件构建加速键串，如 "Ctrl+Alt+V"。仅按了修饰键时返回 null。 */
export function buildAccelerator(e: {
  key: string;
  ctrlKey: boolean;
  altKey: boolean;
  shiftKey: boolean;
  metaKey: boolean;
}): string | null {
  const key = e.key;
  // 仅按下修饰键（还没按主键）→ 继续等待
  if (["Control", "Alt", "Shift", "Meta"].includes(key)) return null;
  // `+` 是加速键分隔符，且 .cargo 里 global-hotkey 的 parse_key 没有 Plus 键码
  // （第二轮任务书 T-C 裁决）：绑出来必然是 "Ctrl++" 这种必失败串，前端直接拒绑。
  // 含 Shift+= 组合产出的 "+"。
  if (key === "+") return null;
  const parts: string[] = [];
  if (e.ctrlKey) parts.push("Ctrl");
  if (e.altKey) parts.push("Alt");
  if (e.shiftKey) parts.push("Shift");
  if (e.metaKey) parts.push("Meta");
  parts.push(mapKey(key));
  return parts.join("+");
}