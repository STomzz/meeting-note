import 'package:flutter/foundation.dart';

import '../data/api/meeting_api.dart';
import '../data/local/app_database.dart';
import '../data/local/meeting_dao.dart';
import '../data/local/settings_store.dart';
import '../data/models/app_settings.dart';
import '../data/models/meeting.dart';

/// 全局状态：设置 + 会议列表 + 接口客户端工厂。
class AppState extends ChangeNotifier {
  AppState({SettingsStore? settingsStore, MeetingDao? dao})
      : _settingsStore = settingsStore ?? SettingsStore(),
        _dao = dao ?? MeetingDao(AppDatabase.instance);

  final SettingsStore _settingsStore;
  final MeetingDao _dao;

  AppSettings _settings = AppSettings.defaults();
  List<Meeting> _meetings = const [];
  bool _ready = false;

  AppSettings get settings => _settings;
  List<Meeting> get meetings => _meetings;
  bool get ready => _ready;

  /// 每次调用都拿最新设置构造客户端（设置页改完立刻生效）。
  MeetingApi createApi() => MeetingApi(settings: _settings);

  /// 用指定的（尚未保存的）设置构造客户端，用于设置页的「测试连接」。
  MeetingApi createApiFor(AppSettings settings) => MeetingApi(settings: settings);

  Future<void> init() async {
    _settings = await _settingsStore.load();
    await refreshMeetings();
    _ready = true;
    notifyListeners();
  }

  Future<void> updateSettings(AppSettings settings) async {
    _settings = settings;
    await _settingsStore.save(settings);
    notifyListeners();
  }

  Future<void> refreshMeetings() async {
    _meetings = await _dao.listMeetings();
    notifyListeners();
  }

  Future<void> saveMeeting(Meeting meeting) async {
    await _dao.saveMeeting(meeting);
    await refreshMeetings();
  }

  Future<Meeting?> getMeeting(String id) => _dao.getMeeting(id);

  Future<void> removeMeeting(String id) async {
    await _dao.deleteMeeting(id);
    await refreshMeetings();
  }
}
