import 'dart:async';
import 'dart:io';

import '../../core/app_config.dart';
import '../../core/errors.dart';
import '../../data/api/meeting_api.dart';
import '../../data/local/meeting_dao.dart';
import '../../data/models/segment.dart';
import 'rate_limiter.dart';

/// 分段上传队列：录完一段就排队转写。
///
/// - 录音线程与上传线程互不阻塞（上一段上传时下一段继续录）；
/// - 失败自动重试（401/400 这类确定性错误不重试）；
/// - 每段的状态变更都会写回本地数据库并广播，UI 订阅 [updates]。
class UploadQueue {
  UploadQueue({required MeetingDao dao, required MeetingApi Function() apiFactory})
      : _dao = dao,
        _apiFactory = apiFactory;

  final MeetingDao _dao;
  final MeetingApi Function() _apiFactory;

  final List<AudioSegment> _waiting = <AudioSegment>[];
  final StreamController<AudioSegment> _controller =
      StreamController<AudioSegment>.broadcast();
  final RateLimiter _limiter = RateLimiter(
    maxRequests: AppConfig.uploadRateLimitPerMinute,
    window: const Duration(minutes: 1),
  );
  int _running = 0;

  Stream<AudioSegment> get updates => _controller.stream;

  bool get isIdle => _running == 0 && _waiting.isEmpty;

  Future<void> add(AudioSegment segment) async {
    _waiting.add(segment);
    _pump();
  }

  /// 重启 App / 重新进入会议时，把未完成的分段重新排进队列。
  Future<void> resume(String meetingId) async {
    final pending = await _dao.listUnfinishedSegments(meetingId);
    _waiting
      ..removeWhere((segment) => segment.meetingId == meetingId)
      ..addAll(pending);
    _pump();
  }

  // ------------------------------------------------------------------ 内部
  void _pump() {
    while (_running < AppConfig.uploadMaxConcurrent && _waiting.isNotEmpty) {
      final segment = _waiting.removeAt(0);
      _running++;
      _process(segment).whenComplete(() {
        _running--;
        _pump();
      });
    }
  }

  Future<void> _process(AudioSegment segment) async {
    var current = segment.copyWith(status: SegmentStatus.uploading, error: null);
    await _dao.updateSegment(current);
    _emit(current);

    final api = _apiFactory();
    for (var attempt = 1; attempt <= AppConfig.uploadMaxRetries; attempt++) {
      // 本地限流：把上游 429 变成"排队"而不是"失败"
      await _limiter.acquire();
      try {
        final text = await api.transcribeSegment(
          file: File(current.filePath),
          seq: current.seq,
          sessionId: current.meetingId,
        );
        current = current.copyWith(
          status: SegmentStatus.done,
          text: text,
          error: null,
          retryCount: attempt - 1,
        );
        await _dao.updateSegment(current);
        _emit(current);
        return;
      } on AppException catch (error) {
        // 429（限流）和 5xx 值得重试；4xx 里的确定性错误（Key 无效、音频非法等）不重试。
        final status = error.statusCode;
        final fatal = status != null && status != 429 && status < 500;
        if (fatal || attempt == AppConfig.uploadMaxRetries) {
          current = current.copyWith(
            status: SegmentStatus.failed,
            error: error.message,
            retryCount: attempt,
          );
          await _dao.updateSegment(current);
          _emit(current);
          return;
        }
        await Future<void>.delayed(_backoff(attempt, rateLimited: status == 429));
      } catch (error) {
        if (attempt == AppConfig.uploadMaxRetries) {
          current = current.copyWith(
            status: SegmentStatus.failed,
            error: '$error',
            retryCount: attempt,
          );
          await _dao.updateSegment(current);
          _emit(current);
          return;
        }
        await Future<void>.delayed(_backoff(attempt, rateLimited: false));
      }
    }
  }

  Duration _backoff(int attempt, {required bool rateLimited}) =>
      rateLimited ? Duration(seconds: 15 * attempt) : Duration(seconds: attempt * 2);

  void _emit(AudioSegment segment) {
    if (!_controller.isClosed) _controller.add(segment);
  }

  Future<void> dispose() async {
    await _controller.close();
  }
}
