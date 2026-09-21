// 真实后端联调用例（默认跳过）：用 App 的 MeetingApi 打真实服务，
// 验证 multipart 上传、鉴权头、上游地址透传、JSON 解析与 docx 字节流。
//
// 运行方式（需先启动后端）：
//   MMA_LIVE_BASEURL=http://YOUR_SERVER_IP:8000 \
//   MMA_LIVE_KEY=sk-xxx \
//   MMA_LIVE_WAV=/tmp/opencode/asr-test/sample_zh.wav \
//   flutter test test/live_backend_test.dart
//
// 未设置环境变量时整体跳过，因此可以安全地留在 CI/本地测试里。
import 'dart:io';

import 'package:flutter_test/flutter_test.dart';

import 'package:bnu_meeting/data/api/meeting_api.dart';
import 'package:bnu_meeting/data/models/app_settings.dart';

void main() {
  final baseUrl = Platform.environment['MMA_LIVE_BASEURL'] ?? '';
  final apiKey = Platform.environment['MMA_LIVE_KEY'] ?? '';
  final wavPath = Platform.environment['MMA_LIVE_WAV'] ?? '';
  final upstream = Platform.environment['MMA_LIVE_UPSTREAM'] ?? 'https://chatapi.bnu.edu.cn/v1';

  final enabled = baseUrl.isNotEmpty && apiKey.isNotEmpty && wavPath.isNotEmpty;

  group('真实后端联调', () {
    late MeetingApi api;

    setUp(() {
      api = MeetingApi(
        settings: AppSettings.defaults().copyWith(
          backendBaseUrl: baseUrl,
          upstreamBaseUrl: upstream,
          apiKey: apiKey,
        ),
      );
    });

    test('health + 上游模型列表', () async {
      final health = await api.health();
      expect(health['status'], 'ok');

      final models = await api.listUpstreamModels();
      expect(models, isNotEmpty);
      // ignore: avoid_print
      print('上游模型：${models.length} 个');
    });

    test('上传 WAV 转写', () async {
      final text = await api.transcribeSegment(
        file: File(wavPath),
        seq: 1,
        sessionId: 'live-test',
      );
      expect(text.trim(), isNotEmpty);
      // ignore: avoid_print
      print('转写结果：$text');
    });

    test('生成纪要 + 导出 Word', () async {
      final result = await api.generateMinutes(
        transcript: '张三：本周完成灰度发布准备。\n'
            '李四：我来写发布公告，周三前给出。\n'
            '张三：决定下周三灰度 10%，周五全量。',
        title: '联调测试会议',
        meetingDate: '2026-09-21',
        participants: ['张三', '李四'],
        template: '重点提炼风险与待办',
      );
      expect(result.raw['title'], isNotEmpty);
      expect(result.doc.title, isNotEmpty);
      // 抬头信息应原样落到结构化纪要里（Word 导出的依据）
      expect(result.doc.participants, contains('张三'));
      expect(result.doc.meetingDate, contains('2026-09-21'));

      final bytes = await api.exportDocx(minutes: result.raw);
      expect(bytes.length, greaterThan(1000));
      // docx 是 zip 包，开头应为 PK
      expect(bytes.take(2).toList(), [0x50, 0x4B]);
      // ignore: avoid_print
      print('Word 大小：${bytes.length} 字节');
    });
  }, skip: enabled ? false : '未设置 MMA_LIVE_BASEURL / MMA_LIVE_KEY / MMA_LIVE_WAV，跳过真实联调');
}
