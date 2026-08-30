# Hojo TTS 插件（Hojo-TTS-Light-80M 本地推理引擎）

上游：[HojoAI/Hojo-TTS-Light](https://github.com/HojoAI/Hojo-TTS-Light)（Apache-2.0，
80M 变体，零样本音色克隆，中英混读，ONNX 纯 CPU 推理，输出 24kHz 单声道 WAV）。

## 架构（对齐 genie-tts 的 Sidecar 形态）

```
VoiceAssist 宿主
  └─ plugin.dll（薄壳：环境引导 + 进程管理 + HTTP 客户端）
       └─ hojo_server.py（内嵌 Python 子进程，FastAPI）
            └─ onnx_model.py（上游推理代码原样内嵌，Apache-2.0，附许可全文）
```

- 推理核心是上游 `Hojo-TTS-Light-80M/onnx_model.py` 原样内嵌（`server/onnx_model.py`），
  运行期写出；本插件不改推理逻辑，只做资源管理与 HTTP 封装。
- Python 依赖不装上游 requirements.txt 全家桶（GPU 训练向），只装
  onnx_model.py 实际 import 的包（numpy/onnxruntime/soundfile/torch/librosa/
  scipy/tokenizers/onnx + fastapi/uvicorn/huggingface_hub）。
  注意 `onnx` 必装：模型权重是 bfloat16，CPU 推理靠它提升为 fp32。

## 数据目录布局（`<exe同级>/plugins/hojo-tts/data/`）

```
data/
├── python/              # embeddable Python 3.12.10 + 推理依赖（约 1.2GB）
├── models/              # HF HojoAI/Hojo-TTS-Light 快照（约 460MB，8 个文件）
├── voices/              # 音色包（见下）
├── hojo_server.py       # 运行期由 dll 写出
├── onnx_model.py        # 同上
├── LICENSE-Hojo-Apache-2.0.txt
├── hojo-server.log      # 服务端日志（排障第一现场）
└── server-port          # 当前服务端口（调试）
```

离线资源包预留：模型全部在 `models/` 一个目录里，把 8 个必备文件
（4 个 .onnx + voice.npz + config.json + tokenizer.json + tokenizer_config.json）
放入 `data/models/` 即等价于在线下载完成，后续补齐环境时会直接跳过模型下载。

## 音色 = 参考音频 + 参考文本（零样本克隆）

```
voices/<音色id>/
├── ref.wav     # 参考音频（建议 5~10 秒干净人声）
└── voice.json  # {"label": "展示名", "text": "参考音频逐字文本", "text_source": "official|asr|..."}
```

- **预置音色**（5 个，上游官方 demo 参考音频，首次安装从 GitHub 下载几百 KB）：

  | id | 标签 | 性别（F0 实测） | 参考文本来源 |
  |---|---|---|---|
  | `female-zh-1`（默认） | 女声·中文一 | 女（277Hz） | 上游 README 官方公开 |
  | `female-zh-2` | 女声·中文二 | 女（247Hz） | faster-whisper 转写 |
  | `female-zh-3` | 女声·中文三 | 女（233Hz） | faster-whisper 转写 |
  | `female-en` | 女声·英文 | 女（203Hz） | faster-whisper 转写 |
  | `male-en` | 男声·英文 | 男（122Hz） | faster-whisper 转写 |

  上游 80M 变体没有内置音色库（音色只能来自参考音频克隆），官方仓库
  assets/audio 里的可用参考音频就这 5 个（wav 形式）。上游未公开其中 4 个的
  参考文本，文本由 faster-whisper（small）转写并用 zh1（官方公开文本）校准
  ——校准样本逐字命中，转写质量可信。
  注意：上游 README 把 zh1 标为 "Code-switching Male Voice"，与基频实测
  （277Hz，女声）不符，本插件以实测为准；上游素材里唯一的男声是 male_en_88。
  **上游没有中文男声素材**——如需中文男声预置，可自录 5~10 秒参考音频自制成
  音色包导入（见下）。
- **自备音色**：设置 → 音色管理 → 导入音色包目录（布局同上）。克隆自己的
  声音：录一段 5~10 秒干净人声，逐字抄成 text 写进 voice.json。

## 网络（面向小白，零配置）

不做用户配置界面（下载源/安装源对普通用户无意义）。网络容错全部内置于插件：

- **模型下载**：多端点回退（hf-mirror → huggingface.co，服务端实现）
- **pip 安装**：多源回退（清华 → 腾讯 → 官方 PyPI，bootstrap.rs 实现）
- **预置音色**：GitHub raw → jsdelivr CDN 双源回退（client.rs 实现）

排障后门（无 UI）：环境变量 `HOJO_TTS_HF_ENDPOINT` 覆盖模型下载起点端点、
`HOJO_TTS_PIP_INDEX_URL` 指定唯一 pip 源（指定后不再多源回退）。

离线分发预留：后续打包好的完整资源（python + models + voices）经 Gitee/群文件
分发时，直接解压到 `data/` 即等价于在线安装完成（各阶段探测均为纯磁盘检查）。
宿主「导入离线资源包」按钮当前是 genie 专属校验，hojo 通用化需宿主侧另行扩展。

## 打包 / 安装

```powershell
powershell -ExecutionPolicy Bypass -File .\package.ps1            # 打包 zip
powershell -ExecutionPolicy Bypass -File .\package.ps1 -Install   # 打包并装进本机宿主
```

zip 只含 manifest.json + plugin.dll；运行环境与模型首次使用时下载
（依赖约 1.2GB + 模型约 460MB，合计约 1.7GB，磁盘建议预留 3GB）。

版本号三处同步：`Cargo.toml`、`src/lib.rs` 宏声明的 `version:`、`package.ps1` 的 `$Version`。

## 依赖升级（上游出新版模型时）

1. `server/onnx_model.py` 用上游新版原样覆盖（保留文件头署名注释）；
2. 若上游新增/改名模型文件：同步改 `src/setup.rs` 的 `MODEL_FILES`、
   `server/hojo_server.py` 的 `MODEL_FILES`、`src/voices.rs` 无关则不动；
3. Python 依赖变化：改 `src/bootstrap.rs` 的 `DEPS` 并把 `DEPS_VERSION` 递增
   （触发已装用户重装依赖）；
4. HF 仓库文件变化：模型下载走 `allow_patterns` 按文件名匹配，无需改仓库地址。
