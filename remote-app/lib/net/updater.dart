// 应用内更新：版本检查 + APK 下载 + 调起系统安装器（设计：dist 分支 version.json
// 为版本清单，Gitee raw 优先、GitHub raw 回退；APK 下载优先 Gitee Release 附件、
// GitHub Release 回退——国内用户走 Gitee 通道）。

import 'dart:convert';
import 'dart:io';

/// version.json 内容（remote-app 发布时随 dist 分支更新）
class UpdateInfo {
  const UpdateInfo({
    required this.version,
    required this.apk,
    required this.giteeUrl,
    required this.githubUrl,
    this.notes,
  });

  final String version;
  final String apk;
  final String giteeUrl;
  final String githubUrl;
  final String? notes;

  static UpdateInfo fromJson(Map<String, dynamic> j) => UpdateInfo(
        version: j['version'] as String? ?? '',
        apk: j['apk'] as String? ?? '',
        giteeUrl: j['gitee_url'] as String? ?? '',
        githubUrl: j['github_url'] as String? ?? '',
        notes: j['notes'] as String?,
      );
}

class UpdateException implements Exception {
  const UpdateException(this.message);
  final String message;
  @override
  String toString() => message;
}

/// 三段版本比较：返回 true 当 remote > local（相等/回退均不提示）
bool isNewerVersion(String remote, String local) {
  List<int> parse(String v) => v
      .split('+')
      .first
      .split('.')
      .map((s) => int.tryParse(s) ?? 0)
      .toList();
  final a = parse(remote);
  final b = parse(local);
  for (var i = 0; i < 3; i++) {
    final x = i < a.length ? a[i] : 0;
    final y = i < b.length ? b[i] : 0;
    if (x != y) return x > y;
  }
  return false;
}

const _versionUrls = [
  // Gitee raw 优先（国内可达）；GitHub raw 回退
  'https://gitee.com/yihwan/TTSassist/raw/dist/remote-app-version.json',
  'https://raw.githubusercontent.com/Mr-Shaw-Yihan/TTSassist/dist/remote-app-version.json',
];

/// 拉取远端版本清单；全部通道失败抛 UpdateException
Future<UpdateInfo> fetchRemoteUpdateInfo() async {
  Object? lastErr;
  for (final url in _versionUrls) {
    try {
      final text = await _httpGet(url, timeout: const Duration(seconds: 6));
      return UpdateInfo.fromJson(
          (jsonDecode(text) as Map).cast<String, dynamic>());
    } catch (e) {
      lastErr = e;
    }
  }
  throw UpdateException('检查更新失败：$lastErr');
}

/// 下载 APK 到临时目录（依次尝试 info.giteeUrl / info.githubUrl），
/// onProgress: 0.0~1.0。返回本地文件路径。
Future<String> downloadApk(
  UpdateInfo info, {
  void Function(double progress, String channel)? onProgress,
}) async {
  final urls = <(String, String)>[
    if (info.giteeUrl.isNotEmpty) ('Gitee', info.giteeUrl),
    if (info.githubUrl.isNotEmpty) ('GitHub', info.githubUrl),
  ];
  if (urls.isEmpty) throw const UpdateException('没有可用的下载地址');

  Object? lastErr;
  for (final (channel, url) in urls) {
    try {
      final path = await _download(url, channel, onProgress);
      return path;
    } catch (e) {
      lastErr = e;
      onProgress?.call(0, channel);
    }
  }
  throw UpdateException('下载失败：$lastErr');
}

// ── 底层 HTTP（dart:io HttpClient，零第三方依赖） ──

Future<String> _httpGet(String url, {required Duration timeout}) async {
  final client = HttpClient();
  try {
    final req =
        await client.getUrl(Uri.parse(url)).timeout(timeout);
    final resp = await req.close().timeout(timeout);
    if (resp.statusCode != 200) {
      throw HttpException('HTTP ${resp.statusCode}');
    }
    return await resp.transform(utf8.decoder).join().timeout(timeout);
  } finally {
    client.close(force: true);
  }
}

Future<String> _download(
  String url,
  String channel,
  void Function(double, String)? onProgress,
) async {
  final client = HttpClient();
  try {
    final req = await client.getUrl(Uri.parse(url));
    final resp = await req.close();
    if (resp.statusCode != 200) {
      throw HttpException('HTTP ${resp.statusCode}');
    }
    final total = resp.contentLength > 0 ? resp.contentLength : -1;
    final dir = await _updateDir();
    final file = File('${dir.path}/update.apk');
    final sink = file.openWrite();
    var received = 0;
    await for (final chunk in resp) {
      received += chunk.length;
      sink.add(chunk);
      if (total > 0) {
        onProgress?.call(received / total, channel);
      } else {
        onProgress?.call(-1, channel); // 未知总长：进度不确定
      }
    }
    await sink.flush();
    await sink.close();
    if (total > 0) onProgress?.call(1.0, channel);
    return file.path;
  } finally {
    client.close(force: true);
  }
}

Future<Directory> _updateDir() async {
  // 应用内部缓存（原生安装时由 Kotlin 侧复制到 externalCacheDir 供 FileProvider 共享）
  final dir = Directory('${Directory.systemTemp.path}/update');
  await dir.create(recursive: true);
  // 清理历史包：只保留当前一个
  await for (final f in dir.list()) {
    if (f is File && f.path != '${dir.path}/update.apk') {
      await f.delete().catchError((_) => f);
    }
  }
  return dir;
}
