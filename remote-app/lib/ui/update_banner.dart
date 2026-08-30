// 应用内更新 UI：发现新版本时在页顶显示毛玻璃更新条；点击弹下载卡
// （金色进度条，Gitee/GitHub 通道自动回退），完成后调起系统安装器。

import 'package:flutter/material.dart';
import 'package:flutter/services.dart';

import '../net/updater.dart';
import '../theme.dart';

/// 页顶更新条：发现新版本时显示
class UpdateBanner extends StatelessWidget {
  const UpdateBanner({super.key, required this.info, required this.onTap});

  final UpdateInfo info;
  final VoidCallback onTap;

  @override
  Widget build(BuildContext context) {
    return Padding(
      padding: const EdgeInsets.fromLTRB(16, 8, 16, 0),
      child: GestureDetector(
        behavior: HitTestBehavior.opaque,
        onTap: onTap,
        child: GlassCard(
          radius: 14,
          padding: const EdgeInsets.symmetric(horizontal: 14, vertical: 9),
          child: Row(
            children: [
              const Expanded(
                child: Text(
                  '发现新版本，点击下载',
                  style: TextStyle(
                      fontSize: 12,
                      letterSpacing: 0.5,
                      color: RT.goldText),
                ),
              ),
              Text('v${info.version}',
                  style: const TextStyle(
                      fontSize: 12,
                      fontWeight: FontWeight.w300,
                      color: RT.gold)),
            ],
          ),
        ),
      ),
    );
  }
}

/// 更新流程弹窗：检查远端版本 → 下载进度 → 调起安装。
/// 返回前保持自管理状态；调用方只需 showDialog 触发。
class UpdateDialog extends StatefulWidget {
  const UpdateDialog({super.key});

  @override
  State<UpdateDialog> createState() => _UpdateDialogState();
}

enum _Phase { checking, available, downloading, done, error }

class _UpdateDialogState extends State<UpdateDialog> {
  _Phase _phase = _Phase.checking;
  UpdateInfo? _info;
  double _progress = 0;
  String _channel = '';
  String _error = '';
  String? _apkPath;
  static const _installChannel =
      MethodChannel('com.voiceassist.remote/update');

  @override
  void initState() {
    super.initState();
    _check();
  }

  Future<void> _check() async {
    try {
      final info = await fetchRemoteUpdateInfo();
      final localVersion = await nativeAppVersion();
      if (!mounted) return;
      if (!isNewerVersion(info.version, localVersion)) {
        setState(() {
          _phase = _Phase.error;
          _error = '已是最新版本（v$localVersion）';
        });
        return;
      }
      setState(() {
        _phase = _Phase.available;
        _info = info;
      });
    } catch (e) {
      if (!mounted) return;
      setState(() {
        _phase = _Phase.error;
        _error = '$e';
      });
    }
  }

  Future<void> _download() async {
    final info = _info;
    if (info == null) return;
    setState(() {
      _phase = _Phase.downloading;
      _progress = 0;
    });
    try {
      final path = await downloadApk(info, onProgress: (p, ch) {
        if (mounted) {
          setState(() {
            if (p >= 0) _progress = p;
            _channel = ch;
          });
        }
      });
      if (!mounted) return;
      setState(() {
        _apkPath = path;
        _phase = _Phase.done;
      });
      await _install();
    } catch (e) {
      if (!mounted) return;
      setState(() {
        _phase = _Phase.error;
        _error = '$e';
      });
    }
  }

  Future<void> _install() async {
    final path = _apkPath;
    if (path == null) return;
    try {
      await _installChannel
          .invokeMethod('installApk', {'path': path});
    } on PlatformException catch (e) {
      if (!mounted) return;
      setState(() {
        _phase = _Phase.error;
        _error = '调起安装失败：${e.message}';
      });
    }
  }

  @override
  Widget build(BuildContext context) {
    return Dialog(
      backgroundColor: Colors.transparent,
      child: GlassCard(
        opacity: 0.92,
        padding: const EdgeInsets.all(20),
        child: Column(
          mainAxisSize: MainAxisSize.min,
          crossAxisAlignment: CrossAxisAlignment.stretch,
          children: [
            const Text('软件更新',
                textAlign: TextAlign.center,
                style: TextStyle(
                    fontSize: 15, letterSpacing: 1, color: RT.ink)),
            const SizedBox(height: 14),
            _body(),
            const SizedBox(height: 14),
            Row(
              children: [
                Expanded(
                  child: TextButton(
                    onPressed: () => Navigator.pop(context),
                    child: const Text('稍后',
                        style: TextStyle(color: RT.sub)),
                  ),
                ),
                if (_phase == _Phase.available && _error.isEmpty) ...[
                  Expanded(
                    child: TextButton(
                      onPressed: _download,
                      child: const Text('立即下载',
                          style: TextStyle(color: RT.gold)),
                    ),
                  ),
                ],
                if (_phase == _Phase.done) ...[
                  Expanded(
                    child: TextButton(
                      onPressed: _install,
                      child: const Text('立即安装',
                          style: TextStyle(color: RT.gold)),
                    ),
                  ),
                ],
                if (_phase == _Phase.error && _error.startsWith('已是最新')) ...[
                  Expanded(
                    child: TextButton(
                      onPressed: () => Navigator.pop(context),
                      child: const Text('好',
                          style: TextStyle(color: RT.gold)),
                    ),
                  ),
                ],
              ],
            ),
          ],
        ),
      ),
    );
  }

  Widget _body() {
    switch (_phase) {
      case _Phase.checking:
        return const Row(
          mainAxisAlignment: MainAxisAlignment.center,
          children: [
            SizedBox(
                width: 18,
                height: 18,
                child: CircularProgressIndicator(
                    strokeWidth: 2, color: RT.gold)),
            SizedBox(width: 12),
            Text('正在检查更新…',
                style: TextStyle(fontSize: 13, color: RT.sub)),
          ],
        );
      case _Phase.available:
        final info = _info!;
        return Column(
          crossAxisAlignment: CrossAxisAlignment.start,
          children: [
            Text('新版本 v${info.version}',
                style: const TextStyle(
                    fontSize: 14,
                    fontWeight: FontWeight.w300,
                    color: RT.ink)),
            if (info.notes != null && info.notes!.isNotEmpty) ...[
              const SizedBox(height: 6),
              Text(info.notes!,
                  style: const TextStyle(fontSize: 12, color: RT.sub)),
            ],
          ],
        );
      case _Phase.downloading:
        return Column(
          crossAxisAlignment: CrossAxisAlignment.start,
          children: [
            ClipRRect(
              borderRadius: BorderRadius.circular(4),
              child: LinearProgressIndicator(
                value: _progress > 0 ? _progress : null,
                minHeight: 6,
                backgroundColor: RT.gold.withValues(alpha: 0.15),
                color: RT.gold,
              ),
            ),
            const SizedBox(height: 8),
            Text(
              _progress > 0
                  ? '下载中 ${(_progress * 100).toStringAsFixed(0)}% · $_channel 通道'
                  : '下载中… · $_channel 通道',
              style: const TextStyle(fontSize: 12, color: RT.sub),
            ),
          ],
        );
      case _Phase.done:
        return const Text('下载完成，正在调起安装…\n如系统询问「允许安装」，请选择允许。',
            style: TextStyle(fontSize: 13, color: RT.ink));
      case _Phase.error:
        return Text(_error,
            style: const TextStyle(fontSize: 13, color: RT.sub));
    }
  }
}

/// 便捷：从原生通道读当前 App 版本（MainActivity/appVersion）。
Future<String> nativeAppVersion() async {
  const channel = MethodChannel('com.voiceassist.remote/update');
  return await channel.invokeMethod<String>('appVersion') ?? '0.0.0';
}

/// 供 app_state 判断是否提示更新（静默检查，失败不打扰用户）。
Future<UpdateInfo?> checkForUpdate() async {
  try {
    final remote = await fetchRemoteUpdateInfo();
    final local = await nativeAppVersion();
    if (isNewerVersion(remote.version, local)) return remote;
    return null;
  } catch (_) {
    return null;
  }
}
