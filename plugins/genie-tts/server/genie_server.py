# -*- coding: utf-8 -*-
"""
VoiceAssist Genie-TTS 插件服务端脚本（内嵌于 plugin.dll，运行期写出后由插件拉起）。

启动方式（由插件自动执行，无需手动运行）：
    python.exe genie_server.py --port <端口> --data-dir <数据目录>

职责：
- 下载/校验 Genie-TTS 运行资源（GenieData + 中文 RoBERTa，走 HF 镜像）
- 加载音色包（预置角色自动下载；用户自备音色包目录直接读）
- 提供 /tts 文本转语音（收集完整 WAV 一次性返回）

关键设计（踩过坑，改动前请阅读）：
- 启动时【不要】import genie_tts！它的 Core/Resources.py 在模块级发现 GenieData
  缺失会调 input() 交互式询问下载，无终端子进程里直接 EOFError 崩溃。
  因此本脚本先提供 /health 与 /ensure_resources（纯 huggingface_hub 下载），
  首次合成相关请求时才惰性 import genie_tts（此时资源已就位）。
- 工作目录切到数据目录：Genie 的资源定位部分依赖 CWD 相对路径。
- 参考音频是硬需求：GPT-SoVITS 零样本合成必须先 set_reference_audio，
  音色包目录内的 prompt_wav.json 记录了参考音频与对应文本。
"""

import argparse
import io
import json
import logging
import os
import sys
import threading
import wave

from fastapi import FastAPI, HTTPException
from fastapi.responses import Response
from pydantic import BaseModel

logging.basicConfig(level=logging.INFO, format="[genie-server] %(message)s")
log = logging.getLogger("genie-server")

# ── 路径与配置（启动参数优先）──────────────────────────────

parser = argparse.ArgumentParser()
parser.add_argument("--port", type=int, required=True)
parser.add_argument("--data-dir", required=True)
ARGS = parser.parse_args()

DATA_DIR = os.path.abspath(ARGS.data_dir)
os.makedirs(DATA_DIR, exist_ok=True)
os.chdir(DATA_DIR)

CONFIG_PATH = os.path.join(DATA_DIR, "genie-config.json")
GENIE_DATA_DIR = os.path.join(DATA_DIR, "GenieData")
CHARACTERS_DIR = os.path.join(DATA_DIR, "characters")
ROBERTA_DIRNAME = "roberta-wwm-ext-large-onnx"

def load_config() -> dict:
    try:
        with open(CONFIG_PATH, "r", encoding="utf-8") as f:
            return json.load(f)
    except Exception:
        return {}

CONFIG = load_config()
# 中国大陆访问 huggingface.co 基本不可达，默认走 hf-mirror.com（可在 genie-config.json 改）
os.environ.setdefault("HF_ENDPOINT", CONFIG.get("hf_endpoint", "https://hf-mirror.com"))
# 供 genie_tts 导入时定位资源（必须早于 import genie_tts）
os.environ["GENIE_DATA_DIR"] = GENIE_DATA_DIR
os.environ.setdefault("HF_HUB_DISABLE_PROGRESS_BARS", "1")

# ── 预置角色表（与官方 PredefinedCharacter.py 保持一致）────

PREDEFINED = {
    "feibi": {"language": "Chinese", "label": "菲比"},
    "mika": {"language": "Japanese", "label": "聖園ミカ"},
    "thirtyseven": {"language": "English", "label": "37"},
}
PREDEFINED_VERSION = "v2ProPlus"

# ── genie_tts 惰性导入 ─────────────────────────────────────

_genie = None
_genie_lock = threading.Lock()

def get_genie():
    """惰性导入 genie_tts（其模块级资源检查要求 GenieData 已就位）"""
    global _genie
    with _genie_lock:
        if _genie is None:
            import genie_tts as g  # noqa：刻意延迟导入，见文件头说明
            _genie = g
    return _genie

# ── 资源下载 ───────────────────────────────────────────────

def genie_data_ready() -> bool:
    """GenieData 是否就位（hubert + speaker_encoder 是最小可运行标志）"""
    return (
        os.path.isdir(os.path.join(GENIE_DATA_DIR, "chinese-hubert-base"))
        and os.path.isfile(os.path.join(GENIE_DATA_DIR, "speaker_encoder.onnx"))
    )

def ensure_genie_data() -> None:
    """下载 GenieData（幂等；snapshot_download 自动跳过已有文件）。
    中文 RoBERTa 为可选韵律增强资源（fp32 约 1.3GB），默认不下载，
    需要时在 genie-config.json 设 "download_roberta": true。"""
    from huggingface_hub import snapshot_download

    if not genie_data_ready():
        log.info("下载 GenieData 资源（首次约 400MB）…")
        snapshot_download(
            repo_id="High-Logic/Genie",
            repo_type="model",
            allow_patterns="GenieData/*",
            local_dir=DATA_DIR,
        )
    if CONFIG.get("download_roberta", False):
        roberta_dir = os.path.join(GENIE_DATA_DIR, ROBERTA_DIRNAME)
        if not os.path.isfile(os.path.join(roberta_dir, "model.onnx")):
            log.info("下载中文 RoBERTa 资源（约 1.3GB）…")
            os.makedirs(roberta_dir, exist_ok=True)
            snapshot_download(
                repo_id="litagin/chinese-roberta-wwm-ext-large-onnx",
                repo_type="model",
                allow_patterns=["model.onnx", "tokenizer.json"],
                local_dir=roberta_dir,
            )
    if not genie_data_ready():
        raise RuntimeError("GenieData 下载完成但校验失败（可能磁盘空间不足或网络中断），请重试")

# ── 音色包 ─────────────────────────────────────────────────

def pack_dir_of(voice_id: str) -> str:
    return os.path.join(CHARACTERS_DIR, voice_id)

def pack_ready(pack_dir: str) -> bool:
    """音色包布局校验：tts_models/ + prompt_wav.json"""
    return os.path.isdir(os.path.join(pack_dir, "tts_models")) and os.path.isfile(
        os.path.join(pack_dir, "prompt_wav.json")
    )

def read_pack_meta(pack_dir: str) -> dict:
    """读音色包元信息（语言/展示名），meta.json 可选"""
    meta = {"language": "Chinese", "label": os.path.basename(pack_dir)}
    meta_path = os.path.join(pack_dir, "meta.json")
    if os.path.isfile(meta_path):
        try:
            with open(meta_path, "r", encoding="utf-8") as f:
                data = json.load(f)
            if isinstance(data, dict):
                if data.get("language"):
                    meta["language"] = str(data["language"])
                if data.get("label"):
                    meta["label"] = str(data["label"])
        except Exception:
            pass
    return meta

def download_predefined(voice_id: str) -> str:
    """从 HF 下载预置角色到 characters/<voice_id>/，并写 meta.json"""
    from huggingface_hub import snapshot_download

    pack_dir = pack_dir_of(voice_id)
    tmp_dir = os.path.join(DATA_DIR, ".dl-cache", voice_id)
    os.makedirs(tmp_dir, exist_ok=True)
    log.info("下载预置音色「%s」（首次约 200MB）…", voice_id)
    snapshot_download(
        repo_id="High-Logic/Genie",
        repo_type="model",
        allow_patterns=f"CharacterModels/{PREDEFINED_VERSION}/{voice_id}/*",
        local_dir=tmp_dir,
    )
    src = os.path.join(tmp_dir, "CharacterModels", PREDEFINED_VERSION, voice_id)
    if not os.path.isdir(src):
        raise RuntimeError(f"预置音色「{voice_id}」下载结果不完整，请重试")
    # 搬到统一的音色包目录（已有则先清掉残留）
    if os.path.isdir(pack_dir):
        import shutil
        shutil.rmtree(pack_dir, ignore_errors=True)
    os.makedirs(CHARACTERS_DIR, exist_ok=True)
    os.replace(src, pack_dir)
    info = PREDEFINED[voice_id]
    with open(os.path.join(pack_dir, "meta.json"), "w", encoding="utf-8") as f:
        json.dump(
            {"label": f"{info['label']}", "language": info["language"], "predefined": True},
            f,
            ensure_ascii=False,
        )
    return pack_dir

# ── 角色加载状态 ───────────────────────────────────────────

_loaded_voice = None
_loaded_lock = threading.Lock()

def ensure_voice_loaded(voice_id: str) -> None:
    """确保指定音色已加载（含参考音频）；切换音色时卸载旧的省内存"""
    global _loaded_voice
    with _loaded_lock:
        if _loaded_voice == voice_id:
            return
        genie = get_genie()

        pack_dir = pack_dir_of(voice_id)
        if not pack_ready(pack_dir):
            if voice_id in PREDEFINED:
                download_predefined(voice_id)
            else:
                raise RuntimeError(
                    f"音色「{voice_id}」不存在或不完整：请把音色包目录放到 characters/{voice_id}/"
                    "（需包含 tts_models/ 与 prompt_wav.json）"
                )
        if not pack_ready(pack_dir):
            raise RuntimeError(f"音色「{voice_id}」下载后仍不完整，请重试")

        meta = read_pack_meta(pack_dir)

        # 切换音色：先卸载旧的（v1 只驻留一个音色，控制内存占用）
        if _loaded_voice is not None:
            try:
                genie.unload_character(_loaded_voice)
            except Exception:
                pass

        log.info("加载音色「%s」（%s）…", voice_id, meta["language"])
        genie.load_character(
            character_name=voice_id,
            onnx_model_dir=os.path.join(pack_dir, "tts_models"),
            language=meta["language"],
        )

        # 参考音频（prompt_wav.json 记录）
        with open(os.path.join(pack_dir, "prompt_wav.json"), "r", encoding="utf-8") as f:
            prompt = json.load(f)
        normal = prompt.get("Normal") or next(iter(prompt.values()))
        audio_path = os.path.join(pack_dir, "prompt_wav", normal["wav"])
        genie.set_reference_audio(
            character_name=voice_id,
            audio_path=audio_path,
            audio_text=normal["text"],
            language=meta["language"],
        )
        _loaded_voice = voice_id

# ── FastAPI 应用 ───────────────────────────────────────────

app = FastAPI(title="VoiceAssist Genie-TTS Server")

class VoicePayload(BaseModel):
    voice_id: str

class TtsPayload(BaseModel):
    voice_id: str
    text: str

@app.get("/health")
def health():
    return {"status": "ok"}

@app.post("/ensure_resources")
def ensure_resources():
    try:
        ensure_genie_data()
        return {"status": "success", "message": "Genie-TTS 资源已就绪"}
    except Exception as e:
        raise HTTPException(status_code=500, detail=f"资源下载失败: {e}")

@app.post("/load_character")
def load_character(payload: VoicePayload):
    try:
        ensure_voice_loaded(payload.voice_id)
        return {"status": "success", "message": f"音色 {payload.voice_id} 已加载"}
    except HTTPException:
        raise
    except Exception as e:
        raise HTTPException(status_code=500, detail=str(e))

@app.post("/tts")
async def tts(payload: TtsPayload):
    try:
        ensure_voice_loaded(payload.voice_id)
    except Exception as e:
        raise HTTPException(status_code=500, detail=str(e))

    genie = get_genie()
    try:
        chunks = []
        async for chunk in genie.tts_async(
            character_name=payload.voice_id,
            text=payload.text,
            play=False,
            split_sentence=True,
        ):
            if chunk:
                chunks.append(chunk)
    except Exception as e:
        raise HTTPException(status_code=500, detail=f"合成失败: {e}")

    audio = b"".join(chunks)
    if not audio:
        raise HTTPException(status_code=500, detail="合成未产生音频（文本可能为空或全是符号）")

    # tts_async 分片是裸 PCM（16bit/32kHz/单声道）；若已带 RIFF 头则直通，
    # 否则包一层 WAV 容器（宿主按 manifest 声明的 wav 落盘播放）
    if audio[:4] == b"RIFF":
        wav_bytes = audio
    else:
        buf = io.BytesIO()
        with wave.open(buf, "wb") as wf:
            wf.setnchannels(1)
            wf.setsampwidth(2)
            wf.setframerate(32000)
            wf.writeframes(audio)
        wav_bytes = buf.getvalue()
    return Response(content=wav_bytes, media_type="audio/wav")

@app.post("/unload_character")
def unload_character(payload: VoicePayload):
    global _loaded_voice
    with _loaded_lock:
        if _loaded_voice == payload.voice_id:
            try:
                get_genie().unload_character(payload.voice_id)
            except Exception:
                pass
            _loaded_voice = None
    return {"status": "success"}

if __name__ == "__main__":
    import uvicorn

    log.info("启动于 127.0.0.1:%s（数据目录 %s）", ARGS.port, DATA_DIR)
    uvicorn.run(app, host="127.0.0.1", port=ARGS.port, log_level="warning")
