import 'package:flutter_foreground_task/flutter_foreground_task.dart';

/// 录音前台服务：锁屏/切后台时保持进程存活，并在通知栏显示录音状态。
///
/// 说明：真正的录音仍在主 isolate（record 插件）；这里只负责
/// 「保活 + 通知栏状态 + 停止按钮」，不参与音频处理。
class RecordingForegroundService {
  RecordingForegroundService._();

  static const String stopButtonId = 'stop_recording';
  static const int _serviceId = 1001;

  static const NotificationButton _stopButton =
      NotificationButton(id: stopButtonId, text: '停止录音');

  static bool _initialized = false;

  static void init() {
    if (_initialized) return;
    FlutterForegroundTask.init(
      androidNotificationOptions: AndroidNotificationOptions(
        channelId: 'bnu_meeting_recording',
        channelName: '录音服务',
        channelDescription: '会议录音进行中',
        channelImportance: NotificationChannelImportance.LOW,
        priority: NotificationPriority.LOW,
        onlyAlertOnce: true,
      ),
      iosNotificationOptions: const IOSNotificationOptions(showNotification: false),
      foregroundTaskOptions: ForegroundTaskOptions(
        // 不需要周期回调：录音与上传都在主 isolate，靠本服务保活
        eventAction: ForegroundTaskEventAction.nothing(),
        autoRunOnBoot: false,
        allowWakeLock: true,
        allowWifiLock: false,
        stopWithTask: true,
      ),
    );
    _initialized = true;
  }

  /// Android 13+ 通知权限；没有权限时服务仍能运行，只是通知栏不显示。
  static Future<void> ensureNotificationPermission() async {
    try {
      final permission = await FlutterForegroundTask.checkNotificationPermission();
      if (permission != NotificationPermission.granted) {
        await FlutterForegroundTask.requestNotificationPermission();
      }
    } catch (_) {
      // 通知权限拿不到不影响录音
    }
  }

  static Future<void> start({required String title}) async {
    init();
    try {
      if (await FlutterForegroundTask.isRunningService) {
        await FlutterForegroundTask.updateService(
          notificationTitle: '正在录音',
          notificationText: title,
        );
        return;
      }
      await ensureNotificationPermission();
      final result = await FlutterForegroundTask.startService(
        serviceId: _serviceId,
        serviceTypes: const [ForegroundServiceTypes.microphone],
        notificationTitle: '正在录音',
        notificationText: title,
        notificationButtons: const [_stopButton],
        callback: recordingServiceCallback,
      );
      if (result is ServiceRequestFailure) {
        // 服务起不来不影响录音本身（只是锁屏可能被打断），仅在调试日志里体现
        // ignore: avoid_print
        print('前台服务启动失败：${result.error}');
      }
    } catch (error) {
      // ignore: avoid_print
      print('前台服务启动异常：$error');
    }
  }

  static Future<void> updateText(String text) async {
    try {
      if (!await FlutterForegroundTask.isRunningService) return;
      await FlutterForegroundTask.updateService(notificationText: text);
    } catch (_) {
      // 忽略
    }
  }

  static Future<void> stop() async {
    try {
      if (!await FlutterForegroundTask.isRunningService) return;
      await FlutterForegroundTask.stopService();
    } catch (_) {
      // 忽略
    }
  }
}

/// 服务 isolate 入口：必须是顶层函数并标注 vm:entry-point。
@pragma('vm:entry-point')
void recordingServiceCallback() {
  FlutterForegroundTask.setTaskHandler(_RecordingTaskHandler());
}

class _RecordingTaskHandler extends TaskHandler {
  @override
  Future<void> onStart(DateTime timestamp, TaskStarter starter) async {}

  @override
  void onRepeatEvent(DateTime timestamp) {}

  @override
  Future<void> onDestroy(DateTime timestamp, bool isTimeout) async {}

  @override
  void onNotificationButtonPressed(String id) {
    // 通知栏按钮 → 主 isolate（录音在那里）
    FlutterForegroundTask.sendDataToMain({'action': id});
  }
}
