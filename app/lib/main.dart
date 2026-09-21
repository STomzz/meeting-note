import 'package:flutter/material.dart';
import 'package:flutter_foreground_task/flutter_foreground_task.dart';
import 'package:provider/provider.dart';

import 'app.dart';
import 'state/app_state.dart';
import 'state/services.dart';

Future<void> main() async {
  WidgetsFlutterBinding.ensureInitialized();

  // 前台服务与 UI 之间的通信端口（通知栏「停止录音」按钮用）
  FlutterForegroundTask.initCommunicationPort();

  final appState = AppState();
  await appState.init();
  final services = AppServices(apiFactory: appState.createApi);

  runApp(
    MultiProvider(
      providers: [
        ChangeNotifierProvider<AppState>.value(value: appState),
        Provider<AppServices>.value(value: services),
      ],
      child: const BnuMeetingApp(),
    ),
  );
}
