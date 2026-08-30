// WS 客户端：实现设计文档 §3.1 传输总则——鉴权先行、ref/ack 关联、应用层心跳、
// 鉴权失败服务端断开。重连退避由上层状态层负责（本类只报告 phase 变化）。

import 'dart:async';
import 'dart:convert';

import 'package:flutter/foundation.dart';
import 'package:web_socket_channel/web_socket_channel.dart';

import '../proto/messages.dart';

enum ConnPhase { disconnected, connecting, awaitingAuth, connected }

class RemoteClient {
  RemoteClient({
    required this.onPhase,
    required this.onMessage,
  });

  /// 连接阶段变化（含意外断开 → disconnected）
  final void Function(ConnPhase phase) onPhase;

  /// 全局消息（state/favorites/pair_ok/hello_ok/event/error/未消费的 ack…）
  final void Function(S2C msg) onMessage;

  WebSocketChannel? _channel;
  ConnPhase _phase = ConnPhase.disconnected;
  ConnPhase get phase => _phase;
  bool get isConnected => _phase == ConnPhase.connected;

  /// 连接世代：旧连接的 stream 回调（onDone/onError）不得影响新连接，
  /// 否则并发连接时旧断开会误触发新会话的重连调度（5ms 断开循环的根因）
  int _gen = 0;

  Timer? _pingTimer;
  int _refSeq = 0;
  final Map<String, Completer<S2C>> _pending = {};

  void _setPhase(ConnPhase p) {
    if (_phase == p) return;
    _phase = p;
    onPhase(p);
  }

  /// 建立连接；成功后进入 awaitingAuth，调用方须 hello() 或 pair() 完成鉴权
  Future<void> connect(String host, {int port = 45271}) async {
    _teardown();
    _setPhase(ConnPhase.connecting);
    final myGen = ++_gen;
    final ch = WebSocketChannel.connect(Uri.parse('ws://$host:$port'));
    try {
      // 5s 握手超时：断网重连时部分连接挂死（无成功无错误），超时确保重连循环转动
      await ch.ready.timeout(const Duration(seconds: 5));
      debugPrint('[remote] WS 握手完成: $host:$port');
    } catch (e) {
      debugPrint('[remote] WS 握手失败: $host:$port $e');
      // 不 await：无路由时 sink.close() 可能永久挂起，卡死上层退避重连链条
      // （真机 WiFi 抖动后「一直连接中」的根因），世代号已使其回调失效
      ch.sink.close().catchError((_) {});
      if (myGen == _gen) _setPhase(ConnPhase.disconnected);
      rethrow;
    }
    if (myGen != _gen) {
      // 握手期间被更新的连接取代，关掉自己退出
      await ch.sink.close().catchError((_) {});
      return;
    }
    _channel = ch;
    _setPhase(ConnPhase.awaitingAuth);
    ch.stream.listen(
      (d) {
        if (myGen == _gen) _onData(d);
      },
      onError: (_) {
        if (myGen == _gen) _drop();
      },
      onDone: () {
        if (myGen == _gen) _drop();
      },
      cancelOnError: true,
    );
  }

  void _onData(dynamic data) {
    S2C msg;
    try {
      msg = S2C.decode(data as String);
    } catch (_) {
      return; // 非法帧忽略
    }
    switch (msg.t) {
      case 'pair_ok':
      case 'hello_ok':
        _setPhase(ConnPhase.connected);
        _startPing();
        break;
      case 'pong':
        return; // 保活回执，不上抛
      case 'ack':
        final r = msg.ref;
        final c = r == null ? null : _pending.remove(r);
        if (c != null) {
          if (!c.isCompleted) c.complete(msg);
          return; // ack 由命令调用方消费，不全局广播
        }
        break;
      default:
        break;
    }
    onMessage(msg);
  }

  /// 意外断开 / 服务端主动断开：清在途命令并上报
  void _drop() {
    debugPrint('[remote] WS 断开');
    _stopPing();
    for (final c in _pending.values) {
      if (!c.isCompleted) c.completeError(const ConnectionLost());
    }
    _pending.clear();
    _channel = null;
    _setPhase(ConnPhase.disconnected);
  }

  void _teardown() {
    _gen++; // 使旧连接的回调失效
    _stopPing();
    for (final c in _pending.values) {
      if (!c.isCompleted) c.completeError(const ConnectionLost());
    }
    _pending.clear();
    final old = _channel;
    _channel = null;
    old?.sink.close().catchError((_) {}); // 主动关旧通道；其 onDone 因世代失效被忽略
  }

  void _startPing() {
    _stopPing();
    _pingTimer = Timer.periodic(const Duration(seconds: 15), (_) {
      _send({'t': 'ping'});
    });
  }

  void _stopPing() {
    _pingTimer?.cancel();
    _pingTimer = null;
  }

  void _send(Map<String, dynamic> m) {
    final ch = _channel;
    if (ch == null) return;
    ch.sink.add(jsonEncode(m));
  }

  // ── 鉴权前可用 ──
  void hello(String token) => _send({'t': 'hello', 'token': token});
  void pairRequest(String device) => _send({'t': 'pair_request', 'device': device});
  // 配对码路径（pair / refresh_code）App 侧已移除：v3 起仅走免码配对 + hello 重连，
  // 服务端协议保留不变（见 doc/移动端遥控器设计.md §4.5）。

  // ── 鉴权后命令（带 ref，等 ack）──
  /// [timeout] 默认 10s；synthesize 合成完才回 ack，调用方放宽到 120s+
  Future<S2C> command(
    String t, {
    Map<String, dynamic>? extra,
    Duration timeout = const Duration(seconds: 10),
  }) {
    final ch = _channel;
    if (ch == null || _phase != ConnPhase.connected) {
      return Future.error(const ConnectionLost());
    }
    final ref = (++_refSeq).toString();
    final c = Completer<S2C>();
    _pending[ref] = c;
    _send({'t': t, 'ref': ref, ...?extra});
    return c.future.timeout(timeout, onTimeout: () {
      _pending.remove(ref);
      throw TimeoutException('命令超时: $t');
    });
  }

  Future<void> listFavorites() async {
    await command('list_favorites'); // 列表经 s2c favorites 帧上抛
  }

  Future<S2C> playFavorite(String id) =>
      command('play_favorite', extra: {'id': id});

  Future<S2C> stop() => command('stop');

  Future<S2C> synthesize(String text) => command(
        'synthesize',
        extra: {'text': text},
        timeout: const Duration(seconds: 180),
      );

  Future<S2C> toggleMic() => command('toggle_mic');

  Future<S2C> playLast() => command('play_last');

  /// 主动断开（用户切换 PC / 退出）
  Future<void> close() async {
    final ch = _channel;
    _teardown();
    await ch?.sink.close().catchError((_) {});
    _setPhase(ConnPhase.disconnected);
  }
}

class ConnectionLost implements Exception {
  const ConnectionLost();
  @override
  String toString() => '连接已断开';
}
