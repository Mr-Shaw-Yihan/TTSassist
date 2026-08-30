// 冒烟测试：视觉基件可构建（网络/平台相关不在单测范围）。

import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';

import 'package:voiceassist_remote/theme.dart';

void main() {
  testWidgets('DiffuseBackground 与 GlassCard 可构建', (tester) async {
    await tester.pumpWidget(
      const MaterialApp(
        home: Scaffold(
          body: DiffuseBackground(),
        ),
      ),
    );
    expect(find.byType(DiffuseBackground), findsOneWidget);

    await tester.pumpWidget(
      const MaterialApp(
        home: Scaffold(
          body: GlassCard(child: Text('ok')),
        ),
      ),
    );
    expect(find.text('ok'), findsOneWidget);
  });
}
