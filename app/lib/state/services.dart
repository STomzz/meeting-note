import '../data/api/meeting_api.dart';
import '../data/local/app_database.dart';
import '../data/local/meeting_dao.dart';
import '../domain/import/audio_importer.dart';
import '../domain/minutes/minutes_service.dart';
import '../domain/recording/upload_queue.dart';

/// 应用级服务容器：跨页面共享（上传队列在离开录音页后继续工作）。
class AppServices {
  AppServices({MeetingDao? dao, required MeetingApi Function() apiFactory})
      : dao = dao ?? MeetingDao(AppDatabase.instance) {
    uploadQueue = UploadQueue(dao: this.dao, apiFactory: apiFactory);
    minutes = MinutesService(dao: this.dao, apiFactory: apiFactory);
    importer = AudioImporter(dao: this.dao, uploadQueue: uploadQueue);
  }

  final MeetingDao dao;
  late final UploadQueue uploadQueue;
  late final MinutesService minutes;
  late final AudioImporter importer;
}
