import 'package:sqflite/sqflite.dart';

import '../models/meeting.dart';
import '../models/segment.dart';
import 'app_database.dart';

/// 会议与分段的本地读写（唯一的数据访问入口）。
class MeetingDao {
  MeetingDao(this._database);

  final AppDatabase _database;

  Future<Database> get _db => _database.database;

  // ---------------------------------------------------------------- meetings
  Future<void> saveMeeting(Meeting meeting) async {
    final db = await _db;
    await db.insert('meetings', meeting.toRow(), conflictAlgorithm: ConflictAlgorithm.replace);
  }

  Future<List<Meeting>> listMeetings() async {
    final db = await _db;
    final rows = await db.query('meetings', orderBy: 'created_at DESC');
    return rows.map(Meeting.fromRow).toList();
  }

  Future<Meeting?> getMeeting(String id) async {
    final db = await _db;
    final rows = await db.query('meetings', where: 'id = ?', whereArgs: [id], limit: 1);
    return rows.isEmpty ? null : Meeting.fromRow(rows.first);
  }

  Future<void> deleteMeeting(String id) async {
    final db = await _db;
    await db.delete('meetings', where: 'id = ?', whereArgs: [id]);
    await db.delete('segments', where: 'meeting_id = ?', whereArgs: [id]);
  }

  // ---------------------------------------------------------------- segments
  Future<AudioSegment> insertSegment(AudioSegment segment) async {
    final db = await _db;
    final id = await db.insert('segments', segment.toRow());
    return segment.copyWith(id: id);
  }

  Future<void> updateSegment(AudioSegment segment) async {
    if (segment.id == null) return;
    final db = await _db;
    await db.update('segments', segment.toRow(), where: 'id = ?', whereArgs: [segment.id]);
  }

  /// 手工修正某段的转写文本（用户编辑）。
  Future<void> updateSegmentText(int segmentId, String text) async {
    final db = await _db;
    await db.update(
      'segments',
      {'text': text},
      where: 'id = ?',
      whereArgs: [segmentId],
    );
  }

  /// 标记会议有改动（用于"转写已修改"提示与排序）。
  Future<void> touchMeeting(String meetingId) async {
    final db = await _db;
    await db.update(
      'meetings',
      {'updated_at': DateTime.now().millisecondsSinceEpoch},
      where: 'id = ?',
      whereArgs: [meetingId],
    );
  }

  Future<List<AudioSegment>> listSegments(String meetingId) async {
    final db = await _db;
    final rows = await db.query(
      'segments',
      where: 'meeting_id = ?',
      whereArgs: [meetingId],
      orderBy: 'seq ASC',
    );
    return rows.map(AudioSegment.fromRow).toList();
  }

  Future<List<AudioSegment>> listUnfinishedSegments(String meetingId) async {
    final db = await _db;
    final rows = await db.query(
      'segments',
      where: 'meeting_id = ? AND status IN (?, ?)',
      whereArgs: [meetingId, SegmentStatus.pending.value, SegmentStatus.failed.value],
      orderBy: 'seq ASC',
    );
    return rows.map(AudioSegment.fromRow).toList();
  }

  /// 拼接某场会议的全部转写文本（按分段序号）。
  Future<String> buildTranscript(String meetingId) async {
    final segments = await listSegments(meetingId);
    return segments
        .where((segment) => segment.hasText)
        .map((segment) => segment.text!.trim())
        .join('\n');
  }

  /// 各会议的分段统计（首页展示用，一次聚合查询）。
  Future<Map<String, SegmentStats>> segmentStats() async {
    final db = await _db;
    final rows = await db.rawQuery('''
      SELECT meeting_id,
             COUNT(*) AS total,
             SUM(CASE WHEN status = 'done' THEN 1 ELSE 0 END) AS done,
             SUM(CASE WHEN status = 'failed' THEN 1 ELSE 0 END) AS failed
      FROM segments
      GROUP BY meeting_id
    ''');
    return {
      for (final row in rows)
        row['meeting_id'] as String: SegmentStats(
          total: row['total'] as int? ?? 0,
          done: row['done'] as int? ?? 0,
          failed: row['failed'] as int? ?? 0,
        ),
    };
  }
}

/// 分段统计。
class SegmentStats {
  const SegmentStats({required this.total, required this.done, required this.failed});

  final int total;
  final int done;
  final int failed;
}
