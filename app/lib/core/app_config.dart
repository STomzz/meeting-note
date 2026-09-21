/// 全局常量与默认配置。
///
/// 用户只需要在设置页填「BNUAPI 地址 + Key」，其余都有合理默认值。
class AppConfig {
  AppConfig._();

  /// 后端服务（无状态编排层，部署在服务器）。
  ///
  /// 默认值面向本机开发；打包自己的服务器地址用编译期注入：
  /// `flutter build apk --release --dart-define=MMA_BACKEND_BASE_URL=http://你的地址:8000`
  /// （`app/scripts/build_apk.sh` 会自动读 `app/scripts/local.env`，见 docs/DEV_ENV.md）
  static const String defaultBackendBaseUrl = String.fromEnvironment(
    'MMA_BACKEND_BASE_URL',
    defaultValue: 'http://127.0.0.1:8000',
  );

  /// BNUAPI（OpenAI 兼容网关）
  static const String defaultUpstreamBaseUrl = 'https://chatapi.bnu.edu.cn/v1';

  static const String defaultAsrModel = 'qwen3-asr-1.7b';
  static const String defaultMinutesModel = 'Qwen-Inno-35B-v1';

  /// 录音参数：16kHz 单声道 16bit（与后端/ASR 对齐）
  static const int sampleRate = 16000;
  static const int channels = 1;
  static const int bitsPerSample = 16;

  /// 分段策略
  static const int minSegmentSeconds = 8;
  static const int maxSegmentSeconds = 45;
  static const int silenceCutMs = 600;
  static const int frameMs = 100;

  /// 上传
  static const int uploadMaxRetries = 3;
  static const int uploadMaxConcurrent = 2;

  /// 上游限流是「每用户 10 请求/分钟」；本地先限到 9，留 1 个余量给纪要生成
  static const int uploadRateLimitPerMinute = 9;

  /// 导入已有音频的大小上限
  static const int importMaxBytes = 200 * 1024 * 1024;
}
