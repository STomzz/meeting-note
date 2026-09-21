import '../../core/app_config.dart';

/// 应用设置（设置页可改；只持久化非敏感默认之外的内容）。
class AppSettings {
  const AppSettings({
    required this.backendBaseUrl,
    required this.upstreamBaseUrl,
    required this.apiKey,
    required this.asrModel,
    required this.minutesModel,
    this.keepScreenOn = false,
  });

  factory AppSettings.defaults() => const AppSettings(
        backendBaseUrl: AppConfig.defaultBackendBaseUrl,
        upstreamBaseUrl: AppConfig.defaultUpstreamBaseUrl,
        apiKey: '',
        asrModel: AppConfig.defaultAsrModel,
        minutesModel: AppConfig.defaultMinutesModel,
      );

  factory AppSettings.fromJson(Map<String, dynamic> json) => AppSettings(
        backendBaseUrl: json['backendBaseUrl'] as String? ?? AppConfig.defaultBackendBaseUrl,
        upstreamBaseUrl: json['upstreamBaseUrl'] as String? ?? AppConfig.defaultUpstreamBaseUrl,
        apiKey: json['apiKey'] as String? ?? '',
        asrModel: json['asrModel'] as String? ?? AppConfig.defaultAsrModel,
        minutesModel: json['minutesModel'] as String? ?? AppConfig.defaultMinutesModel,
        keepScreenOn: json['keepScreenOn'] as bool? ?? false,
      );

  final String backendBaseUrl;
  final String upstreamBaseUrl;
  final String apiKey;
  final String asrModel;
  final String minutesModel;

  /// 录音时是否强制屏幕常亮（默认关：前台服务已能保证锁屏录音）
  final bool keepScreenOn;

  bool get hasApiKey => apiKey.trim().isNotEmpty;

  AppSettings copyWith({
    String? backendBaseUrl,
    String? upstreamBaseUrl,
    String? apiKey,
    String? asrModel,
    String? minutesModel,
    bool? keepScreenOn,
  }) =>
      AppSettings(
        backendBaseUrl: backendBaseUrl ?? this.backendBaseUrl,
        upstreamBaseUrl: upstreamBaseUrl ?? this.upstreamBaseUrl,
        apiKey: apiKey ?? this.apiKey,
        asrModel: asrModel ?? this.asrModel,
        minutesModel: minutesModel ?? this.minutesModel,
        keepScreenOn: keepScreenOn ?? this.keepScreenOn,
      );

  Map<String, dynamic> toJson() => {
        'backendBaseUrl': backendBaseUrl,
        'upstreamBaseUrl': upstreamBaseUrl,
        'apiKey': apiKey,
        'asrModel': asrModel,
        'minutesModel': minutesModel,
        'keepScreenOn': keepScreenOn,
      };
}
