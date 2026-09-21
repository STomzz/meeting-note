/// 结构化会议纪要（与后端 `Minutes` 契约一一对应，用于 UI 渲染与 Word 导出）。
class MinutesDoc {
  const MinutesDoc({
    required this.title,
    this.meetingDate,
    this.participants = const [],
    this.overview = '',
    this.topics = const [],
    this.decisions = const [],
    this.actionItems = const [],
    this.risks = const [],
    this.nextSteps = const [],
  });

  factory MinutesDoc.fromJson(Map<String, dynamic> json) => MinutesDoc(
        title: json['title'] as String? ?? '会议纪要',
        meetingDate: json['meeting_date'] as String?,
        participants: (json['participants'] as List?)?.map((e) => '$e').toList() ?? const [],
        overview: json['overview'] as String? ?? '',
        topics: (json['topics'] as List?)
                ?.whereType<Map<String, dynamic>>()
                .map(MinuteTopic.fromJson)
                .toList() ??
            const [],
        decisions: (json['decisions'] as List?)?.map((e) => '$e').toList() ?? const [],
        actionItems: (json['action_items'] as List?)
                ?.whereType<Map<String, dynamic>>()
                .map(ActionItem.fromJson)
                .toList() ??
            const [],
        risks: (json['risks'] as List?)?.map((e) => '$e').toList() ?? const [],
        nextSteps: (json['next_steps'] as List?)?.map((e) => '$e').toList() ?? const [],
      );

  final String title;
  final String? meetingDate;
  final List<String> participants;
  final String overview;
  final List<MinuteTopic> topics;
  final List<String> decisions;
  final List<ActionItem> actionItems;
  final List<String> risks;
  final List<String> nextSteps;
}

class MinuteTopic {
  const MinuteTopic({required this.title, this.points = const []});

  factory MinuteTopic.fromJson(Map<String, dynamic> json) => MinuteTopic(
        title: json['title'] as String? ?? '',
        points: (json['points'] as List?)?.map((e) => '$e').toList() ?? const [],
      );

  final String title;
  final List<String> points;
}

class ActionItem {
  const ActionItem({required this.task, this.owner, this.due});

  factory ActionItem.fromJson(Map<String, dynamic> json) => ActionItem(
        task: json['task'] as String? ?? '',
        owner: json['owner'] as String?,
        due: json['due'] as String?,
      );

  final String task;
  final String? owner;
  final String? due;
}
