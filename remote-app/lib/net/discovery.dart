// 发现层：mDNS 解析 _ttsassist-remote._tcp（PC 端 lan-remote 插件广播，实例名=计算机名）。
// 手动 IP:端口兜底在配对页直接走 session.connectManual，不经本模块。
// 模拟器桥接：安卓模拟器在隔离虚拟网络，收不到真实局域网的 mDNS 广播；
// 检测到模拟器时周期探测 10.0.2.2（模拟器对宿主机的固定别名），端口通即
// 登记进发现列表，让模拟器也走免码自动配对体验。
// 子网扫描兜底（2026-08-30）：Windows 上 UDP 5353 被 Steam/QQ 等多进程抢占
// （组播包只投递给最后绑定者），mdns-sd 监听会被顶掉导致 mDNS 静默失效。
// 因此真机也周期扫描本机各网段的 45271 端口，作为与 mDNS 并行的发现通道。

import 'dart:async';
import 'dart:convert';
import 'dart:io';

import 'package:device_info_plus/device_info_plus.dart';
import 'package:flutter/foundation.dart';
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
  /// 超时 700ms（VPN TUN/代理环境握手延迟大，300ms 会误杀真 PC）。
  /// 排除本机自身 IP 与虚拟隧道接口（adb reverse / VPN TUN 自命中等）。
  /// 命中后发极简 HTTP 探测复核：WS 服务端回 HTTP 状态行才算真命中。
  Future<void> _subnetScan() async {
    if (_scanning) return;
    _scanning = true;
    try {
      final selfIps = <String>{};
      final bases = <String>{};
      final interfaces = await NetworkInterface.list();
      final ifLog = <String>[];
      for (final itf in interfaces) {
        // 跳过虚拟隧道接口（tun0/ppp/wg…）：VPN/代理 TUN 模式下其对任意地址的
        // TCP connect 都会被代理栈假握手成功，产生「扫到了假 PC」的误报
        final n = itf.name.toLowerCase();
        final skipped = n.startsWith('tun') || n.startsWith('ppp') || n.startsWith('wg');
        final v4 = itf.addresses
            .where((a) => a.type == InternetAddressType.IPv4)
            .map((a) => a.address)
            .join(',');
        ifLog.add('${itf.name}=$v4${skipped ? '[跳过:虚拟隧道]' : ''}');
        if (skipped) continue;
        for (final addr in itf.addresses) {
          if (addr.type != InternetAddressType.IPv4) continue;
          if (addr.isLoopback || addr.isLinkLocal) continue;
          selfIps.add(addr.address);
          final o = addr.address.split('.');
          if (o.length != 4) continue;
          bases.add('${o[0]}.${o[1]}.${o[2]}.');
        }
      }
      debugPrint('[discovery] 接口: ${ifLog.join(' | ')}');
      final probeIps = <String>[];
      for (final base in bases) {
        probeIps.addAll(List.generate(255, (i) => '$base${i + 1}'));
      }
      debugPrint(
          '[discovery] 扫描 ${bases.length} 个网段 / ${probeIps.length - selfIps.length} 个地址');
      final hits = <DiscoveredPc>[];
      // 分批并发：508 个同时 connect 会淹 WiFi 栈（ARP/SYN 大量丢失），
      // 每批 64 个、批间 60ms，总时长仍 <2s
      const batch = 64;
      for (var i = 0; i < probeIps.length; i += batch) {
        final slice = probeIps.skip(i).take(batch).toList();
        await Future.wait(slice.map((ip) async {
          if (selfIps.contains(ip)) return;
          try {
            final s = await Socket.connect(
              ip,
              45271,
              timeout: const Duration(milliseconds: 700),
            );
            // 二次复核：端口通不等于电子声带在跑（VPN TUN 等会假握手、
            // 其他设备也可能开着 45271）。发 WebSocket 升级握手——
            // 电子声带的 WS 服务端必然回 101 Switching Protocols，其余不会
            final isReal = await _verifyWsService(s);
            s.destroy();
            debugPrint(
                '[discovery] 端口命中 $ip → HTTP复核: ${isReal ? "通过" : "未通过(剔除)"}');
            if (isReal) {
              hits.add(DiscoveredPc(name: '局域网电脑', host: ip, port: 45271));
            }
          } catch (e) {
            // 非「目标不可达/超时」类异常打出来（定位真机 connect 失败原因）
            final msg = '$e';
            if (!msg.contains('timed out') &&
                !msg.contains('Connection refused') &&
                !msg.contains('No route to host') &&
                !msg.contains('Network is unreachable') &&
                !msg.contains('Connection timed out')) {
              debugPrint('[discovery] connect $ip 异常: $msg');
            }
          }
        }));
        if (i + batch < probeIps.length) {
          await Future<void>.delayed(const Duration(milliseconds: 60));
        }
      }
      debugPrint('[discovery] 本轮命中: ${hits.map((h) => h.host).join(", ")}');
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

  /// WebSocket 升级握手复核：发合法 upgrade 请求，
  /// 电子声带（tokio-tungstenite）必然回 `HTTP/1.1 101`，才算真命中。
  /// （裸 GET 复核已废弃：tungstenite 对非 upgrade 请求不回可读响应，会误杀真 PC）
  Future<bool> _verifyWsService(Socket s) async {
    try {
      s.add(ascii.encode(
          'GET / HTTP/1.1\r\nHost: probe\r\nUpgrade: websocket\r\n'
          'Connection: Upgrade\r\nSec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==\r\n'
          'Sec-WebSocket-Version: 13\r\n\r\n'));
      final data = await s.first
          .timeout(const Duration(milliseconds: 1000), onTimeout: () => Uint8List(0));
      if (data.isEmpty) return false;
      final head = String.fromCharCodes(data.take(40));
      return head.startsWith('HTTP/1.1 101');
    } catch (_) {
      return false;
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
