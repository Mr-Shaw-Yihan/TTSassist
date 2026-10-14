# MOSS 内置引擎 → 预装插件 `moss-tts` 设计（P2.8）

> 建立：2026-10-14。承接发布看板「🆕 二·补」。目标并入 1.9.0。
> 执行前须用户确认本设计；确认后再用 writing-plans 出逐任务实施计划。

## 一、目标与非目标

**目标**
- 把 MOSS 从宿主**内置引擎**改造成标准插件 `moss-tts`，与 **mimo-tts / minimax-tts-stream 一样随安装包"内置预装"**（`resources/plugins` 离线即装）。
- 架构上本体**不再有内置 TTS 引擎**（edge/mimo/moss/… 全部是插件）。
- 老用户升级：**装好即用不变**，`tts_engine="moss"` 及其 API Key / 音色**自动迁移**到插件、不丢。

**非目标**
- 不改分发口径的本质：mimo/moss/minimax = 预装；只有 **fish** 走"仅在线索引"。
- 不改默认引擎：新装机默认仍是 `edge-tts`（本就是插件），本项**不动默认值**。
- 不做 MOSS 流式化（保持现状非流式）；不引入新能力。

## 二、关键事实（调研所得，非假设）

- `src-tauri/src/tts/moss.rs`（124 行）：`MossEngine{api_key,voice_id,content_dir}`，`POST https://api.mosi.cn/v1/audio/speech`，body `model:"moss-tts"`+`voice_id`，返回 mp3 url→落盘。**非流式**。key/voice 为空时直接报错。
- 默认引擎：`storage/types.rs:186` `tts_engine: "edge-tts"`。**与 moss 无关**，移除内置 moss 不影响新装机默认。
- 预装机制：`plugins/manager.rs:119 bootstrap_bundled_zips` 启动扫描 `<exe>/resources/plugins` 与 `resource_dir` 层级 zip，自动登记/加载（日志"内置插件引导：发现 N 个内置 zip"）。→ 把 `moss-tts-*.zip` 放进 `resources/plugins` 即预装。
- 迁移先例：`plugins/mod.rs:38 migrate_legacy_engine` 已做 `edge→edge-tts`、`mimo→mimo-tts`（含 `tts_model`→`plugin_voices["mimo-tts"]`、`set_plugin_voice_if_absent`）。moss 完全照此加一条。
- 插件范式：`plugins/engines/` 下 edge-tts/mimo-tts/genie-tts/hojo-tts/minimax-* 为模板；`va_tts_plugin!` 宏导出 8 符号；C ABI。

## 三、设计

### 3.1 新插件 crate `plugins/engines/moss-tts/`
- `manifest.json`：`id="moss-tts"`、`type=tts_engine`、`category="remote"`、`version`、`checksum`；`config` 声明 `api_key`（password 型）字段供通用配置面板；能力=非流式（不导出 stream_* 符号）。
- `lib.rs`：`va_tts_plugin!`，把 `moss.rs` 的 HTTP→mp3 逻辑搬入；`synthesize(text, params)` 从插件自身配置读 `api_key`（经 `VA_PLUGIN_*`/`plugin_config` 注入）与音色。
- 数据目录：无本地模型；不需要 `plugin-data`。

### 3.2 宿主摘除内置 MOSS
- 删 `src-tauri/src/tts/moss.rs`；`tts/mod.rs` 去 `pub mod moss;`。
- `commands/tts.rs`：去掉 `match` 的 `"moss"` 分支（一律走 `plugin_id => build_engine`）。
- 前端/设置：去掉 MOSS 专属输入（API Key、voice_id）与 `tts_engine==="moss"` 特判；改由插件自述配置面板 + `plugin_voices` 通用音色。
- `storage/types.rs`：`moss_api_key` / `moss_voice_id` **两步走退役**（AGENTS §遗留字段约定）——本版本**保留字段**仅供 `migrate_legacy_engine` 读取迁移值；不再从 UI 写；下一版本再删字段。

### 3.3 迁移（`migrate_legacy_engine` 增一条）
在现有 edge/mimo 迁移后追加：
```
// 内置 moss → moss-tts 插件；key 迁 plugin_config、voice 迁 plugin_voices
if obj.get("tts_engine") == Some("moss") {
    obj["tts_engine"] = "moss-tts";
    // moss_api_key → plugin_config["moss-tts"]["api_key"]（仅当插件侧尚未有值）
    // moss_voice_id → plugin_voices["moss-tts"]（set_plugin_voice_if_absent 语义）
}
```
- 幂等：已有目标值不覆盖；不经 `Settings` 结构体（同法，避免丢未知键）。
- 单测：moss→moss-tts 引擎改指 + key/voice 搬迁 + 二次调用不重复改。

### 3.4 打包预装 + 全插件进在线索引
- 构建 `moss-tts` 插件 zip（`package.ps1`）→ `Copy-Item` 入 `src-tauri/resources/plugins/`（与 mimo/minimax 同批预装，随包离线即装；tauri `bundle.resources=["resources/plugins/*.zip"]`）。
- **用户定调 2026-10-14：所有引擎插件都进在线索引**（moss-tts 预装 + 同时进 `plugins-index.json`，以便在线更新）。需核 `publish.ps1` 的 `$Plugins` 清单涵盖全部 engines（edge/genie/hojo/mimo/minimax-*/moss）且无不当 `$BuiltinOnly` 排除；moss-tts 加入清单。
- 首启 `bootstrap_bundled_zips` 自动装预装 zip；`migrate_legacy_engine` 把老用户 `tts_engine` 指过去。
- 注：“预装+索引同 id”需遵循“同 id 内置优先、版本对齐”，`verify-index` 据“索引有、resources 也有”算绿。

### 3.5 护栏与隐私
- **base-hygiene**：`moss`/`MossEngine` 是引擎身份字面量（现列在册豁免）。摘除后按 AGENTS 说明跑 `-UpdateBaseline`（基线只许变少）。
- 隐私：API Key 不落日志、`audio_path` 保持相对；不写绝对路径（存储契约）。
- 迁移/装载失败：记入 `PluginManager.failed` 含中文原因，不影响主程序启动。

## 四、风险 / 边界（诚实列出）

1. **老用户没配 key**：升级前就没填 `moss_api_key` 的 moss 用户，迁移后选 moss-tts 仍报"未配置 Key"——行为不变、可接受（错误经插件返回、走现有失败可见链路）。
2. **字段退役节奏**：本版本保留 `moss_api_key`/`moss_voice_id` 字段做迁移源；若过早删字段会让老用户 key 无处读 → 必须**两步走**，本版只停写、不删。
3. **dll 不卸载**：moss-tts 作为新预装插件首次进包，正常加载；无历史 dll 覆盖问题。
4. **索引与内置并存**：若 moss-tts 同时进在线索引与内置，需遵循"同 id 内置优先、版本对齐"，避免 `verify-index` 与安装优先级打架。
5. 版本 bump / tag 属 P6 红线，本设计不含发布动作。

## 五、测试与验收

- 单元：`migrate_legacy_engine` moss 分支（引擎改指、key/voice 搬迁、幂等）；插件 `synthesize` 关键分支（缺 key→中文错误）按 edge/mimo 既有测法。
- 门禁 8 道全绿（含 base-hygiene 更新后）。
- 真机：① 老用户（曾内置 moss 且配了 key/voice）升级后默认仍可选 moss-tts 并发声、key/音色在；② 新装机 resources 里有 moss-tts、插件库可见"已预装"、选了能发；③ 本体不再出现内置 MOSS 代码路径。

## 六、影响文件清单（预估）

- 新增：`plugins/engines/moss-tts/**`（crate：Cargo.toml/lib.rs/manifest 由 build 产出）
- 删除：`src-tauri/src/tts/moss.rs`
- 改：`tts/mod.rs`、`commands/tts.rs`、`plugins/mod.rs`（迁移）、`storage/types.rs`（字段标注退役/停写 UI）、`settings.rs`/`src/types/index.ts`/前端设置面板与 `VoiceSynthPanel`/`settingsStore` 相关 moss 特判、`publish.ps1`（预装清单）、`base-hygiene` 基线、skill/参考文档"内置只剩 MOSS"一句。
- 文档：本设计 + 实施 plan（待生成）+ 看板 P2.8 勾选。
