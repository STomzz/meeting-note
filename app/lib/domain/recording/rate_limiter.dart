import 'dart:collection';

/// 简易令牌桶（滑动窗口）：保证 [window] 内最多 [maxRequests] 次请求。
///
/// 用途：BNUAPI 限流是「每用户 10 请求/分钟」，分段转写 + 纪要生成都算在内。
/// 本地先限到 9/分钟，把上游 429 变成"排一会儿队"而不是"失败重试"。
class RateLimiter {
  RateLimiter({required this.maxRequests, required this.window});

  final int maxRequests;
  final Duration window;

  final Queue<DateTime> _timestamps = Queue<DateTime>();

  /// 取得一个令牌；必要时等待到最早的请求滑出窗口。
  ///
  /// 注意：判空 + 计数 + 入队的代码段没有 await，在同一 isolate 内是原子的。
  Future<void> acquire() async {
    while (true) {
      final now = DateTime.now();
      while (_timestamps.isNotEmpty && now.difference(_timestamps.first) >= window) {
        _timestamps.removeFirst();
      }
      if (_timestamps.length < maxRequests) {
        _timestamps.add(now);
        return;
      }
      final wait = window - now.difference(_timestamps.first);
      await Future<void>.delayed(
        wait > Duration.zero ? wait : const Duration(milliseconds: 200),
      );
    }
  }
}
