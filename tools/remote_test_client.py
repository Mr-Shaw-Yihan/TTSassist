#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""移动端遥控 本体服务 验证客户端（模拟手机 App 的完整遥控流程）。

原 lan-remote 插件的 test_client.py 迁入本体 tools/，并同步到当前协议：
配对码（pair/refresh_code）已于 1.8.x 移除，现走免码弹窗配对（pair_request）
或已配对 token 重连（hello）。命令序列（list_favorites/play_favorite/stop/
synthesize/toggle_mic/play_last）与协议契约保持不变（doc/移动端遥控器设计.md §三）。

跑通：pair_request（或 hello）→ list_favorites → play_favorite → stop
→ synthesize → toggle_mic → play_last，并打印期间收到的全部 s2c 推送
（state / event / ack）。

用法：
    pip install websockets
    # 免码配对（需在 PC 端弹窗点「是」，物理在场模型）
    python tools/remote_test_client.py --host 127.0.0.1 --pair-request --device "测试手机"
    # 已配对 token 重连（跳过配对，适合自动化回归）
    python tools/remote_test_client.py --host 127.0.0.1 --token <64位hex>

参数：
    --host          PC 端 IP（本机测试填 127.0.0.1）
    --port          WS 端口，默认 45271
    --pair-request  走免码弹窗配对（发 pair_request，等 PC 端点「是」后回 pair_ok）
    --device        pair_request 上报的设备名，默认「测试手机」
    --token         用已配对的 token 走 hello 重连（跳过配对）
    --skip          跳过部分步骤（逗号分隔：favorites,play,stop,synth,mic,last）

说明：
    - 命令回执按 ref 关联（协议契约见 doc/移动端遥控器设计.md §三）；
    - 每步打印宿主推送，最后静听 5 秒事件后退出；
    - pair_ok 返回的 token 应持久化，供下次 --token 重连（App 侧同理）。
"""

import argparse
import asyncio
import json
import sys

try:
    import websockets
except ImportError:
    print("缺少 websockets 库：pip install websockets", file=sys.stderr)
    sys.exit(1)

REF = 0


def next_ref() -> str:
    global REF
    REF += 1
    return str(REF)


async def recv_until(ws, want_t: str, timeout: float = 30.0):
    """收帧直到出现指定消息类型（沿途打印其它推送）"""
    while True:
        raw = await asyncio.wait_for(ws.recv(), timeout=timeout)
        msg = json.loads(raw)
        tag = msg.get("t")
        if tag == "state":
            print(f"    << state: {json.dumps(msg['state'], ensure_ascii=False)}")
        elif tag == "event":
            print(f"    << event: {json.dumps(msg.get('event'), ensure_ascii=False)}")
        else:
            print(f"    << {raw}")
        if tag == want_t:
            return msg


async def send_and_wait_ack(ws, t: str, fields: dict, timeout: float = 60.0):
    """发命令并等对应 ref 的 ack"""
    ref = next_ref()
    payload = {"t": t, "ref": ref, **fields}
    print(f"    >> {json.dumps(payload, ensure_ascii=False)}")
    await ws.send(json.dumps(payload, ensure_ascii=False))
    while True:
        raw = await asyncio.wait_for(ws.recv(), timeout=timeout)
        msg = json.loads(raw)
        tag = msg.get("t")
        if tag == "ack" and msg.get("ref") == ref:
            print(f"    << ack: ok={msg.get('ok')} err={msg.get('err')!r}")
            return msg
        if tag == "state":
            print(f"    << state: {json.dumps(msg['state'], ensure_ascii=False)}")
        elif tag == "event":
            print(f"    << event: {json.dumps(msg.get('event'), ensure_ascii=False)}")
        else:
            print(f"    << {raw}")


async def main() -> int:
    ap = argparse.ArgumentParser(description="移动端遥控本体服务 验证客户端")
    ap.add_argument("--host", default="127.0.0.1")
    ap.add_argument("--port", type=int, default=45271)
    ap.add_argument("--pair-request", action="store_true", help="走免码弹窗配对")
    ap.add_argument("--device", default="测试手机")
    ap.add_argument("--token")
    ap.add_argument("--skip", default="")
    args = ap.parse_args()
    skip = {s.strip() for s in args.skip.split(",") if s.strip()}

    if not args.token and not args.pair_request:
        ap.error("需要 --token（重连）或 --pair-request（免码弹窗配对）")

    uri = f"ws://{args.host}:{args.port}"
    print(f"连接 {uri} …")
    async with websockets.connect(uri) as ws:
        if args.token:
            print(">> hello（token 重连）")
            await ws.send(json.dumps({"t": "hello", "token": args.token}))
            hello = await recv_until(ws, "hello_ok")
            print(f"重连成功: {json.dumps(hello.get('state'), ensure_ascii=False)}")
        else:
            print(f">> pair_request（免码配对，设备名 {args.device}）")
            print("    ⚠ 请在 PC 端弹窗点「是」以完成物理在场确认…")
            await ws.send(
                json.dumps({"t": "pair_request", "device": args.device}, ensure_ascii=False)
            )
            pair = await recv_until(ws, "pair_ok", timeout=120)
            token = pair.get("token", "")
            print(f"配对成功 token={token[:8]}…（App 侧应持久化此 token 用于重连）")

        # 保活
        await ws.send(json.dumps({"t": "ping"}))
        await recv_until(ws, "pong", timeout=5)

        # 拉收藏
        fav_id = None
        if "favorites" not in skip:
            print("\n[1] list_favorites")
            await ws.send(json.dumps({"t": "list_favorites", "ref": next_ref()}))
            favs = await recv_until(ws, "favorites")
            items = favs.get("items", [])
            print(f"    收藏 {len(items)} 条")
            for it in items[:5]:
                print(f"      - {it.get('id')} {it.get('note')}")
            fav_id = items[0]["id"] if items else None
            if not items:
                print("    （PC 端暂无收藏，跳过播放步骤）")

        # 播放收藏 → 停止
        if fav_id and "play" not in skip:
            print("\n[2] play_favorite")
            await send_and_wait_ack(ws, "play_favorite", {"id": fav_id})
            print("    等待 3 秒（期间应收到播放态 state 推送）…")
            await asyncio.sleep(3)
            if "stop" not in skip:
                print("\n[3] stop")
                await send_and_wait_ack(ws, "stop", {})
                await asyncio.sleep(1)

        # 文字合成
        if "synth" not in skip:
            print("\n[4] synthesize")
            await send_and_wait_ack(
                ws, "synthesize", {"text": "你好，这是手机遥控的合成测试。"}, timeout=120
            )
            print("    等待 4 秒（播放中应收到 state: synthesizing/playing 变化）…")
            await asyncio.sleep(4)

        # 麦克风开关
        if "mic" not in skip:
            print("\n[5] toggle_mic")
            await send_and_wait_ack(ws, "toggle_mic", {})
            await asyncio.sleep(0.5)
            print("    （再切一次，恢复原状态）")
            await send_and_wait_ack(ws, "toggle_mic", {})

        # 播放上一条
        if "last" not in skip:
            print("\n[6] play_last")
            await send_and_wait_ack(ws, "play_last", {})
            await asyncio.sleep(2)

        # 静听事件推送
        print("\n静听 5 秒宿主事件推送…")
        try:
            await recv_until(ws, "__never__", timeout=5)
        except asyncio.TimeoutError:
            pass

    print("\n全部流程完成 ✔")
    return 0


if __name__ == "__main__":
    sys.exit(asyncio.run(main()))
