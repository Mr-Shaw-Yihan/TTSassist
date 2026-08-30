// 发现层：mDNS 解析 _ttsassist-remote._tcp（PC 端 lan-remote 插件广播，实例名=计算机名）。
// 手动 IP:端口兜底在配对页直接走 session.connectManual，不经本模块。
// 模拟器桥接：安卓模拟器在隔离虚拟网络，收不到真实局域网的 mDNS 广播；
// 检测到模拟器时周期探测 10.0.2.2（模拟器对宿主机的固定别名），端口通即
// 登记进发现列表，让模拟器也走免码自动配对体验。
// 子网扫描兜底（2026-08-30）：Windows 上 UDP 5353 被 Steam/QQ 等多进程抢占
// （组播包只投递给最后绑定者），mdns-sd 监听会被顶掉导致 mDNS 静默失效。
// 因此真机也周期扫描本机各网段的 45271 端口，作为与 mDNS 并行的发现通道。

import 'dart:async';
import 'dart:io';

import 'package:device_info_plus/device_info_plus.dart';
import 'package:nsd/nsd.dart' as nsd;

/// 发现的 PC（name=计算机名，host/port 为 WS 连接目标）
class DiscoveredPc {
  const DiscoveredPc({required this.name, required this.host, required this.port});

  final String name;
  final String host;
  final int port;

  @override
  bool operator ==(Object other) =>
      other is DiscoveredPc && other.host == host && other.port == port;

  @override
  int get hashCode => Object.hash(host, port);
}

class Discovery {
  /// 发现列表变化（含增删）
  void Function(List<DiscoveredPc> found)? onChange;

  nsd.Discovery? _discovery;
  List<DiscoveredPc> _mdnsFound = [];
  DiscoveredPc? _bridge;
  List<DiscoveredPc> _subnetHits = const [];
  Timer? _bridgeTimer;
  Timer? _subnetTimer;
  bool _scanning = false;
  bool _started = false;

  List<DiscoveredPc> get found =>
      [..._mdnsFound, ?_bridge, ..._subnetHits];

  /// 配对页进入时调用；重复调用无副作用
  Future<void> start() async {
    if (_started) return;
    _started = true;
    try {
      final d = await nsd.startDiscovery('_ttsassist-remote._tcp.');
      _discovery = d;
      d.addListener(_syncMdns);
      _syncMdns();
    } catch (_) {
      // mDNS 不可用（权限等）：静默，走子网扫描/手动 IP 兜底
    }
    // 子网扫描：所有平台都跑（mDNS 在 PC 端口被抢占时失效，见文件头注释）
    await _subnetScan();
    _subnetTimer = Timer.periodic(
        const Duration(seconds: 5), (_) => _subnetScan());
    if (await _isEmulator()) {
      await _probeBridge();
      _bridgeTimer =
          Timer.periodic(const Duration(seconds: 3), (_) => _probeBridge());
    }
  }

  /// 模拟器检测：虚拟设备才启用宿主桥接探测（真机走 mDNS + 子网扫描）
  Future<bool> _isEmulator() async {
    try {
      final info = await DeviceInfoPlugin().androidInfo;
      return !info.isPhysicalDevice;
    } catch (_) {
      return false;
    }
  }

  /// 探测模拟器内宿主机别名地址；端口通即认为宿主的电子声带在运行。
  /// 优先 127.0.0.1（adb reverse 隧道，绕开 qemu NAT 的 WS 握手问题），
  /// 兜底 10.0.2.2（模拟器宿主机别名）。
  Future<void> _probeBridge() async {
    String? upHost;
    for (final h in const ['127.0.0.1', '10.0.2.2']) {
      try {
        final s = await Socket.connect(
          h,
          45271,
          timeout: const Duration(milliseconds: 600),
        );
        upHost = h;
        await s.close();
        break;
      } catch (_) {}
    }
    final pc = upHost != null
        ? DiscoveredPc(
            name: '本机电脑（模拟器桥接）', host: upHost, port: 45271)
        : null;
    if (pc != _bridge) {
      _bridge = pc;
      _emit();
    }
  }

  /// 子网扫描兜底：对本机每个非环回 IPv4 所在 /24 并发探测 45271。
  /// 一轮 256 个 connect、单发 300ms 超时，约 1~2 秒完成；5 秒一轮成本可接受。
  /// 排除本机自身 IP（adb reverse 等工具会在手机端开监听端口造成自命中）。
  Future<void> _subnetScan() async {
    if (_scanning) return;
    _scanning = true;
    try {
      final selfIps = <String>{};
      final bases = <String>{};
      final interfaces = await NetworkInterface.list();
      for (final itf in interfaces) {
        for (final addr in itf.addresses) {
          if (addr.type != InternetAddressType.IPv4) continue;
          if (addr.isLoopback || addr.isLinkLocal) continue;
          selfIps.add(addr.address);
          final o = addr.address.split('.');
          if (o.length != 4) continue;
          bases.add('${o[0]}.${o[1]}.${o[2]}.');
        }
      }
      final hits = <DiscoveredPc>[];
      await Future.wait(bases.expand((base) {
        return List.generate(255, (i) => '$base${i + 1}').map((ip) async {
          if (selfIps.contains(ip)) return;
          try {
            final s = await Socket.connect(
              ip,
              45271,
              timeout: const Duration(milliseconds: 300),
            );
            hits.add(DiscoveredPc(name: '局域网电脑', host: ip, port: 45271));
            s.destroy();
          } catch (_) {}
        });
      }));
      // 稳定排序：IP 升序，避免每轮列表顺序抖动
      hits.sort((a, b) => a.host.compareTo(b.host));
      final list = hits.isEmpty ? null : hits;
      final changed = (list == null) != (_subnetHits.isEmpty) ||
          list?.any((p) => !_subnetHits.contains(p)) == true ||
          _subnetHits.any((p) => list?.contains(p) != true);
      if (changed) {
        _subnetHits = list ?? const [];
        _emit();
      }
    } catch (_) {
      // 网卡枚举失败：静默，下一轮再试
    } finally {
      _scanning = false;
    }
  }

  void _syncMdns() {
    final d = _discovery;
    if (d == null) return;
    _mdnsFound = d.services
        .where((s) => s.host != null && s.port != null)
        .map((s) => DiscoveredPc(
              name: (s.name != null && s.name!.isNotEmpty)
                  ? s.name!
                  : (s.host ?? 'PC'),
              host: s.host!,
              port: s.port!,
            ))
        .toList();
    _emit();
  }

  void _emit() => onChange?.call(found);

  /// 连接建立后调用，释放发现资源（设计：发现仅服务于配对页）
  Future<void> stop() async {
    _started = false;
    _subnetTimer?.cancel();
    _subnetTimer = null;
    _subnetHits = const [];
    _bridgeTimer?.cancel();
    _bridgeTimer = null;
    _bridge = null;
    final d = _discovery;
    _discovery = null;
    if (d != null) {
      d.removeListener(_syncMdns);
      try {
        await nsd.stopDiscovery(d);
      } catch (_) {}
    }
  }
}
