import 'dart:io';

import 'package:file_picker/file_picker.dart';
import 'package:flutter/material.dart';
import 'package:provider/provider.dart';

import '../../core/errors.dart';
import '../../data/local/meeting_dao.dart';
import '../../data/models/meeting.dart';
import '../../domain/storage/app_paths.dart';
import '../../state/app_state.dart';
import '../../state/services.dart';
import '../format.dart';
import 'meeting_detail_screen.dart';
import 'recording_screen.dart';
import 'settings_screen.dart';

/// 首页：会议列表 + 开始录音 + 导入音频。
class HomeScreen extends StatefulWidget {
  const HomeScreen({super.key});

  @override
  State<HomeScreen> createState() => _HomeScreenState();
}

class _HomeScreenState extends State<HomeScreen> {
  Map<String, SegmentStats> _stats = const {};
  bool _busy = false;

  @override
  void initState() {
    super.initState();
    WidgetsBinding.instance.addPostFrameCallback((_) => _refresh());
  }

  Future<void> _refresh() async {
    final services = context.read<AppServices>();
    final appState = context.read<AppState>();
    await appState.refreshMeetings();
    final stats = await services.dao.segmentStats();
    if (mounted) setState(() => _stats = stats);
  }

  Future<void> _openSettings() async {
    await Navigator.of(context).push(
      MaterialPageRoute(builder: (_) => const SettingsScreen()),
    );
    if (mounted) await _refresh();
  }

  Future<void> _startRecording() async {
    final appState = context.read<AppState>();
    if (!appState.settings.hasApiKey) {
      _showSnack('请先在设置里填写 BNUAPI Key', action: '去设置', onAction: _openSettings);
      return;
    }
    await Navigator.of(context).push(
      MaterialPageRoute(builder: (_) => const RecordingScreen()),
    );
    if (mounted) await _refresh();
  }

  Future<void> _openMeeting(String meetingId) async {
    await Navigator.of(context).push(
      MaterialPageRoute(builder: (_) => MeetingDetailScreen(meetingId: meetingId)),
    );
    if (mounted) await _refresh();
  }

  Future<void> _importWav() async {
    final services = context.read<AppServices>();
    final picked = await FilePicker.pickFile(
      type: FileType.custom,
      allowedExtensions: ['wav'],
    );
    final path = picked?.path;
    if (path == null) return;

    setState(() => _busy = true);
    _showProgressDialog('正在导入音频…');
    try {
      final meeting = await services.importer.importWav(file: File(path));
      if (!mounted) return;
      Navigator.of(context, rootNavigator: true).pop(); // 关进度框
      await _openMeeting(meeting.id);
    } on AppException catch (error) {
      if (!mounted) return;
      Navigator.of(context, rootNavigator: true).pop();
      _showSnack(error.message);
    } finally {
      if (mounted) setState(() => _busy = false);
    }
  }

  Future<void> _rename(Meeting meeting) async {
    final controller = TextEditingController(text: meeting.title);
    final title = await showDialog<String>(
      context: context,
      builder: (context) => AlertDialog(
        title: const Text('重命名会议'),
        content: TextField(controller: controller, autofocus: true),
        actions: [
          TextButton(onPressed: () => Navigator.pop(context), child: const Text('取消')),
          FilledButton(
            onPressed: () => Navigator.pop(context, controller.text.trim()),
            child: const Text('保存'),
          ),
        ],
      ),
    );
    if (title == null || title.isEmpty || !mounted) return;
    await context.read<AppState>().saveMeeting(meeting.copyWith(title: title));
    await _refresh();
  }

  Future<void> _delete(Meeting meeting) async {
    final confirmed = await showDialog<bool>(
      context: context,
      builder: (context) => AlertDialog(
        title: const Text('删除会议'),
        content: Text('将删除「${meeting.title}」的录音、转写和纪要（仅本机）。'),
        actions: [
          TextButton(onPressed: () => Navigator.pop(context, false), child: const Text('取消')),
          FilledButton(
            onPressed: () => Navigator.pop(context, true),
            child: const Text('删除'),
          ),
        ],
      ),
    );
    if (confirmed != true || !mounted) return;

    final services = context.read<AppServices>();
    await services.dao.deleteMeeting(meeting.id);
    final dir = await AppPaths.meetingDirectory(meeting.id);
    if (await dir.exists()) {
      await dir.delete(recursive: true);
    }
    await _refresh();
  }

  void _showProgressDialog(String message) {
    showDialog<void>(
      context: context,
      barrierDismissible: false,
      builder: (_) => AlertDialog(
        content: Row(
          children: [
            const SizedBox(
              width: 22,
              height: 22,
              child: CircularProgressIndicator(strokeWidth: 2.5),
            ),
            const SizedBox(width: 16),
            Text(message),
          ],
        ),
      ),
    );
  }

  void _showSnack(String message, {String? action, VoidCallback? onAction}) {
    ScaffoldMessenger.of(context).showSnackBar(
      SnackBar(
        content: Text(message),
        action: action == null
            ? null
            : SnackBarAction(label: action, onPressed: onAction ?? () {}),
      ),
    );
  }

  @override
  Widget build(BuildContext context) {
    final appState = context.watch<AppState>();
    final meetings = appState.meetings;

    return Scaffold(
      appBar: AppBar(
        title: const Text('会议纪要'),
        actions: [
          IconButton(
            tooltip: '导入 WAV',
            onPressed: _busy ? null : _importWav,
            icon: const Icon(Icons.upload_file),
          ),
          IconButton(
            tooltip: '设置',
            onPressed: _openSettings,
            icon: const Icon(Icons.settings),
          ),
        ],
      ),
      floatingActionButton: FloatingActionButton.extended(
        onPressed: _startRecording,
        icon: const Icon(Icons.mic),
        label: const Text('开始录音'),
      ),
      body: Column(
        children: [
          if (!appState.settings.hasApiKey)
            MaterialBanner(
              content: const Text('还没有配置 BNUAPI Key，转写与纪要生成不可用。'),
              actions: [
                TextButton(onPressed: _openSettings, child: const Text('去设置')),
              ],
            ),
          Expanded(
            child: meetings.isEmpty
                ? _EmptyHint(onStart: _startRecording, onImport: _importWav)
                : RefreshIndicator(
                    onRefresh: _refresh,
                    child: ListView.separated(
                      itemCount: meetings.length,
                      separatorBuilder: (_, __) => const Divider(height: 1),
                      itemBuilder: (context, index) => _meetingTile(meetings[index]),
                    ),
                  ),
          ),
        ],
      ),
    );
  }

  Widget _meetingTile(Meeting meeting) {
    final stats = _stats[meeting.id];
    final subtitle = StringBuffer(formatDateTime(meeting.createdAt));
    if (stats != null && stats.total > 0) {
      subtitle.write(' · ${stats.done}/${stats.total} 段已转写');
    }
    if (meeting.status == MeetingStatus.recording) {
      subtitle.write(' · 录音未结束');
    }

    return ListTile(
      leading: CircleAvatar(
        backgroundColor: meeting.hasMinutes
            ? Theme.of(context).colorScheme.primaryContainer
            : null,
        child: Icon(meeting.hasMinutes ? Icons.description : Icons.graphic_eq),
      ),
      title: Text(meeting.title, maxLines: 1, overflow: TextOverflow.ellipsis),
      subtitle: Text(subtitle.toString()),
      trailing: PopupMenuButton<String>(
        onSelected: (value) {
          if (value == 'rename') _rename(meeting);
          if (value == 'delete') _delete(meeting);
        },
        itemBuilder: (_) => const [
          PopupMenuItem(value: 'rename', child: Text('重命名')),
          PopupMenuItem(value: 'delete', child: Text('删除')),
        ],
      ),
      onTap: () => _openMeeting(meeting.id),
    );
  }
}

class _EmptyHint extends StatelessWidget {
  const _EmptyHint({required this.onStart, required this.onImport});

  final VoidCallback onStart;
  final VoidCallback onImport;

  @override
  Widget build(BuildContext context) {
    return Center(
      child: Padding(
        padding: const EdgeInsets.all(32),
        child: Column(
          mainAxisAlignment: MainAxisAlignment.center,
          children: [
            Icon(Icons.mic_none, size: 72, color: Theme.of(context).colorScheme.outline),
            const SizedBox(height: 16),
            Text('点右下角开始录音，说话停顿即自动分段转写', style: Theme.of(context).textTheme.titleMedium),
            const SizedBox(height: 8),
            const Text('会议结束一键生成纪要，可导出 Word', textAlign: TextAlign.center),
            const SizedBox(height: 24),
            OutlinedButton.icon(
              onPressed: onImport,
              icon: const Icon(Icons.upload_file),
              label: const Text('导入已有 WAV 音频'),
            ),
          ],
        ),
      ),
    );
  }
}
