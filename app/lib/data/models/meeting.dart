/// 会议（一场录音 = 一条会议记录，全部保存在手机本地）。
class Meeting {
  const Meeting({
    required this.id,
    required this.title,
    required this.createdAt,
    this.endedAt,
    this.status = MeetingStatus.recording,
    this.minutesJson,
    this.minutesMarkdown,
    this.minutesModel,
    this.updatedAt,
    this.meetingDate,
    this.participants = const [],
    this.template,
  });

  factory Meeting.fromRow(Map<String, dynamic> row) => Meeting(
        id: row['id'] as String,
        title: row['title'] as String,
        createdAt: DateTime.fromMillisecondsSinceEpoch(row['created_at'] as int),
        endedAt: row['ended_at'] == null
            ? null
            : DateTime.fromMillisecondsSinceEpoch(row['ended_at'] as int),
        status: MeetingStatus.fromValue(row['status'] as String? ?? 'recording'),
        minutesJson: row['minutes_json'] as String?,
        minutesMarkdown: row['minutes_markdown'] as String?,
        minutesModel: row['minutes_model'] as String?,
        updatedAt: row['updated_at'] == null
            ? null
            : DateTime.fromMillisecondsSinceEpoch(row['updated_at'] as int),
        meetingDate: row['meeting_date'] as String?,
        participants: _splitParticipants(row['participants'] as String?),
        template: row['template'] as String?,
      );

  final String id;
  final String title;
  final DateTime createdAt;
  final DateTime? endedAt;
  final MeetingStatus status;

  /// 结构化纪要原始 JSON（后端返回的 minutes 对象）
  final String? minutesJson;
  final String? minutesMarkdown;
  final String? minutesModel;
  final DateTime? updatedAt;

  /// 纪要抬头信息（生成纪要时填写，导出 Word 用）
  final String? meetingDate;
  final List<String> participants;
  final String? template;

  bool get hasMinutes => (minutesJson ?? '').isNotEmpty;

  Map<String, dynamic> toRow() => {
        'id': id,
        'title': title,
        'created_at': createdAt.millisecondsSinceEpoch,
        'ended_at': endedAt?.millisecondsSinceEpoch,
        'status': status.value,
        'minutes_json': minutesJson,
        'minutes_markdown': minutesMarkdown,
        'minutes_model': minutesModel,
        'updated_at': (updatedAt ?? createdAt).millisecondsSinceEpoch,
        'meeting_date': meetingDate,
        'participants': participants.isEmpty ? null : participants.join(','),
        'template': template,
      };

  Meeting copyWith({
    String? title,
    DateTime? endedAt,
    MeetingStatus? status,
    String? minutesJson,
    String? minutesMarkdown,
    String? minutesModel,
    DateTime? updatedAt,
    String? meetingDate,
    List<String>? participants,
    String? template,
  }) =>
      Meeting(
        id: id,
        title: title ?? this.title,
        createdAt: createdAt,
        endedAt: endedAt ?? this.endedAt,
        status: status ?? this.status,
        minutesJson: minutesJson ?? this.minutesJson,
        minutesMarkdown: minutesMarkdown ?? this.minutesMarkdown,
        minutesModel: minutesModel ?? this.minutesModel,
        updatedAt: updatedAt ?? this.updatedAt,
        meetingDate: meetingDate ?? this.meetingDate,
        participants: participants ?? this.participants,
        template: template ?? this.template,
      );

  static List<String> _splitParticipants(String? raw) => raw == null || raw.trim().isEmpty
      ? const []
      : raw.split(',').map((name) => name.trim()).where((name) => name.isNotEmpty).toList();
}

enum MeetingStatus {
  recording('recording'),
  completed('completed');

  const MeetingStatus(this.value);

  factory MeetingStatus.fromValue(String value) => MeetingStatus.values.firstWhere(
        (status) => status.value == value,
        orElse: () => MeetingStatus.completed,
      );

  final String value;
}
