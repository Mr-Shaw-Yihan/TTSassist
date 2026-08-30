// 入口：AppState 单一数据源；connected → 主遥控页，否则配对页；toast 全局悬浮。

import 'package:flutter/material.dart';

import 'state/app_state.dart';
import 'theme.dart';
import 'ui/main_page.dart';
import 'ui/pair_page.dart';

void main() => runApp(const RemoteApp());

class RemoteApp extends StatelessWidget {
  const RemoteApp({super.key});

  @override
  Widget build(BuildContext context) => MaterialApp(
        title: '电子声带遥控',
        debugShowCheckedModeBanner: false,
        theme: ThemeData(
          useMaterial3: true,
          scaffoldBackgroundColor: Colors.transparent,
          splashFactory: NoSplash.splashFactory,
          highlightColor: Colors.transparent,
        ),
        home: const RootPage(),
      );
}

class RootPage extends StatefulWidget {
  const RootPage({super.key});

  @override
  State<RootPage> createState() => _RootPageState();
}

class _RootPageState extends State<RootPage> {
  final AppState _app = AppState();

  @override
  void initState() {
    super.initState();
    _app.init();
  }

  @override
  void dispose() {
    _app.dispose();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) => ListenableBuilder(
        listenable: _app,
        builder: (_, _) => Stack(
          children: [
            _app.connected
                ? MainPage(app: _app, key: const ValueKey('main'))
                : PairPage(app: _app, key: const ValueKey('pair')),
            if (_app.toast != null)
              Positioned(
                left: 24,
                right: 24,
                bottom: 120,
                child: Center(
                  child: Container(
                    padding: const EdgeInsets.symmetric(
                        horizontal: 18, vertical: 12),
                    decoration: BoxDecoration(
                      color: RT.ink.withValues(alpha: 0.88),
                      borderRadius: BorderRadius.circular(14),
                    ),
                    child: Text(
                      _app.toast!,
                      style: const TextStyle(
                          fontSize: 13, color: Colors.white),
                    ),
                  ),
                ),
              ),
          ],
        ),
      );
}
