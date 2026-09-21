import 'dart:async';
import 'dart:io';

import 'package:flutter/foundation.dart';
import 'package:flutter_foreground_task/flutter_foreground_task.dart';

import '../core/errors.dart';
import '../data/local/meeting_dao.dart';
import '../data/models/meeting.dart';
import '../data/models/segment.dart';
import '../domain/recording/recording_foreground_service.dart';
import '../domain/recording/segment_recorder.dart';
import '../domain/recording/upload_queue.dart';
import '../domain/storage/app_paths.dart';

/// 单次录音会话的状态机：录音 → 切段 → 落盘 → 排队转写。
class RecordingController extends ChangeNotifier {
  RecordingController({
    required MeetingDao dao,
    required UploadQueue uploadQueue,
    SegmentRecorder? recorder,
  })  : _dao = dao,
        _queue = uploadQueue,
        _recorder = recorder ?? SegmentRecorder();

  final MeetingDao _dao;
  final UploadQueue _queue;
  final SegmentRecorder _recorder;

  final List<StreamSubscription<dynamic>> _subscriptions = <StreamSubscription<dynamic>>[];
  final List<AudioSegment> _segments = <AudioSegment>[];
  final Set<Future<void>> _pendingSaves = <Future<void>>{};

  Meeting? _meeting;
  bool _recording = false;
  bool _stopping = false;
  Duration _elapsed = Duration.zero;
  Timer? _timer;
  String? _error;
  double _level = 0;

  /// 通知栏「停止录音」等外部停止后的回调（由页面负责跳转收尾）。
  Future<void> Function()? onStoppedExternally;

  late final void Function(Object) _serviceDataCallback = _onServiceData;

  Meeting? get meeting => _meeting;
  List<AudioSegment> get segments => List.unmodifiable(_segments);
  bool get recording => _recording;
  bool get stopping => _stopping;
  Duration get elapsed => _elapsed;
  String? get error => _error;
  double get level => _level;

  /// 已转写的文本（按分段顺序拼接）。
  String get transcript => _segments
      .where((segment) => segment.hasText)
      .map((segment) => segment.text!.trim())
      .join('\n');

  int get failedCount =>
      _segments.where((segment) => segment.status == SegmentStatus.failed).length;

  bool get hasPendingWork =>
      _segments.any((segment) =>
          segment.status == SegmentStatus.pending || segment.status == SegmentStatus.uploading) ||
      !_queue.isIdle;

  Future<void> start() async {
    if (_recording) return;
    _error = null;

    final now = DateTime.now();
    final meeting = Meeting(
      id: 'm${now.millisecondsSinceEpoch}',
      title: _defaultTitle(now),
      createdAt: now,
    );
    await _dao.saveMeeting(meeting);

    _meeting = meeting;
    _segments.clear();
    _elapsed = Duration.zero;

    _subscriptions
      ..add(_recorder.segments.listen(_onSegment))
      ..add(_recorder.levels.listen((value) => _level = value))
      ..add(_queue.updates.listen(_onQueueUpdate));

    try {
      await _recorder.start();
      // 前台服务：锁屏/切后台也能继续录
      FlutterForegroundTask.addTaskDataCallback(_serviceDataCallback);
      await RecordingForegroundService.start(title: meeting.title);
    } catch (error) {
      _error = error is AppException ? error.message : '$error';
      await _cancelSubscriptions();
      notifyListeners();
      rethrow;
    }

    _recording = true;
    _timer = Timer.periodic(const Duration(seconds: 1), (_) {
      _elapsed += const Duration(seconds: 1);
      if (_elapsed.inSeconds % 30 == 0) {
        _updateNotification();
      }
      notifyListeners();
    });
    notifyListeners();
  }

  /// 停止录音：吐出最后一段、落盘、把会议标记为已完成（上传继续在后台跑）。
  Future<void> stop() async {
    if (!_recording || _stopping) return;
    _stopping = true;
    notifyListeners();

    await _recorder.stop();
    if (_pendingSaves.isNotEmpty) {
      await Future.wait(_pendingSaves.toList());
    }

    _recording = false;
    _timer?.cancel();
    _timer = null;
    await _cancelSubscriptions();
    FlutterForegroundTask.removeTaskDataCallback(_serviceDataCallback);
    unawaited(RecordingForegroundService.stop());

    final meeting = _meeting;
    if (meeting != null) {
      final updated = meeting.copyWith(
        status: MeetingStatus.completed,
        endedAt: DateTime.now(),
        updatedAt: DateTime.now(),
      );
      await _dao.saveMeeting(updated);
      _meeting = updated;
    }

    _stopping = false;
    notifyListeners();
  }

  /// 手动重试失败的分段。
  Future<void> retryFailed() async {
    final failed = _segments.where((segment) => segment.status == SegmentStatus.failed).toList();
    for (final segment in failed) {
      await _queue.add(segment.copyWith(status: SegmentStatus.pending, error: null));
    }
    notifyListeners();
  }

  void clearError() {
    _error = null;
    notifyListeners();
  }

  @override
  void dispose() {
    _timer?.cancel();
    unawaited(_cancelSubscriptions());
    unawaited(_recorder.dispose());
    super.dispose();
  }

  // ------------------------------------------------------------------ 内部
  void _onSegment(RecordedSegment recorded) {
    final future = _saveSegment(recorded);
    _pendingSaves.add(future);
    future.whenComplete(() => _pendingSaves.remove(future));
  }

  Future<void> _saveSegment(RecordedSegment recorded) async {
    final meeting = _meeting;
    if (meeting == null) return;
    try {
      final path = await AppPaths.segmentPath(meeting.id, recorded.seq);
      await File(path).writeAsBytes(recorded.wavBytes, flush: true);

      var segment = AudioSegment(
        meetingId: meeting.id,
        seq: recorded.seq,
        filePath: path,
        durationMs: recorded.durationMs,
        createdAt: DateTime.now(),
      );
      segment = await _dao.insertSegment(segment);
      _segments.add(segment);
      notifyListeners();

      await _queue.add(segment);
    } catch (error) {
      _error = '保存录音分段失败：$error';
      notifyListeners();
    }
  }

  void _onQueueUpdate(AudioSegment updated) {
    final index = _segments.indexWhere((segment) => segment.id == updated.id);
    if (index < 0) return;
    _segments[index] = updated;
    if (updated.status == SegmentStatus.done) {
      _updateNotification();
    }
    notifyListeners();
  }

  /// 通知栏「停止录音」：从服务 isolate 收到动作后收尾并通知页面。
  void _onServiceData(Object data) {
    if (data is! Map) return;
    if (data['action'] != RecordingForegroundService.stopButtonId) return;
    stop().then((_) => onStoppedExternally?.call());
  }

  void _updateNotification() {
    final done = _segments.where((segment) => segment.status == SegmentStatus.done).length;
    unawaited(
      RecordingForegroundService.updateText(
        '已转写 $done 段 · ${_clock(_elapsed)}',
      ),
    );
  }

  String _clock(Duration duration) =>
      '${duration.inHours.toString().padLeft(2, '0')}:'
      '${(duration.inMinutes % 60).toString().padLeft(2, '0')}:'
      '${(duration.inSeconds % 60).toString().padLeft(2, '0')}';

  Future<void> _cancelSubscriptions() async {
    for (final subscription in _subscriptions) {
      await subscription.cancel();
    }
    _subscriptions.clear();
  }

  String _defaultTitle(DateTime time) =>
      '会议 ${time.month.toString().padLeft(2, '0')}-${time.day.toString().padLeft(2, '0')} '
      '${time.hour.toString().padLeft(2, '0')}:${time.minute.toString().padLeft(2, '0')}';
}
