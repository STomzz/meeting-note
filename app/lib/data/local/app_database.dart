import 'package:path/path.dart' as p;
import 'package:sqflite/sqflite.dart';

/// SQLite 初始化与迁移（数据全部在手机本地，服务端不存任何内容）。
class AppDatabase {
  AppDatabase._();

  static final AppDatabase instance = AppDatabase._();

  /// v1: meetings / segments
  /// v2: meetings 增加纪要抬头字段（meeting_date / participants / template）
  static const int _version = 2;

  Database? _db;

  Future<Database> get database async {
    if (_db != null) return _db!;
    final dir = await getDatabasesPath();
    _db = await openDatabase(
      p.join(dir, 'bnu_meeting.db'),
      version: _version,
      onCreate: _onCreate,
      onUpgrade: _onUpgrade,
    );
    return _db!;
  }

  Future<void> _onCreate(Database db, int version) async {
    await db.execute('''
      CREATE TABLE meetings (
        id TEXT PRIMARY KEY,
        title TEXT NOT NULL,
        created_at INTEGER NOT NULL,
        ended_at INTEGER,
        status TEXT NOT NULL,
        minutes_json TEXT,
        minutes_markdown TEXT,
        minutes_model TEXT,
        updated_at INTEGER,
        meeting_date TEXT,
        participants TEXT,
        template TEXT
      )
    ''');
    await db.execute('''
      CREATE TABLE segments (
        id INTEGER PRIMARY KEY AUTOINCREMENT,
        meeting_id TEXT NOT NULL,
        seq INTEGER NOT NULL,
        file_path TEXT NOT NULL,
        duration_ms INTEGER NOT NULL DEFAULT 0,
        status TEXT NOT NULL,
        text TEXT,
        error TEXT,
        retry_count INTEGER NOT NULL DEFAULT 0,
        created_at INTEGER NOT NULL
      )
    ''');
    await db.execute('CREATE INDEX idx_segments_meeting ON segments(meeting_id, seq)');
  }

  Future<void> _onUpgrade(Database db, int oldVersion, int newVersion) async {
    if (oldVersion < 2) {
      await db.execute('ALTER TABLE meetings ADD COLUMN meeting_date TEXT');
      await db.execute('ALTER TABLE meetings ADD COLUMN participants TEXT');
      await db.execute('ALTER TABLE meetings ADD COLUMN template TEXT');
    }
  }
}
