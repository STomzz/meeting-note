// 说明：该文件替换了 `flutter create` 生成的模板测试（模板里的 MyApp 已不存在）。
// 这里放不依赖插件的纯 Dart 层测试：设置、会议/分段映射、纪要解析。
import 'package:flutter_test/flutter_test.dart';

import 'package:bnu_meeting/core/app_config.dart';
import 'package:bnu_meeting/data/models/app_settings.dart';
import 'package:bnu_meeting/data/models/meeting.dart';
import 'package:bnu_meeting/data/models/minutes_doc.dart';
import 'package:bnu_meeting/data/models/segment.dart';

void main() {
  test('AppSettings 默认值与 JSON 往返', () {
    final defaults = AppSettings.defaults();
    expect(defaults.backendBaseUrl, AppConfig.defaultBackendBaseUrl);
    expect(defaults.upstreamBaseUrl, 'https://chatapi.bnu.edu.cn/v1');
    expect(defaults.hasApiKey, isFalse);

    final restored = AppSettings.fromJson(
      defaults.copyWith(apiKey: 'sk-test', asrModel: 'asr-x').toJson(),
    );
    expect(restored.apiKey, 'sk-test');
    expect(restored.asrModel, 'asr-x');
    expect(restored.minutesModel, AppConfig.defaultMinutesModel);
  });

  test('Meeting 行映射往返（sqflite 存的是毫秒时间戳）', () {
    final now = DateTime.fromMillisecondsSinceEpoch(1789988274000);
    final meeting = Meeting(id: 'm1', title: '周会', createdAt: now);

    final restored = Meeting.fromRow(meeting.toRow());
    expect(restored.id, 'm1');
    expect(restored.title, '周会');
    expect(restored.createdAt, now);
    expect(restored.status, MeetingStatus.recording);
    expect(restored.hasMinutes, isFalse);
  });

  test('Meeting 抬头信息（日期/参会人/附加要求）往返', () {
    final now = DateTime.fromMillisecondsSinceEpoch(1789988274000);
    final meeting = Meeting(
      id: 'm2',
      title: '产品周会',
      createdAt: now,
      meetingDate: '2026-09-21',
      participants: const ['张三', '李四'],
      template: '重点提炼风险',
    );

    final restored = Meeting.fromRow(meeting.toRow());
    expect(restored.meetingDate, '2026-09-21');
    expect(restored.participants, ['张三', '李四']);
    expect(restored.template, '重点提炼风险');

    final empty = Meeting.fromRow(
      Meeting(id: 'm3', title: '无抬头', createdAt: now).toRow(),
    );
    expect(empty.participants, isEmpty);
    expect(empty.meetingDate, isNull);
  });

  test('AudioSegment 状态与文本', () {
    final segment = AudioSegment(
      meetingId: 'm1',
      seq: 2,
      filePath: '/tmp/seg.wav',
      durationMs: 12345,
      createdAt: DateTime.fromMillisecondsSinceEpoch(1789988274000),
      status: SegmentStatus.done,
      text: '大家好',
    );
    final restored = AudioSegment.fromRow(segment.toRow());
    expect(restored.seq, 2);
    expect(restored.status, SegmentStatus.done);
    expect(restored.hasText, isTrue);
    expect(restored.durationMs, 12345);
  });

  test('MinutesDoc 解析后端口径的 JSON', () {
    final doc = MinutesDoc.fromJson({
      'title': '产品周会',
      'meeting_date': '2026-09-21',
      'participants': ['张三', '李四'],
      'overview': '讨论了发布计划。',
      'topics': [
        {
          'title': '发布计划',
          'points': ['下周三灰度', '周五全量'],
        }
      ],
      'decisions': ['按原计划发布'],
      'action_items': [
        {'task': '准备发布公告', 'owner': '李四', 'due': '周三'},
      ],
      'risks': ['监控不足'],
      'next_steps': ['下周一预演'],
    });

    expect(doc.title, '产品周会');
    expect(doc.participants, ['张三', '李四']);
    expect(doc.topics.single.points.length, 2);
    expect(doc.actionItems.single.owner, '李四');
    expect(doc.risks.single, '监控不足');
  });
}
