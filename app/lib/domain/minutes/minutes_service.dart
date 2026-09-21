import 'dart:convert';
import 'dart:io';

import '../../core/errors.dart';
import '../../data/api/meeting_api.dart';
import '../../data/local/meeting_dao.dart';
import '../../data/models/meeting.dart';
import '../storage/app_paths.dart';

/// 纪要生成与导出（业务层，UI 只调用这里）。
class MinutesService {
  MinutesService({required MeetingDao dao, required MeetingApi Function() apiFactory})
      : _dao = dao,
        _apiFactory = apiFactory;

  final MeetingDao _dao;
  final MeetingApi Function() _apiFactory;

  /// 用当前会议的完整转写生成结构化纪要，并写回本地。
  ///
  /// [title]、[meetingDate]、[participants]、[template] 是用户在生成前填写的抬头信息，
  /// 会一并持久化到会议记录，下次生成时自动带出。
  Future<Meeting> generate(
    String meetingId, {
    String? title,
    String? meetingDate,
    List<String>? participants,
    String? template,
  }) async {
    final meeting = await _dao.getMeeting(meetingId);
    if (meeting == null) {
      throw AppException('会议不存在');
    }
    final transcript = await _dao.buildTranscript(meetingId);
    if (transcript.trim().isEmpty) {
      throw AppException('还没有转写内容，无法生成纪要');
    }

    final effectiveTitle = (title == null || title.trim().isEmpty) ? meeting.title : title.trim();
    final effectiveParticipants = participants ?? meeting.participants;
    final effectiveDate = meetingDate ?? meeting.meetingDate;
    final effectiveTemplate = template ?? meeting.template;

    final result = await _apiFactory().generateMinutes(
      transcript: transcript,
      title: effectiveTitle,
      meetingDate: effectiveDate,
      participants: effectiveParticipants,
      template: effectiveTemplate,
    );

    final updated = meeting.copyWith(
      title: effectiveTitle,
      meetingDate: effectiveDate,
      participants: effectiveParticipants,
      template: effectiveTemplate,
      minutesJson: jsonEncode(result.raw),
      minutesMarkdown: result.markdown,
      minutesModel: result.model,
      updatedAt: DateTime.now(),
    );
    await _dao.saveMeeting(updated);
    return updated;
  }

  /// 导出 Word（后端渲染，手机端只负责保存与分享）。
  Future<File> exportDocx(Meeting meeting, {bool includeTranscript = false}) async {
    final raw = meeting.minutesJson;
    if (raw == null || raw.trim().isEmpty) {
      throw AppException('还没有生成纪要');
    }
    final minutes = jsonDecode(raw) as Map<String, dynamic>;
    final transcript = includeTranscript ? await _dao.buildTranscript(meeting.id) : null;

    final bytes = await _apiFactory().exportDocx(
      minutes: minutes,
      transcript: transcript,
      includeTranscript: includeTranscript,
    );

    final file = await AppPaths.exportFile('${sanitizeFileName(meeting.title)}-纪要.docx');
    await file.writeAsBytes(bytes, flush: true);
    return file;
  }

  /// 去掉文件名里的非法字符（跨平台安全）。
  static String sanitizeFileName(String name) {
    final cleaned = name.replaceAll(RegExp(r'[\\/:*?"<>|\s]+'), '_').trim();
    final trimmed = cleaned.length > 40 ? cleaned.substring(0, 40) : cleaned;
    return trimmed.isEmpty ? '会议' : trimmed;
  }
}
