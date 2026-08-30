// 协议编解码：与 doc/移动端遥控器设计.md §三 定稿契约逐条对应。
// 传输：WebSocket 文本帧，UTF-8 JSON，一条一帧。

import 'dart:convert';

/// s2c 消息（PC → App）
class S2C {
  S2C(this.t, this.raw);

  final String t;
  final Map<String, dynamic> raw;

  static S2C decode(String text) {
    final m = jsonDecode(text);
    if (m is! Map<String, dynamic>) {
      throw const FormatException('s2c 消息必须是 JSON 对象');
    }
    return S2C(m['t'] as String? ?? '', m);
  }

  String? get ref => raw['ref'] as String?;
  bool get ok => raw['ok'] as bool? ?? false;
  String? get err => raw['err'] as String?;
  String? get token => raw['token'] as String?;

  RemoteState? get state => raw['state'] is Map
      ? RemoteState.fromJson((raw['state'] as Map).cast<String, dynamic>())
      : null;

  List<FavoriteItem>? get items => (raw['items'] as List?)
      ?.map((e) => FavoriteItem.fromJson((e as Map).cast<String, dynamic>()))
      .toList();

  /// event.type：favorites_changed / settings_changed / playback_changed
  String? get eventType =>
      (raw['event'] as Map?)?.cast<String, dynamic>()['type'] as String?;
}

/// 宿主状态（§3.4 state 结构）
class RemoteState {
  const RemoteState({
    this.micSend = false,
    this.playingId,
    this.synthesizing = false,
  });

  final bool micSend;
  final String? playingId;
  final bool synthesizing;

  factory RemoteState.fromJson(Map<String, dynamic> j) => RemoteState(
        micSend: j['mic_send'] as bool? ?? false,
        playingId: j['playing_id'] as String?,
        synthesizing: j['synthesizing'] as bool? ?? false,
      );
}

/// 收藏元数据（§3.4 favorites.items，不含音频，只读+触发）
class FavoriteItem {
  const FavoriteItem({
    required this.id,
    required this.note,
    this.createdAt,
    this.hotkey,
  });

  final String id;
  final String note;
  final String? createdAt;
  final String? hotkey;

  factory FavoriteItem.fromJson(Map<String, dynamic> j) => FavoriteItem(
        id: j['id'] as String? ?? '',
        note: j['note'] as String? ?? '',
        createdAt: j['created_at'] as String?,
        hotkey: j['hotkey'] as String?,
      );
}

/// c2s 命令构造（§3.2）：所有命令建议带 ref 以关联 ack
Map<String, dynamic> c2s(
  String t, {
  String? ref,
  Map<String, dynamic>? extra,
}) {
  final m = <String, dynamic>{'t': t};
  if (ref != null) m['ref'] = ref;
  if (extra != null) m.addAll(extra);
  return m;
}

String encodeC2s(Map<String, dynamic> m) => jsonEncode(m);
