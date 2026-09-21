import 'package:flutter_test/flutter_test.dart';

import 'package:bnu_meeting/domain/recording/rate_limiter.dart';

void main() {
  test('窗口内不超过上限', () async {
    final limiter = RateLimiter(maxRequests: 3, window: const Duration(milliseconds: 300));
    final stopwatch = Stopwatch()..start();

    for (var i = 0; i < 3; i++) {
      await limiter.acquire();
    }
    expect(stopwatch.elapsedMilliseconds, lessThan(100), reason: '前 3 次不应等待');

    await limiter.acquire(); // 第 4 次必须等窗口滑出
    expect(stopwatch.elapsedMilliseconds, greaterThanOrEqualTo(250));
  });

  test('窗口滑出后可继续获取', () async {
    final limiter = RateLimiter(maxRequests: 2, window: const Duration(milliseconds: 200));
    await limiter.acquire();
    await limiter.acquire();
    await Future<void>.delayed(const Duration(milliseconds: 220));

    final stopwatch = Stopwatch()..start();
    await limiter.acquire();
    await limiter.acquire();
    expect(stopwatch.elapsedMilliseconds, lessThan(100));
  });
}
