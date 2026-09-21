import 'dart:io';
import 'dart:typed_data';

import '../../core/app_config.dart';
import '../../core/errors.dart';
import '../../data/local/meeting_dao.dart';
import '../../data/models/meeting.dart';
import '../../data/models/segment.dart';
import '../audio/wav.dart';
import '../recording/upload_queue.dart';
import '../storage/app_paths.dart';

/// 导入已有 WAV 文件（例如电脑录的系统声音），自动切块后走同一条转写流水线。
class AudioImporter {
  AudioImporter({required MeetingDao dao, required UploadQueue uploadQueue})
      : _dao = dao,
        _queue = uploadQueue;

  final MeetingDao _dao;
  final UploadQueue _queue;

  /// 单块最大 5 分钟：请求更少、失败重试成本更低。
  static const Duration _chunkDuration = Duration(minutes: 5);

  Future<Meeting> importWav({
    required File file,
    String? title,
    void Function(int done, int total)? onProgress,
  }) async {
    final bytes = await _readAll(file);
    if (bytes.length < 1024 || !_looksLikeWav(bytes)) {
      throw AppException('只支持 WAV 文件（推荐 16kHz 单声道）');
    }

    final chunks = WavCodec.splitByDuration(bytes, _chunkDuration);
    final now = DateTime.now();
    final meeting = Meeting(
      id: 'm${now.millisecondsSinceEpoch}',
      title: (title == null || title.trim().isEmpty) ? '导入音频 ${_stamp(now)}' : title.trim(),
      createdAt: now,
      endedAt: now,
      status: MeetingStatus.completed,
      updatedAt: now,
    );
    await _dao.saveMeeting(meeting);

    for (var index = 0; index < chunks.length; index++) {
      final chunk = chunks[index];
      final path = await AppPaths.segmentPath(meeting.id, index);
      await File(path).writeAsBytes(chunk, flush: true);
      var segment = AudioSegment(
        meetingId: meeting.id,
        seq: index,
        filePath: path,
        durationMs: WavCodec.durationMs(chunk) ?? 0,
        createdAt: DateTime.now(),
      );
      segment = await _dao.insertSegment(segment);
      await _queue.add(segment);
      onProgress?.call(index + 1, chunks.length);
    }

    return meeting;
  }

  Future<Uint8List> _readAll(File file) async {
    if (!await file.exists()) {
      throw AppException('文件不存在或无法读取');
    }
    final length = await file.length();
    if (length > AppConfig.importMaxBytes) {
      throw AppException('文件太大（>${AppConfig.importMaxBytes ~/ (1024 * 1024)}MB）');
    }
    return file.readAsBytes();
  }

  bool _looksLikeWav(Uint8List bytes) =>
      bytes.length > 12 &&
      String.fromCharCodes(bytes.sublist(0, 4)) == 'RIFF' &&
      String.fromCharCodes(bytes.sublist(8, 12)) == 'WAVE';

  String _stamp(DateTime time) =>
      '${time.month.toString().padLeft(2, '0')}-${time.day.toString().padLeft(2, '0')} '
      '${time.hour.toString().padLeft(2, '0')}:${time.minute.toString().padLeft(2, '0')}';
}
