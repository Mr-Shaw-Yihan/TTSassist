// 视觉规范：doc/移动端遥控器设计.md §4.3（v3 黑金改版：黄蓝弥散 + 毛玻璃 + 哑金强调）。
// 暖米白底 + 黄/蓝弥散色块；毛玻璃卡片；主按钮面纯黑；哑金实心磨砂图标与金瘦体文字；
// 按压 0.96；播放态金色脉动。字重：全局无衬线瘦体（大字 w300，功能小字 w400）。

import 'dart:ui';

import 'package:flutter/material.dart';
import 'package:flutter_svg/flutter_svg.dart';

/// 设计令牌
class RT {
  static const bg = Color(0xFFF8F4EA); // 暖米白底
  static const ink = Color(0xFF1A1816); // 主文字
  static const sub = Color(0xFF6B675F); // 次文字
  static const gold = Color(0xFFA9821C); // 哑金强调（状态/次级操作）
  static const goldText = Color(0xFF8A7414); // 条目金（瘦体文字用）
  static const goldTextDim = Color(0x8C8A7414); // 条目金·弱化（快捷键等）
  static const btnBlack = Color(0xFF17150F); // 主按钮面（纯黑，平色无噪点）
  static const onBlack = Color(0xFFF7F3EA); // 黑面上的文字
  static const blobYellow = Color(0xFFEDD9A4); // 弥散色块·暖黄
  static const blobBlue = Color(0xFFB9C7DC); // 弥散色块·雾蓝

  // 全局瘦体：大字（标题/条目名）
  static const thin = TextStyle(
      fontWeight: FontWeight.w300, letterSpacing: 0.5, height: 1.5);
}

/// 背景：暖米白底 + 暖黄/雾蓝两团极度模糊弥散色块（图一风格，左下黄斑补光）
class DiffuseBackground extends StatelessWidget {
  const DiffuseBackground({super.key});

  @override
  Widget build(BuildContext context) {
    final size = MediaQuery.of(context).size;
    return Container(
      color: RT.bg,
      child: Stack(
        children: [
          _blob(size.width * 1.2, RT.blobYellow, 0.55,
              Offset(-size.width * 0.33, -size.height * 0.07)),
          _blob(size.width * 1.08, RT.blobBlue, 0.55,
              Offset(size.width * 0.44, size.height * 0.56)),
          _blob(size.width * 0.77, RT.blobYellow, 0.35,
              Offset(-size.width * 0.28, size.height * 0.71)),
        ],
      ),
    );
  }

  Widget _blob(double d, Color c, double alpha, Offset off) => Positioned(
        left: off.dx,
        top: off.dy,
        width: d,
        height: d,
        child: ImageFiltered(
          imageFilter: ImageFilter.blur(sigmaX: 90, sigmaY: 90),
          child: Container(
            decoration: BoxDecoration(
              shape: BoxShape.circle,
              color: c.withValues(alpha: alpha),
            ),
          ),
        ),
      );
}

/// 毛玻璃卡片：半透明白 + backdrop blur，圆角 22，极淡白边，轻投影（图一/图二卡片）
class GlassCard extends StatelessWidget {
  const GlassCard({
    super.key,
    required this.child,
    this.radius = 22,
    this.opacity = 0.52,
    this.padding,
  });

  final Widget child;
  final double radius;
  final double opacity;
  final EdgeInsets? padding;

  @override
  Widget build(BuildContext context) {
    return ClipRRect(
      borderRadius: BorderRadius.circular(radius),
      child: BackdropFilter(
        filter: ImageFilter.blur(sigmaX: 22, sigmaY: 22),
        child: Container(
          padding: padding,
          decoration: BoxDecoration(
            color: Colors.white.withValues(alpha: opacity),
            borderRadius: BorderRadius.circular(radius),
            border: Border.all(color: Colors.white.withValues(alpha: 0.4)),
            boxShadow: const [
              BoxShadow(
                color: Color(0x0F1A1816),
                offset: Offset(0, 10),
                blurRadius: 28,
              ),
            ],
          ),
          child: child,
        ),
      ),
    );
  }
}

/// 按压缩放 0.96 反馈（§4.3 动效）
class PressScale extends StatefulWidget {
  const PressScale({super.key, required this.onTap, required this.child});

  final VoidCallback onTap;
  final Widget child;

  @override
  State<PressScale> createState() => _PressScaleState();
}

class _PressScaleState extends State<PressScale> {
  bool _pressed = false;

  void _set(bool v) {
    if (mounted && _pressed != v) setState(() => _pressed = v);
  }

  @override
  Widget build(BuildContext context) {
    return GestureDetector(
      behavior: HitTestBehavior.opaque,
      onTapDown: (_) => _set(true),
      onTapUp: (_) {
        _set(false);
        widget.onTap();
      },
      onTapCancel: () => _set(false),
      child: AnimatedScale(
        scale: _pressed ? 0.96 : 1,
        duration: const Duration(milliseconds: 120),
        child: widget.child,
      ),
    );
  }
}

/// 播放态金色脉动光晕（§4.2 收藏卡片）
class PulseGlow extends StatefulWidget {
  const PulseGlow({super.key, required this.child, this.radius = 22});

  final Widget child;
  final double radius;

  @override
  State<PulseGlow> createState() => _PulseGlowState();
}

class _PulseGlowState extends State<PulseGlow>
    with SingleTickerProviderStateMixin {
  late final AnimationController _ctl = AnimationController(
    vsync: this,
    duration: const Duration(milliseconds: 900),
  )..repeat(reverse: true);

  @override
  void dispose() {
    _ctl.dispose();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    return AnimatedBuilder(
      animation: _ctl,
      builder: (_, _) {
        final t = _ctl.value;
        return Container(
          decoration: BoxDecoration(
            borderRadius: BorderRadius.circular(widget.radius),
            boxShadow: [
              BoxShadow(
                color: RT.gold.withValues(alpha: 0.22 + 0.22 * t),
                blurRadius: 12 + 8 * t,
                spreadRadius: 1,
              ),
            ],
          ),
          child: widget.child,
        );
      },
    );
  }
}

/// 主按钮面：纯黑平色（图一式，无噪点）。顶行按钮与「连接」类主按钮统一用它。
class MatteBlack extends StatelessWidget {
  const MatteBlack({super.key, required this.child, this.radius = 18});

  final Widget child;
  final double radius;

  @override
  Widget build(BuildContext context) {
    return Container(
      decoration: BoxDecoration(
        color: RT.btnBlack,
        borderRadius: BorderRadius.circular(radius),
      ),
      child: child,
    );
  }
}

/// 磨砂哑金按钮面：金渐变 + gold_texture.png 噪点叠层。
/// 用于发现列表设备条目等需要金色主视觉的按钮（黑色主按钮用 MatteBlack）。
class FrostedGoldTile extends StatelessWidget {
  const FrostedGoldTile({super.key, required this.child, this.radius = 16});

  final Widget child;
  final double radius;

  @override
  Widget build(BuildContext context) {
    final rb = BorderRadius.circular(radius);
    return Container(
      decoration: BoxDecoration(
        gradient: const LinearGradient(
          begin: Alignment.topCenter,
          end: Alignment.bottomCenter,
          colors: [Color(0xFFE6C25C), Color(0xFFC69A2E)],
        ),
        borderRadius: rb,
      ),
      foregroundDecoration: BoxDecoration(
        image: const DecorationImage(
          image: AssetImage('assets/icons/gold_texture.png'),
          repeat: ImageRepeat.repeat,
        ),
        borderRadius: rb,
      ),
      child: child,
    );
  }
}

/// 哑金磨砂 SVG 图标（assets/icons/，gen_icons.py 生成；_f 系列为实心金渐变 + 噪斑）
class RIcon extends StatelessWidget {
  const RIcon(this.name, {super.key, this.size = 22});

  final String name;
  final double size;

  @override
  Widget build(BuildContext context) {
    return SvgPicture.asset(
      'assets/icons/$name.svg',
      width: size,
      height: size,
    );
  }
}
