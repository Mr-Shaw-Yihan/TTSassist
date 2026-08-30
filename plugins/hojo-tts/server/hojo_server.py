# -*- coding: utf-8 -*-
"""
VoiceAssist Hojo-TTS-Light-80M 插件服务端脚本（内嵌于 plugin.dll，运行期写出后由插件拉起）。

启动方式（由插件自动执行，无需手动运行）：
    python.exe hojo_server.py --port <端口> --data-dir <数据目录>

职责：
- 下载/校验 Hojo-TTS-Light ONNX 模型（HuggingFace 仓库 HojoAI/Hojo-TTS-Light，走 HF 镜像回退）
- 加载音色包（voices/<id>/ 下的参考音频 + 参考文本，零样本克隆）
- 提供 /tts 文本转语音（生成 24kHz 单声道 16bit WAV 一次性返回）

推理核心是同目录的 onnx_model.py（上游 https://github.com/HojoAI/Hojo-TTS-Light
的 Hojo-TTS-Light-80M/onnx_model.py 原样内嵌，Apache-2.0 许可，随包附
LICENSE-Hojo-Apache-2.0.txt）。本脚本只做资源管理与 HTTP 封装，不改推理逻辑。

关键设计（与 genie_server.py 同构）：
- 启动时不 import onnx_model——它顶层 import torch/librosa/onnxruntime（重且慢），
  且构造引擎要求模型文件全部就位。因此先提供 /health 与 /ensure_models（纯
  huggingface_hub 下载），首次 /load_voice 或 /tts 时才惰性构造推理引擎。
- 合成用推理锁串行：CPU 上 LM 逐 token 生成，并发合成只会互相拖慢。
"""

import argparse
import io
import json
import logging
import os
import sys
import threading

import soundfile as sf
from fastapi import FastAPI, HTTPException
from fastapi.responses import Response
from pydantic import BaseModel

logging.basicConfig(level=logging.INFO, format="[hojo-server] %(message)s")
log = logging.getLogger("hojo-server")

# ── 路径与配置（启动参数优先）──────────────────────────────

parser = argparse.ArgumentParser()
parser.add_argument("--port", type=int, required=True)
parser.add_argument("--data-dir", required=True)
ARGS = parser.parse_args()

DATA_DIR = os.path.abspath(ARGS.data_dir)
os.makedirs(DATA_DIR, exist_ok=True)
os.chdir(DATA_DIR)

# embeddable Python 的 ._pth 接管了 sys.path（不含脚本目录），
# onnx_model.py 与本脚本同目录，必须显式补上才能 import
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))

MODELS_DIR = os.path.join(DATA_DIR, "models")
VOICES_DIR = os.path.join(DATA_DIR, "voices")

# HF 端点由 dll 注入（来自宿主通用插件配置 HOJO_TTS_HF_ENDPOINT，缺省走镜像）。
# 中国大陆访问 huggingface.co 基本不可达，镜像优先；下载统一多端点回退。
os.environ.setdefault("HF_ENDPOINT", os.environ.get("HOJO_TTS_HF_ENDPOINT", "https://hf-mirror.com"))
os.environ.setdefault("HF_HUB_DISABLE_PROGRESS_BARS", "1")
# 单请求超时收短：坏端点快速失败，交给回退机制换源重试（缺省 10s 太保守易误杀大文件，
# 这里只管 connect/read 空闲超时，大文件持续传输不受影响）
os.environ.setdefault("HF_HUB_DOWNLOAD_TIMEOUT", "30")

HF_REPO_ID = "HojoAI/Hojo-TTS-Light"

# 模型目录必备文件（与上游 onnx_model.py 的加载要求一一对应）
MODEL_FILES = [
    "Hojo-TTS-Light-llm.onnx",
    "Hojo-TTS-Light-encoder.onnx",
    "Hojo-TTS-Light-decoder.onnx",
    "Hojo-TTS-Light-speaker.onnx",
    "Hojo-TTS-Light-voice.npz",
    "config.json",
    "tokenizer.json",
    "tokenizer_config.json",
]


def endpoint_candidates() -> list:
    """下载端点候选：注入端点 → hf-mirror → 官方源（去重）"""
    cands = []
    for c in (
        os.environ.get("HOJO_TTS_HF_ENDPOINT", ""),
        "https://hf-mirror.com",
        "https://huggingface.co",
    ):
        c = (c or "").strip().rstrip("/")
        if c and c not in cands:
            cands.append(c)
    return cands


def _reset_hf_client() -> None:
    """强制重置 huggingface_hub 全局 HTTP 客户端。
    某端点请求失败后客户端可能被关闭/污染，不重置的话下一个端点会报
    "Cannot send a request, as the client has been closed"。"""
    for importer in (
        lambda: __import__("huggingface_hub.utils._http", fromlist=["close_session"]).close_session,
        lambda: __import__("huggingface_hub.utils", fromlist=["close_session"]).close_session,
    ):
        try:
            importer()()
            return
        except Exception:
            continue


def snapshot_download_with_fallback(repo_id: str, allow_patterns, local_dir: str, desc: str) -> None:
    """多端点回退下载：任一端点成功即返回；全部失败才抛错。

    端点切换的坑（huggingface_hub 1.x 实测）：
    - HF_ENDPOINT 环境变量只在库导入时读一次，运行期改它无效；
    - 运行期改 constants.ENDPOINT 也无效——文件下载 URL 由
      constants.HUGGINGFACE_CO_URL_TEMPLATE 构造，该模板导入时就固化了。
    因此两个都要改，并在换端点后 close_session() 重置 HTTP 客户端。"""
    from huggingface_hub import snapshot_download
    import huggingface_hub.constants as hf_const

    last_err = None
    for ep in endpoint_candidates():
        try:
            log.info("%s：尝试端点 %s", desc, ep)
            _reset_hf_client()
            try:
                hf_const.ENDPOINT = ep
                hf_const.HUGGINGFACE_CO_URL_TEMPLATE = (
                    ep + "/{repo_id}/resolve/{revision}/{filename}"
                )
            except Exception:
                pass
            os.environ["HF_ENDPOINT"] = ep
            snapshot_download(
                repo_id=repo_id,
                repo_type="model",
                allow_patterns=allow_patterns,
                local_dir=local_dir,
            )
            log.info("%s：端点 %s 下载完成", desc, ep)
            return
        except Exception as e:
            log.warning("%s：端点 %s 失败（%s），尝试下一个", desc, ep, e)
            last_err = e
    raise RuntimeError(f"{desc}失败：全部下载端点均不可用（{' / '.join(endpoint_candidates())}）。请检查网络后重试。最后错误: {last_err}")


# ── 推理引擎惰性构造 ─────────────────────────────────────

_tts = None
_tts_lock = threading.Lock()


def get_tts():
    """惰性构造 HojoTTSLightOnnx（onnx_model 顶层 import torch 等，重且慢）。
    模型加载含 bf16→fp32 提升（onnx 包），首次构造可能需要几十秒。"""
    global _tts
    if _tts is not None:
        return _tts
    with _tts_lock:
        if _tts is None:
            import onnx_model  # noqa：刻意延迟导入，见文件头说明

            log.info("加载 Hojo 推理引擎（首次较慢）…")
            _tts = onnx_model.HojoTTSLightOnnx(MODELS_DIR, provider="cpu")
    return _tts


# ── 模型下载 ───────────────────────────────────────────────

def models_ready() -> bool:
    """模型目录 8 个必备文件是否齐全"""
    return all(os.path.isfile(os.path.join(MODELS_DIR, name)) for name in MODEL_FILES)


def ensure_models() -> None:
    """下载模型（幂等；snapshot_download 自动跳过已有文件，断点续传）"""
    if not models_ready():
        log.info("下载 Hojo 模型（首次约 460MB）…")
        snapshot_download_with_fallback(
            repo_id=HF_REPO_ID,
            allow_patterns=MODEL_FILES,
            local_dir=MODELS_DIR,
            desc="Hojo 模型下载",
        )
    if not models_ready():
        raise RuntimeError("模型下载完成但校验失败（可能磁盘空间不足或网络中断），请重试")


# ── 音色包 ─────────────────────────────────────────────────

def voice_pack_dir(voice_id: str) -> str:
    return os.path.join(VOICES_DIR, voice_id)


def read_pack_meta(pack_dir: str) -> dict:
    """读音色包 voice.json：{"label": 展示名, "text": 参考音频文本}"""
    path = os.path.join(pack_dir, "voice.json")
    with open(path, "r", encoding="utf-8") as f:
        data = json.load(f)
    if not isinstance(data, dict):
        raise ValueError("voice.json 格式不对")
    return {
        "label": str(data.get("label") or os.path.basename(pack_dir)),
        "text": str(data.get("text") or ""),
    }


def pack_ready(pack_dir: str) -> bool:
    """音色包布局校验：ref.wav + voice.json（缺参考文本仍可用，仅克隆质量受限）"""
    return os.path.isfile(os.path.join(pack_dir, "ref.wav")) and os.path.isfile(
        os.path.join(pack_dir, "voice.json")
    )


# ── FastAPI 应用 ───────────────────────────────────────────

app = FastAPI(title="VoiceAssist Hojo-TTS-Light Server")

# 合成串行锁：CPU 推理一次一句，避免并发互相拖慢 + numpy 全局随机种子竞争
_gen_lock = threading.Lock()


class VoicePayload(BaseModel):
    voice_id: str


class TtsPayload(BaseModel):
    voice_id: str
    text: str


@app.get("/health")
def health():
    return {"status": "ok"}


@app.post("/ensure_models")
def ensure_models_ep():
    try:
        ensure_models()
        return {"status": "success", "message": "Hojo 模型已就绪"}
    except Exception as e:
        raise HTTPException(status_code=500, detail=f"模型下载失败: {e}")


@app.post("/load_voice")
def load_voice(payload: VoicePayload):
    """加载音色：校验音色包 + 构造推理引擎（预热）。参考音频在每次合成时现编码
    （上游 generate 的行为），因此这里没有按音色的内存状态可加载。"""
    pack_dir = voice_pack_dir(payload.voice_id)
    if not pack_ready(pack_dir):
        raise HTTPException(
            status_code=400,
            detail=f"音色「{payload.voice_id}」未安装或不完整（需要 ref.wav 与 voice.json）",
        )
    try:
        get_tts()
    except Exception as e:
        raise HTTPException(status_code=500, detail=f"推理引擎初始化失败: {e}")
    return {"status": "success", "message": f"音色 {payload.voice_id} 已就绪"}


@app.post("/tts")
def tts(payload: TtsPayload):
    pack_dir = voice_pack_dir(payload.voice_id)
    if not pack_ready(pack_dir):
        raise HTTPException(
            status_code=400,
            detail=f"音色「{payload.voice_id}」未安装或不完整，请到 设置 → 音色管理 中安装",
        )
    meta = read_pack_meta(pack_dir)

    try:
        engine = get_tts()
    except Exception as e:
        raise HTTPException(status_code=500, detail=f"推理引擎初始化失败: {e}")

    try:
        with _gen_lock:
            wav = engine.generate(
                payload.text,
                prompt_speech=os.path.join(pack_dir, "ref.wav"),
                prompt_text=meta["text"],
            )
    except Exception as e:
        raise HTTPException(status_code=500, detail=f"合成失败: {e}")

    # 上游输出 float32 ndarray @24kHz；统一包成 16bit PCM WAV 容器
    buf = io.BytesIO()
    sf.write(buf, wav, engine.sample_rate, format="WAV", subtype="PCM_16")
    return Response(content=buf.getvalue(), media_type="audio/wav")


if __name__ == "__main__":
    import uvicorn

    log.info("启动于 127.0.0.1:%s（数据目录 %s）", ARGS.port, DATA_DIR)
    uvicorn.run(app, host="127.0.0.1", port=ARGS.port, log_level="warning")
