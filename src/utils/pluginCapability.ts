// 插件能力位判定（基座合同：引擎差异只能以插件自述的能力位存在，不许按 id 写死）。

/** 插件自述的能力位；字段来自 manifest.json 的 capabilities，缺省一律按「不具备」处理 */
export type PluginCapabilitiesLike = { capabilities?: { voice_library?: boolean } | null };

/** 是否有云端音色库（用户可在「我的音色」里保存并选用自己的音色 ID） */
export function hasVoiceLibrary(plugin: PluginCapabilitiesLike): boolean {
  return plugin.capabilities?.voice_library === true;
}
