import 'dart:convert';
import 'dart:io';

import 'package:http/http.dart' as http;

import '../../core/errors.dart';
import '../models/app_settings.dart';
import '../models/minutes_doc.dart';

/// 纪要生成结果：结构化对象 + 原始 JSON（导出用）+ 渲染好的 Markdown。
class MinutesResult {
  const MinutesResult({required this.doc, required this.raw, this.markdown, this.model});

  final MinutesDoc doc;
  final Map<String, dynamic> raw;
  final String? markdown;
  final String? model;
}

/// 后端接口客户端（后端无状态，只做编排；模型调用由后端转发给 BNUAPI）。
///
/// - `Authorization: Bearer <用户 Key>`
/// - `X-Upstream-Base-URL: <用户填写的 BNUAPI 地址>`
class MeetingApi {
  MeetingApi({required this.settings, http.Client? client}) : _client = client ?? http.Client();

  final AppSettings settings;
  final http.Client _client;

  Uri _uri(String path) => Uri.parse('${settings.backendBaseUrl.replaceAll(RegExp(r'/+$'), '')}$path');

  Map<String, String> _headers({bool json = false}) => {
        'Authorization': 'Bearer ${settings.apiKey.trim()}',
        'X-Upstream-Base-URL': settings.upstreamBaseUrl.trim(),
        if (json) 'Content-Type': 'application/json',
      };

  Future<Map<String, dynamic>> health() async {
    final response = await _send(() => _client.get(_uri('/health')));
    return _decodeJson(response);
  }

  /// 上游可用模型（校验 Key 是否有效，不消耗模型额度）。
  Future<List<String>> listUpstreamModels() async {
    final response = await _send(() => _client.get(_uri('/v1/upstream/models'), headers: _headers()));
    final payload = _decodeJson(response);
    final models = payload['models'];
    if (models is! List) {
      throw AppException('后端返回的模型列表异常');
    }
    return models.map((model) => '$model').toList();
  }

  /// 上传一个音频分段并取回转写文本。
  Future<String> transcribeSegment({
    required File file,
    required int seq,
    required String sessionId,
    String? prompt,
  }) async {
    final request = http.MultipartRequest('POST', _uri('/v1/segments/transcribe'))
      ..headers.addAll(_headers())
      ..fields['seq'] = '$seq'
      ..fields['session_id'] = sessionId
      ..fields['model'] = settings.asrModel.trim()
      ..files.add(await http.MultipartFile.fromPath('file', file.path));
    if (prompt != null && prompt.trim().isNotEmpty) {
      request.fields['prompt'] = prompt.trim();
    }

    final streamed = await _send(() => _client.send(request));
    final response = await http.Response.fromStream(streamed);
    final payload = _decodeJson(response);
    return (payload['text'] as String?)?.trim() ?? '';
  }

  /// 由完整转写文本生成结构化纪要。
  Future<MinutesResult> generateMinutes({
    required String transcript,
    String? title,
    String? meetingDate,
    List<String>? participants,
    String? template,
  }) async {
    final response = await _send(
      () => _client.post(
        _uri('/v1/minutes'),
        headers: _headers(json: true),
        body: jsonEncode({
          'transcript': transcript,
          if (title != null && title.isNotEmpty) 'title': title,
          if (meetingDate != null && meetingDate.isNotEmpty) 'meeting_date': meetingDate,
          if (participants != null && participants.isNotEmpty) 'participants': participants,
          if (template != null && template.isNotEmpty) 'template': template,
          'model': settings.minutesModel.trim(),
        }),
      ),
    );
    final payload = _decodeJson(response);
    final minutes = payload['minutes'];
    if (minutes is! Map<String, dynamic>) {
      throw AppException('后端返回的纪要结构异常');
    }
    return MinutesResult(
      doc: MinutesDoc.fromJson(minutes),
      raw: minutes,
      markdown: payload['markdown'] as String?,
      model: payload['model'] as String?,
    );
  }

  /// 请求后端把纪要渲染成 Word，返回字节流（由调用方保存/分享）。
  Future<List<int>> exportDocx({
    required Map<String, dynamic> minutes,
    String? transcript,
    bool includeTranscript = false,
  }) async {
    final response = await _send(
      () => _client.post(
        _uri('/v1/export/docx'),
        headers: _headers(json: true),
        body: jsonEncode({
          'minutes': minutes,
          if (transcript != null && transcript.isNotEmpty) 'transcript': transcript,
          'include_transcript': includeTranscript,
        }),
      ),
    );
    return response.bodyBytes;
  }

  // ------------------------------------------------------------------ 内部
  Future<T> _send<T>(Future<T> Function() action) async {
    if (!settings.hasApiKey) {
      throw AppException('请先在设置里填写 BNUAPI Key');
    }
    try {
      return await action();
    } on SocketException catch (error) {
      throw AppException('无法连接服务：${error.message}\n请检查手机是否在校园网内、后端地址是否正确');
    } on http.ClientException catch (error) {
      throw AppException('网络请求失败：${error.message}');
    }
  }

  Map<String, dynamic> _decodeJson(http.Response response) {
    final body = utf8.decode(response.bodyBytes, allowMalformed: true);
    Map<String, dynamic>? payload;
    try {
      final decoded = jsonDecode(body);
      if (decoded is Map<String, dynamic>) payload = decoded;
    } catch (_) {
      payload = null;
    }

    if (response.statusCode >= 400) {
      final error = payload?['error'];
      final message = error is Map ? '${error['message']}' : 'HTTP ${response.statusCode}';
      throw AppException(
        message,
        code: error is Map ? error['code'] as String? : null,
        statusCode: response.statusCode,
      );
    }

    if (payload == null) {
      throw AppException('后端返回了无法解析的内容');
    }
    return payload;
  }
}
