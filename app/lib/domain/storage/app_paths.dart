import 'dart:io';

import 'package:path/path.dart' as p;
import 'package:path_provider/path_provider.dart';

/// 本地文件布局：音频/导出都放在 App 私有目录，服务端不保存任何内容。
class AppPaths {
  AppPaths._();

  static Future<Directory> meetingDirectory(String meetingId) async {
    final base = await getApplicationDocumentsDirectory();
    final dir = Directory(p.join(base.path, 'meetings', meetingId));
    if (!await dir.exists()) {
      await dir.create(recursive: true);
    }
    return dir;
  }

  static Future<String> segmentPath(String meetingId, int seq) async {
    final dir = await meetingDirectory(meetingId);
    return p.join(dir.path, 'seg_${seq.toString().padLeft(4, '0')}.wav');
  }

  /// 导出文件（Word）放在独立目录，方便分享。
  static Future<File> exportFile(String fileName) async {
    final base = await getApplicationDocumentsDirectory();
    final dir = Directory(p.join(base.path, 'exports'));
    if (!await dir.exists()) {
      await dir.create(recursive: true);
    }
    return File(p.join(dir.path, fileName));
  }
}
