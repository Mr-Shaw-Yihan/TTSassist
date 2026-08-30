// 状态层：state 单一数据源 + event 驱动刷新（设计文档 §3.3/§3.4）。
// UI 只读本类并调本类动作；所有 s2c 消息在此归一。

import 'dart:async';

import 'package:device_info_plus/device_info_plus.dart';
import 'package:flutter/foundation.dart';

import '../net/discovery.dart';
import '../net/remote_client.dart';
import '../net/session.dart';
import '../net/updater.dart';
import '../proto/messages.dart';
import '../ui/update_banner.dart';

class AppState extends ChangeNotifier {
  AppState() {
    session.onPhase = _onPhase;
    session.onMessage = _onMessage;
    session.onNeedsPairing = () {
      needsPairing = true;
      notifyListeners();
    };
    discovery.onChange = (list) {
      discovered = list;
      // 小白体验：发现 PC 且当前未连接 → 自动连第一台（手动断开后不再自动连）
      if (_autoConnect &&
          phase == ConnPhase.disconnected &&
          list.isNotEmpty) {
        connectDiscovered(list.first);
      }
      notifyListeners();
    };
  }

  final session = RemoteSession();
  final discovery = Discovery();

  // ── 单一数据源 ──
  ConnPhase get phase => session.phase;
  RemoteState? state;
  List<FavoriteItem> favorites = [];
  List<DiscoveredPc> discovered = [];
  bool needsPairing = false;
  /// 已发 pair_request，等 PC 弹窗确认中（超时/被拒回退配对码 UI）
  bool waitingConfirm = false;
  bool _autoConnect = true;
  Timer? _confirmTimer;
  String deviceName = '移动设备';
  String? toast;
  Timer? _toastTimer;
  /// 有新版本时的远端更新信息（启动静默检查；null = 无更新或检查失败）
  UpdateInfo? updateInfo;

  bool get connected => phase == ConnPhase.connected;
  bool get synthesizing => state?.synthesizing ?? false;

  /// 启动：恢复持久化 → 有上次 PC 自动连，否则配对页；同时开始发现
  Future<void> init() async {
    // 设备名用于 PC 端确认弹窗展示（「允许 XX 遥控？」）
    try {
      final info = await DeviceInfoPlugin().androidInfo;
      deviceName = '${info.brand} ${info.model}'.trim();
      if (deviceName.isEmpty) deviceName = '移动设备';
    } catch (_) {}
    final hasSaved = await session.init();
    await discovery.start();
    if (hasSaved) {
      await session.connectSaved();
    } else {
      needsPairing = true;
      notifyListeners();
    }
    // 应用内更新：静默检查（失败不打扰）；有新版本时通知 UI 显示更新条
    checkForUpdate().then((info) {
      if (info != null) {
        updateInfo = info;
        notifyListeners();
      }
    });
  }

  void _onPhase(ConnPhase p) {
    if (p == ConnPhase.connected) {
      needsPairing = false;
      waitingConfirm = false;
      _confirmTimer?.cancel();
      // 鉴权成功后服务端会立即推一帧 state；收藏主动拉一次
      session.client.listFavorites().catchError((_) {});
      discovery.stop(); // 已连上，释放 mDNS
    } else if (p == ConnPhase.awaitingAuth) {
      // 无 token 的新连接 → 自动发免码配对请求（PC 弹窗确认）
      if (!session.usedHello) _startPairRequest();
    } else if (p == ConnPhase.disconnected) {
      waitingConfirm = false;
      _confirmTimer?.cancel();
    }
    notifyListeners();
  }

  /// 免码配对：发 pair_request + 10s 超时回退配对码 UI
  void _startPairRequest() {
    waitingConfirm = true;
    _confirmTimer?.cancel();
    _confirmTimer = Timer(const Duration(seconds: 10), () {
      if (waitingConfirm) {
        waitingConfirm = false;
        notifyListeners();
      }
    });
    session.client.pairRequest(deviceName);
    notifyListeners();
  }

  void _onMessage(S2C m) {
    switch (m.t) {
      case 'state':
        state = m.state ?? state;
        break;
      case 'favorites':
        favorites = m.items ?? const [];
        break;
      case 'event':
        // 收藏变化 → 重拉列表；settings/playback 变化 → 服务端随后推 state 帧
        if (m.eventType == 'favorites_changed') {
          session.client.listFavorites().catchError((_) {});
        }
        break;
      case 'ack':
        if (!m.ok) showToast(m.err ?? '操作失败');
        break;
      case 'error':
        // 免码配对被拒/超时类错误 → 回退手动输入
        if (waitingConfirm) {
          waitingConfirm = false;
          _confirmTimer?.cancel();
        }
        showToast(m.err ?? '连接错误');
        break;
      default:
        break;
    }
    notifyListeners();
  }

  void showToast(String text) {
    toast = text;
    _toastTimer?.cancel();
    _toastTimer = Timer(const Duration(milliseconds: 2500), () {
      toast = null;
      notifyListeners();
    });
    notifyListeners();
  }

  // ── UI 动作 ──

  /// 收藏卡片：播放中再点 = 停止（§4.2）
  Future<void> tapFavorite(FavoriteItem f) async {
    try {
      if (state?.playingId == f.id) {
        await session.client.stop();
      } else {
        await session.client.playFavorite(f.id);
      }
    } catch (e) {
      showToast('$e');
    }
  }

  Future<void> sendText(String text) async {
    final t = text.trim();
    if (t.isEmpty) return;
    try {
      final ack = await session.client.synthesize(t);
      if (!ack.ok) showToast(ack.err ?? '合成失败');
    } catch (e) {
      showToast('$e');
    }
  }

  Future<void> toggleMic() async {
    try {
      await session.client.toggleMic();
    } catch (e) {
      showToast('$e');
    }
  }

  Future<void> playLast() async {
    try {
      await session.client.playLast();
    } catch (e) {
      showToast('$e');
    }
  }

  /// logo 点击 = 刷新收藏（§4.2）
  Future<void> refreshFavorites() async {
    try {
      await session.client.listFavorites();
    } catch (e) {
      showToast('$e');
    }
  }

  // ── 配对页动作 ──

  Future<void> connectManual(String host, {int port = 45271}) async {
    needsPairing = false;
    notifyListeners();
    await session.connectManual(host.trim(), port: port);
  }

  Future<void> connectDiscovered(DiscoveredPc pc) async {
    needsPairing = false;
    notifyListeners();
    await session.connectManual(pc.host, port: pc.port);
  }

  /// 连接管理：主动断开回配对页（停止自动连接，避免马上又连回去）
  Future<void> disconnect() async {
    _autoConnect = false;
    await session.disconnectByUser();
    needsPairing = true;
    await discovery.start();
    notifyListeners();
  }

  @override
  void dispose() {
    _toastTimer?.cancel();
    _confirmTimer?.cancel();
    discovery.stop();
    super.dispose();
  }
}
