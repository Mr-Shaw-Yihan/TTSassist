// 连接/配对页（§4.1 v3）：mDNS 发现列表（金瘦体条目）+ 手动 IP:端口。
// 有发现设备时只显示条目 + 「手动输入地址」开关，手动区默认折叠；
// 无设备时直接展示手动区。配对码 UI 已移除（v3 决策：仅免码配对，
// PC 弹窗确认为唯一配对路径；协议 §三 配对码仍保留于服务端）。

import 'package:flutter/material.dart';

import '../net/remote_client.dart';
import '../state/app_state.dart';
import '../theme.dart';
import 'grok_logo.dart';
import 'update_banner.dart';

class PairPage extends StatefulWidget {
  const PairPage({super.key, required this.app});

  final AppState app;

  @override
  State<PairPage> createState() => _PairPageState();
}

class _PairPageState extends State<PairPage> {
  final _ip = TextEditingController();
  final _port = TextEditingController(text: '45271');
  bool _manualOpen = false;

  @override
  void dispose() {
    _ip.dispose();
    _port.dispose();
    super.dispose();
  }

  String get _subtitle {
    switch (widget.app.phase) {
      case ConnPhase.connecting:
        return '连接中…';
      case ConnPhase.awaitingAuth:
        return '已连上，等待鉴权';
      case ConnPhase.connected:
        return '已连接';
      case ConnPhase.disconnected:
        return '未连接 · 选择发现的 PC 或手动输入';
    }
  }

  void _connectManual() {
    final host = _ip.text.trim();
    if (host.isEmpty) {
      widget.app.showToast('请输入 IP 地址');
      return;
    }
    widget.app.connectManual(host, port: int.tryParse(_port.text) ?? 45271);
  }

  @override
  Widget build(BuildContext context) {
    final app = widget.app;
    final hasDevice = app.discovered.isNotEmpty;
    return Scaffold(
      backgroundColor: Colors.transparent,
      body: Stack(
        children: [
          const DiffuseBackground(),
          Center(
            child: SingleChildScrollView(
              padding: const EdgeInsets.all(24),
              child: ConstrainedBox(
                constraints: const BoxConstraints(maxWidth: 380),
                child: GlassCard(
                  padding: const EdgeInsets.all(22),
                  child: Column(
                    mainAxisSize: MainAxisSize.min,
                    crossAxisAlignment: CrossAxisAlignment.stretch,
                    children: [
                      // 断开态 logo（sleeping）：黑底金墨，与主遥控页同一职责语言
                      Center(
                        child: GrokLogo(
                          size: 56,
                          radius: 18,
                          state:
                              app.connected ? 'suspicious' : 'sleeping',
                        ),
                      ),
                      if (app.updateInfo != null) ...[
                        const SizedBox(height: 10),
                        UpdateBanner(
                          info: app.updateInfo!,
                          onTap: () => showDialog(
                              context: context,
                              builder: (_) => const UpdateDialog()),
                        ),
                      ],
                      const SizedBox(height: 12),
                      // 标题：哑金瘦体
                      const Text(
                        '电子声带遥控',
                        style: TextStyle(
                          fontSize: 22,
                          fontWeight: FontWeight.w300,
                          letterSpacing: 2,
                          color: RT.goldText,
                        ),
                      ),
                      const SizedBox(height: 6),
                      Text(_subtitle,
                          style: const TextStyle(fontSize: 12, color: RT.sub)),
                      if (app.waitingConfirm) ...[
                        const SizedBox(height: 14),
                        GlassCard(
                          radius: 16,
                          padding: const EdgeInsets.all(16),
                          child: Row(
                            children: [
                              const SizedBox(
                                width: 18,
                                height: 18,
                                child: CircularProgressIndicator(
                                    strokeWidth: 2, color: RT.gold),
                              ),
                              const SizedBox(width: 12),
                              const Expanded(
                                child: Column(
                                  crossAxisAlignment:
                                      CrossAxisAlignment.start,
                                  children: [
                                    Text(
                                      '已连上，请在电脑屏幕上点击「允许」',
                                      style: TextStyle(
                                          fontSize: 14, color: RT.ink),
                                    ),
                                    SizedBox(height: 4),
                                    Text(
                                      'PC 无人确认时，请求 10 秒后自动超时',
                                      style: TextStyle(
                                          fontSize: 11, color: RT.sub),
                                    ),
                                  ],
                                ),
                              ),
                            ],
                          ),
                        ),
                      ],
                      if (hasDevice) ...[
                        const SizedBox(height: 16),
                        const Text('发现的电脑',
                            style:
                                TextStyle(fontSize: 11, color: RT.sub)),
                        const SizedBox(height: 6),
                        for (final pc in app.discovered)
                          Padding(
                            padding: const EdgeInsets.only(bottom: 8),
                            child: PressScale(
                              onTap: () => app.connectDiscovered(pc),
                              // 磨砂哑金按钮样式：金渐变 + 噪点（区别于白色毛玻璃
                              // 与黑色主按钮），金底文字用深墨保证可读
                              child: FrostedGoldTile(
                                radius: 16,
                                child: Padding(
                                  padding: const EdgeInsets.symmetric(
                                      horizontal: 14, vertical: 13),
                                  child: Row(
                                    children: [
                                      Expanded(
                                        child: Text(pc.name,
                                            maxLines: 1,
                                            overflow: TextOverflow.ellipsis,
                                            style: const TextStyle(
                                                fontSize: 14,
                                                fontWeight: FontWeight.w600,
                                                color: RT.ink)),
                                      ),
                                      Text('${pc.host}:${pc.port}',
                                          style: TextStyle(
                                              fontSize: 11,
                                              color: RT.ink
                                                  .withValues(alpha: 0.62))),
                                    ],
                                  ),
                                ),
                              ),
                            ),
                          ),
                        // 手动输入开关：有设备时手动区默认折叠
                        Padding(
                          padding: const EdgeInsets.only(top: 2),
                          child: PressScale(
                            onTap: () =>
                                setState(() => _manualOpen = !_manualOpen),
                            child: GlassCard(
                              radius: 16,
                              padding: const EdgeInsets.symmetric(
                                  vertical: 13),
                              child: Text(
                                _manualOpen ? '收起手动输入' : '手动输入地址',
                                textAlign: TextAlign.center,
                                style: const TextStyle(
                                    fontSize: 13,
                                    letterSpacing: 1,
                                    color: RT.gold),
                              ),
                            ),
                          ),
                        ),
                      ],
                      // 手动区：无设备直接展示；有设备点开折叠后展示
                      if (!hasDevice || _manualOpen) ...[
                        const SizedBox(height: 16),
                        Row(
                          children: [
                            Expanded(
                              child: TextField(
                                controller: _ip,
                                style: const TextStyle(
                                    fontSize: 14, color: RT.ink),
                                decoration: _field('IP 地址（如 192.168.1.5）'),
                              ),
                            ),
                            const SizedBox(width: 10),
                            SizedBox(
                              width: 74,
                              child: TextField(
                                controller: _port,
                                keyboardType: TextInputType.number,
                                style: const TextStyle(
                                    fontSize: 14, color: RT.ink),
                                decoration: _field('端口'),
                              ),
                            ),
                          ],
                        ),
                        const SizedBox(height: 10),
                        _blackButton('连接', _connectManual),
                      ],
                    ],
                  ),
                ),
              ),
            ),
          ),
        ],
      ),
    );
  }

  InputDecoration _field(String hint) => InputDecoration(
        hintText: hint,
        hintStyle: const TextStyle(fontSize: 13, color: RT.sub),
        contentPadding:
            const EdgeInsets.symmetric(horizontal: 14, vertical: 12),
        filled: true,
        fillColor: Colors.white.withValues(alpha: 0.55),
        border: OutlineInputBorder(
          borderRadius: BorderRadius.circular(14),
          borderSide: BorderSide.none,
        ),
      );

  Widget _blackButton(String label, VoidCallback onTap) => PressScale(
        onTap: onTap,
        child: MatteBlack(
          radius: 16,
          child: SizedBox(
            height: 46,
            child: Center(
              child: Text(
                label,
                style: const TextStyle(
                    fontSize: 15,
                    fontWeight: FontWeight.w400,
                    letterSpacing: 3,
                    color: RT.onBlack),
              ),
            ),
          ),
        ),
      );
}
