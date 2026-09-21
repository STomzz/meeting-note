/// 一个录音分段（音频文件保存在手机，转写文本存在这里）。
class AudioSegment {
  const AudioSegment({
    this.id,
    required this.meetingId,
    required this.seq,
    required this.filePath,
    required this.durationMs,
    this.status = SegmentStatus.pending,
    this.text,
    this.error,
    this.retryCount = 0,
    required this.createdAt,
  });

  factory AudioSegment.fromRow(Map<String, dynamic> row) => AudioSegment(
        id: row['id'] as int?,
        meetingId: row['meeting_id'] as String,
        seq: row['seq'] as int,
        filePath: row['file_path'] as String,
        durationMs: row['duration_ms'] as int? ?? 0,
        status: SegmentStatus.fromValue(row['status'] as String? ?? 'pending'),
        text: row['text'] as String?,
        error: row['error'] as String?,
        retryCount: row['retry_count'] as int? ?? 0,
        createdAt: DateTime.fromMillisecondsSinceEpoch(row['created_at'] as int),
      );

  final int? id;
  final String meetingId;
  final int seq;
  final String filePath;
  final int durationMs;
  final SegmentStatus status;
  final String? text;
  final String? error;
  final int retryCount;
  final DateTime createdAt;

  bool get hasText => (text ?? '').trim().isNotEmpty;

  AudioSegment copyWith({
    int? id,
    SegmentStatus? status,
    String? text,
    String? error,
    int? retryCount,
  }) =>
      AudioSegment(
        id: id ?? this.id,
        meetingId: meetingId,
        seq: seq,
        filePath: filePath,
        durationMs: durationMs,
        status: status ?? this.status,
        text: text ?? this.text,
        error: error,
        retryCount: retryCount ?? this.retryCount,
        createdAt: createdAt,
      );

  Map<String, dynamic> toRow() => {
        if (id != null) 'id': id,
        'meeting_id': meetingId,
        'seq': seq,
        'file_path': filePath,
        'duration_ms': durationMs,
        'status': status.value,
        'text': text,
        'error': error,
        'retry_count': retryCount,
        'created_at': createdAt.millisecondsSinceEpoch,
      };
}

enum SegmentStatus {
  pending('pending'),
  uploading('uploading'),
  done('done'),
  failed('failed');

  const SegmentStatus(this.value);

  factory SegmentStatus.fromValue(String value) => SegmentStatus.values.firstWhere(
        (status) => status.value == value,
        orElse: () => SegmentStatus.pending,
      );

  final String value;
}
