// 连接会话：鉴权编排 + token/上次 PC 持久化 + 断线重连退避（设计文档 §3.1/§3.3）。
// 上层（状态层/UI）通过回调感知阶段与消息；命令发送直接用 client。

import 'dart:async';

import 'package:flutter/foundation.dart';
import 'package:shared_preferences/shared_preferences.dart';

import '../proto/messages.dart';
import 'remote_client.dart';

class RemoteSession {
  RemoteSession() {
    _client = RemoteClient(onPhase: _onPhase, onMessage: _onMessage);
  }

  late final RemoteClient _client;
  RemoteClient get client => _client;

  /// 阶段变化（含重连引起的 connecting/connected 往返）
  void Function(ConnPhase phase)? onPhase;

  /// s2c 消息透传给上层状态层（state/favorites/event/ack…）
  void Function(S2C msg)? onMessage;

  /// 需要用户介入配对（无 token / hello 被拒）：App 走免码配对 + 手动 IP 兜底
  void Function()? onNeedsPairing;

  String? _token;
  String? _host;
  int _port = 45271;
  bool _userClosed = true;
  int _backoffSec = 0;
  Timer? _reconnectTimer;
  /// 本次连接是否走了 token 重连（hello）；false = 需要新配对（pair_request/配对码）
  bool _usedHello = false;
  bool get usedHello => _usedHello;

  static const _kToken = 'lan_remote.token';
  static const _kHost = 'lan_remote.host';
  static const _kPort = 'lan_remote.port';

  ConnPhase get phase => _client.phase;
  String? get savedHost => _host;
  int get savedPort => _port;

  /// 启动时恢复持久化（token / 上次 PC）；有上次 PC 则返回 true 供 UI 自动连接
  Future<bool> init() async {
    final p = await SharedPreferences.getInstance();
    _token = p.getString(_kToken);
    _host = p.getString(_kHost);
    _port = p.getInt(_kPort) ?? 45271;
    return _host != null;
  }

  /// 连接手动输入 IP:端口（配对页入口）
  Future<void> connectManual(String host, {int port = 45271}) async {
    final p = await SharedPreferences.getInstance();
    await p.setString(_kHost, host);
    await p.setInt(_kPort, port);
    _host = host;
    _port = port;
    await _connect(host, port);
  }

  Future<void> _connect(String host, int port) async {
    debugPrint('[remote] connect: $host:$port');
    _reconnectTimer?.cancel();
    _userClosed = false;
    _usedHello = false;
    try {
      // 看门狗：connect 内部若有未预料的挂起点（不止握手），10s 强制返回，
      // 确保退避重连链条永不断（真机 WiFi 抖动「一直连接中」的兜底）
      await _client.connect(host, port: port).timeout(
            const Duration(seconds: 10),
            onTimeout: () => throw const ConnectionLost(),
          );
    } catch (_) {
      _scheduleReconnect(); // 连接被拒/超时 → 退避重试
    }
  }

  void _onPhase(ConnPhase ph) {
    switch (ph) {
      case ConnPhase.awaitingAuth:
        final t = _token;
        if (t != null && t.isNotEmpty) {
          _usedHello = true;
          _client.hello(t); // 有 token 自动重连鉴权
        } else {
          onNeedsPairing?.call();
        }
        break;
      case ConnPhase.connected:
        _backoffSec = 0; // 连上即重置退避
        break;
      case ConnPhase.disconnected:
        if (!_userClosed) _scheduleReconnect();
        break;
      case ConnPhase.connecting:
        break;
    }
    onPhase?.call(ph);
  }

  /// 指数退避 1→2→4→…→30s 封顶；用户主动断开不重连
  void _scheduleReconnect() {
    if (_userClosed || _host == null) return;
    _backoffSec = _backoffSec == 0 ? 1 : (_backoffSec * 2).clamp(1, 30);
    debugPrint('[remote] ${_backoffSec}s 后重连');
    _reconnectTimer?.cancel();
    _reconnectTimer = Timer(Duration(seconds: _backoffSec), () {
      if (!_userClosed && _host != null) _connect(_host!, _port);
    });
  }

  void _onMessage(S2C m) {
    switch (m.t) {
      case 'pair_ok':
        _saveToken(m.token); // 配对成功：token 即用即持久化
        break;
      case 'error':
        // 配对被拒/令牌错误：服务端随后断开。清 token → 重连后走配对流程而非死循环 hello；
        // 已鉴权后的 error 是命令/冷却类错误，不得清 token
        if (_client.phase != ConnPhase.connected) {
          _clearToken();
          onNeedsPairing?.call();
        }
        break;
      default:
        break;
    }
    onMessage?.call(m);
  }

  Future<void> _saveToken(String? t) async {
    if (t == null || t.isEmpty) return;
    _token = t;
    final p = await SharedPreferences.getInstance();
    await p.setString(_kToken, t);
  }

  void _clearToken() {
    _token = null;
    SharedPreferences.getInstance().then((p) => p.remove(_kToken));
  }

  /// 用户主动断开（切换 PC / 退出）：停止重连
  Future<void> disconnectByUser() async {
    _userClosed = true;
    _reconnectTimer?.cancel();
    await _client.close();
  }
}
