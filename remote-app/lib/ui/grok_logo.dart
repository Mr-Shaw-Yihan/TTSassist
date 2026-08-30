// logo：内嵌透明 WebView 复用 PC 端 grok 引擎动态（设计文档 §4.2）。
// 状态随连接：connected → suspicious（常态），断开 → sleeping。
// 资产由 tools/sync_grok_engine.ps1 同步；grok 素材为演示占位，正式分发前换原创。

import 'package:flutter/material.dart';
import 'package:webview_flutter/webview_flutter.dart';

import '../theme.dart';

class GrokLogo extends StatefulWidget {
  const GrokLogo({
    super.key,
    this.size = 44,
    this.radius = 14,
    this.state = 'suspicious',
  });

  final double size;
  final double radius;

  /// grok 引擎姿态：suspicious（连接常态）/ sleeping（断开）
  final String state;

  @override
  State<GrokLogo> createState() => _GrokLogoState();
}

class _GrokLogoState extends State<GrokLogo> {
  late final WebViewController _ctl;
  bool _ready = false;

  @override
  void initState() {
    super.initState();
    _ctl = WebViewController()
      ..setJavaScriptMode(JavaScriptMode.unrestricted)
      ..setBackgroundColor(const Color(0x00000000))
      ..setNavigationDelegate(NavigationDelegate(
        onPageFinished: (_) {
          if (mounted) {
            setState(() => _ready = true);
            _applyState();
          }
        },
      ))
      ..loadFlutterAsset('assets/grok/grok_logo.html');
  }

  @override
  void didUpdateWidget(GrokLogo old) {
    super.didUpdateWidget(old);
    if (old.state != widget.state && _ready) _applyState();
  }

  void _applyState() {
    _ctl.runJavaScript('window.setGrokState && setGrokState("${widget.state}")');
  }

  @override
  Widget build(BuildContext context) {
    return SizedBox(
      width: widget.size,
      height: widget.size,
      child: Stack(
        children: [
          // 加载完成前的黑底占位（与 v3 黑色按钮面一致，金墨体防白闪）
          MatteBlack(radius: widget.radius, child: const SizedBox.expand()),
          if (_ready)
            ClipRRect(
              borderRadius: BorderRadius.circular(widget.radius),
              // WebView 是平台视图会吞掉触摸，logo 纯装饰（followPointer:false），
              // 必须忽略指针，否则外层按钮（断开确认）收不到点击
              child: IgnorePointer(
                child: WebViewWidget(controller: _ctl),
              ),
            ),
        ],
      ),
    );
  }
}
