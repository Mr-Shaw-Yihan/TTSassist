// 受控 <select> 的「当前选中值」兜底推导（纯函数，与 React/DOM 解耦，便于单测）。
//
// 要解决的实测问题：下拉的 value 若在现有选项里找不到匹配（存的是空串、脏 id，
// 或库里只剩一项而从没选过），React 会把 selectedIndex 置成 -1，此时用户点那唯一
// 一项往往**不会派发 change**——表现成「只有一个音色时选不上、加到两个才正常」。
// 合成侧读到的还是空值，于是报「未配置音色 id」。
//
// 修法不依赖浏览器细节：统一给出「存的值合法就用它，否则用第一项」的生效值，
// 调用方既用它渲染，也在两者不一致时回写一次，保证下拉显示的与后端读到的永远是同一个。
// 本函数不认识任何引擎（基座合同 §3.6-1：引擎差异不得写进本体）。

/**
 * 解析生效的选中值。
 * @param stored  设置里存的 id（可能为 null/undefined/空串/已被删除的脏值）
 * @param options 当前可用选项的 id 列表（顺序即下拉顺序，空/纯空白的项会被剔除）
 * @returns stored 命中选项时返回 trim 后的 stored；否则返回第一个有效选项；无可用选项时返回 ""
 */
export function resolveSelectedValue(
  stored: string | null | undefined,
  options: (string | null | undefined)[],
): string {
  const list = options.map((v) => (v ?? "").trim()).filter((v) => v !== "");
  if (list.length === 0) return "";
  const s = (stored ?? "").trim();
  return list.includes(s) ? s : list[0];
}
