// 发送键点击决策（纯函数）。见 sendAction.test.ts 的行为契约。
//
// 需求：合成期间不阻塞输入，发送键置灰但仍可点——
// 第一次点 = 只 arm（轻提示"再点一次打断"），不打断当前合成；
// 第二次点 = interrupt（强制打断当前合成并改发最新一条）；
// 空闲 = 直接 send。

export type SendClickAction = "noop" | "send" | "arm" | "interrupt";

export function decideSendClick(args: {
  busy: boolean;
  armed: boolean;
  hasText: boolean;
}): SendClickAction {
  const { busy, armed, hasText } = args;
  if (!hasText) return "noop";
  if (!busy) return "send";
  return armed ? "interrupt" : "arm";
}
