// 主遥控页（§4.2 v3）：顶行三个同级纯黑方形按钮（logo/麦克风/播放上一条，
// 金色实心磨砂图标）+ 收藏列表（毛玻璃卡 + 金瘦体文字，再点停止 + 金色脉动）
// + 跳转顶/底悬浮钮 + 底部胶囊输入栏。
// logo 兼连接状态：suspicious=已连；点击 logo = 断开确认（原绿徽职责）。

import 'package:flutter/material.dart';

import '../proto/messages.dart';
import '../state/app_state.dart';
import '../theme.dart';
import 'grok_logo.dart';

class MainPage extends StatefulWidget {
  const MainPage({super.key, required this.app});

  final AppState app;

  @override
  State<MainPage> createState() => _MainPageState();
}

class _MainPageState extends State<MainPage> {
  final _input = TextEditingController();
  final _sc = ScrollController();
  bool _showJump = false;

  @override
  void initState() {
    super.initState();
    _sc.addListener(_onScroll);
  }

  @override
  void dispose() {
    _sc.removeListener(_onScroll);
    _sc.dispose();
    _input.dispose();
    super.dispose();
  }

  void _onScroll() {
    final v = _sc.hasClients && _sc.offset > 240;
    if (v != _showJump && mounted) setState(() => _showJump = v);
  }

  void _send() {
    final app = widget.app;
    if (app.synthesizing) return;
    final t = _input.text;
    if (t.trim().isEmpty) return;
    _input.clear();
    app.sendText(t);
  }

  @override
  Widget build(BuildContext context) {
    final app = widget.app;
    final pad = MediaQuery.of(context).padding;
    return Scaffold(
      backgroundColor: Colors.transparent,
      body: Stack(
        children: [
          const DiffuseBackground(),
          Column(
            children: [
              SizedBox(height: pad.top + 8),
              Padding(
                padding: const EdgeInsets.fromLTRB(16, 0, 16, 0),
                child: _topRow(app),
              ),
              const SizedBox(height: 12),
              Expanded(child: _list(app)),
              _bottomBar(app),
              SizedBox(height: pad.bottom + 10),
            ],
          ),
          if (_showJump)
            Positioned(
              right: 14,
              bottom: 130,
              child: Column(
                children: [
                  _jumpBtn('jump_top_f', () => _sc.animateTo(0,
                      duration: const Duration(milliseconds: 300),
                      curve: Curves.easeOut)),
                  const SizedBox(height: 10),
                  _jumpBtn('jump_bottom_f', () => _sc.animateTo(
                      _sc.position.maxScrollExtent,
                      duration: const Duration(milliseconds: 300),
                      curve: Curves.easeOut)),
                ],
              ),
            ),
        ],
      ),
    );
  }

  // ── 顶行：三个同级按钮（等大正方形并列占据顶行）──

  Widget _topRow(AppState app) => Row(
        children: [
          // logo：grok 金墨动态（WebView，黑底）；点击 = 断开确认（取代原绿徽）
          Expanded(
            child: _topTile(
              onTap: () => _confirmDisconnect(app),
              child: GrokLogo(
                size: 64,
                radius: 18,
                state: app.connected ? 'suspicious' : 'sleeping',
              ),
            ),
          ),
          const SizedBox(width: 10),
          Expanded(
            child: _topTile(
              onTap: app.toggleMic,
              child: RIcon(
                (app.state?.micSend ?? false) ? 'mic_on_f' : 'mic_off_f',
                size: 46,
              ),
            ),
          ),
          const SizedBox(width: 10),
          Expanded(
            child: _topTile(
              onTap: app.playLast,
              child: const RIcon('play_last_f', size: 46),
            ),
          ),
        ],
      );

  /// 同级按钮统一形制：纯黑方形（宽随行三等分）+ 居中内容
  Widget _topTile({required VoidCallback onTap, required Widget child}) =>
      PressScale(
        onTap: onTap,
        child: MatteBlack(
          radius: 18,
          child: AspectRatio(
            aspectRatio: 1,
            child: Center(child: child),
          ),
        ),
      );

  void _confirmDisconnect(AppState app) {
    showDialog<void>(
      context: context,
      builder: (ctx) => Dialog(
        backgroundColor: Colors.transparent,
        child: GlassCard(
          opacity: 0.92,
          padding: const EdgeInsets.all(20),
          child: Column(
            mainAxisSize: MainAxisSize.min,
            children: [
              const Text('断开连接？',
                  style: TextStyle(fontSize: 15, color: RT.ink)),
              const SizedBox(height: 16),
              Row(
                children: [
                  Expanded(
                    child: TextButton(
                      onPressed: () => Navigator.pop(ctx),
                      child: const Text('取消',
                          style: TextStyle(color: RT.sub)),
                    ),
                  ),
                  Expanded(
                    child: TextButton(
                      onPressed: () {
                        Navigator.pop(ctx);
                        app.disconnect();
                      },
                      child: const Text('断开',
                          style: TextStyle(color: RT.gold)),
                    ),
                  ),
                ],
              ),
            ],
          ),
        ),
      ),
    );
  }

  // ── 收藏列表 ──

  Widget _list(AppState app) {
    if (app.favorites.isEmpty) {
      return Center(
        child: GlassCard(
          padding: const EdgeInsets.all(28),
          child: Column(
            mainAxisSize: MainAxisSize.min,
            children: [
              const RIcon('empty_fav', size: 56),
              const SizedBox(height: 12),
              const Text('PC 端暂无收藏',
                  style: TextStyle(fontSize: 14, color: RT.ink)),
              const SizedBox(height: 6),
              const Text('在 PC 端右键消息即可收藏',
                  style: TextStyle(fontSize: 12, color: RT.sub)),
            ],
          ),
        ),
      );
    }
    return ListView.builder(
      controller: _sc,
      padding: const EdgeInsets.symmetric(horizontal: 16),
      itemCount: app.favorites.length,
      itemBuilder: (_, i) => _favCard(app, app.favorites[i]),
    );
  }

  Widget _favCard(AppState app, FavoriteItem f) {
    final playing = app.state?.playingId == f.id;
    final card = GlassCard(
      radius: 18,
      padding: const EdgeInsets.symmetric(horizontal: 16, vertical: 14),
      child: Row(
        children: [
          Expanded(
            child: Column(
              crossAxisAlignment: CrossAxisAlignment.start,
              children: [
                Text(
                  f.note,
                  maxLines: 2,
                  overflow: TextOverflow.ellipsis,
                  style: RT.thin.copyWith(
                      fontSize: 15,
                      fontWeight: FontWeight.w300,
                      color: RT.goldText),
                ),
                if (f.hotkey != null && f.hotkey!.isNotEmpty) ...[
                  const SizedBox(height: 4),
                  Text(f.hotkey!,
                      style: const TextStyle(
                          fontSize: 11, color: RT.goldTextDim)),
                ],
              ],
            ),
          ),
          if (playing)
            Container(
              width: 8,
              height: 8,
              decoration: const BoxDecoration(
                  shape: BoxShape.circle, color: RT.gold),
            ),
        ],
      ),
    );
    return Padding(
      padding: const EdgeInsets.only(bottom: 10),
      child: PressScale(
        onTap: () => app.tapFavorite(f),
        child: playing ? PulseGlow(child: card) : card,
      ),
    );
  }

  // ── 底部输入栏 ──

  Widget _bottomBar(AppState app) => Padding(
        padding: const EdgeInsets.fromLTRB(16, 0, 16, 0),
        child: GlassCard(
          radius: 28,
          opacity: 0.6,
          padding: const EdgeInsets.symmetric(horizontal: 8, vertical: 6),
          child: Row(
            children: [
              const SizedBox(width: 10),
              Expanded(
                child: TextField(
                  controller: _input,
                  enabled: !app.synthesizing,
                  onSubmitted: (_) => _send(),
                  style: const TextStyle(fontSize: 14, color: RT.ink),
                  decoration: InputDecoration(
                    border: InputBorder.none,
                    hintText:
                        app.synthesizing ? '合成中…' : '发消息 或 输入文字',
                    hintStyle:
                        const TextStyle(fontSize: 13, color: RT.sub),
                  ),
                ),
              ),
              PressScale(
                onTap: _send,
                child: Opacity(
                  opacity: app.synthesizing ? 0.4 : 1,
                  child: Padding(
                    padding: const EdgeInsets.all(10),
                    child: const RIcon('send_f', size: 24),
                  ),
                ),
              ),
            ],
          ),
        ),
      );

  Widget _jumpBtn(String icon, VoidCallback onTap) => PressScale(
        onTap: onTap,
        child: GlassCard(
          radius: 16,
          opacity: 0.6,
          padding: const EdgeInsets.all(13),
          child: RIcon(icon, size: 22),
        ),
      );
}
