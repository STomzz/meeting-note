import 'dart:async';
import 'dart:convert';

import 'package:flutter/material.dart';
import 'package:just_audio/just_audio.dart';
import 'package:provider/provider.dart';
import 'package:share_plus/share_plus.dart';

import '../../core/errors.dart';
import '../../data/models/meeting.dart';
import '../../data/models/minutes_doc.dart';
import '../../data/models/segment.dart';
import '../../state/services.dart';
import '../format.dart';

/// 会议详情：转写（可编辑、可试听）+ 结构化纪要 + 导出 Word。
class MeetingDetailScreen extends StatefulWidget {
  const MeetingDetailScreen({super.key, required this.meetingId});

  final String meetingId;

  @override
  State<MeetingDetailScreen> createState() => _MeetingDetailScreenState();
}

class _MeetingDetailScreenState extends State<MeetingDetailScreen> {
  final AudioPlayer _player = AudioPlayer();

  Meeting? _meeting;
  List<AudioSegment> _segments = const [];
  StreamSubscription<AudioSegment>? _queueSubscription;
  StreamSubscription<ProcessingState>? _playerStateSubscription;

  bool _generating = false;
  bool _exporting = false;
  bool _transcriptDirty = false;
  String? _error;

  /// 正在播放的分段序号（null = 未播放）
  int? _playingSeq;
  bool _paused = false;

  @override
  void initState() {
    super.initState();
    final services = context.read<AppServices>();
    _queueSubscription = services.uploadQueue.updates.listen((segment) {
      if (segment.meetingId == widget.meetingId) _load();
    });
    _playerStateSubscription = _player.processingStateStream.listen((state) {
      if (state == ProcessingState.completed) _playNext();
    });
    _load();
    // 重新进入会议时，把没转写完的分段继续排队
    unawaited(services.uploadQueue.resume(widget.meetingId));
  }

  @override
  void dispose() {
    _queueSubscription?.cancel();
    _playerStateSubscription?.cancel();
    _player.dispose();
    super.dispose();
  }

  // ------------------------------------------------------------------ 数据
  Future<void> _load() async {
    final services = context.read<AppServices>();
    final meeting = await services.dao.getMeeting(widget.meetingId);
    final segments = await services.dao.listSegments(widget.meetingId);
    if (!mounted) return;
    setState(() {
      _meeting = meeting;
      _segments = segments;
    });
  }

  // ---------------------------------------------------------------- 试听
  Future<void> _play(int seq) async {
    final segment = _segments.firstWhere((item) => item.seq == seq, orElse: () => _segments.first);
    try {
      await _player.setFilePath(segment.filePath);
      await _player.play();
      if (!mounted) return;
      setState(() {
        _playingSeq = segment.seq;
        _paused = false;
      });
    } catch (error) {
      if (mounted) setState(() => _error = '无法播放本段音频：$error');
    }
  }

  Future<void> _togglePause() async {
    if (_paused) {
      await _player.play();
    } else {
      await _player.pause();
    }
    if (mounted) setState(() => _paused = !_paused);
  }

  Future<void> _stopPlayback() async {
    await _player.stop();
    if (!mounted) return;
    setState(() {
      _playingSeq = null;
      _paused = false;
    });
  }

  /// 当前段播完 → 自动接下一段（连播整场会议）。
  void _playNext() {
    final current = _playingSeq;
    if (current == null) return;
    final next = _segments.where((segment) => segment.seq > current).toList();
    if (next.isEmpty) {
      unawaited(_stopPlayback());
      return;
    }
    unawaited(_play(next.first.seq));
  }

  // ---------------------------------------------------------------- 生成纪要
  Future<void> _openMinutesOptions() async {
    final meeting = _meeting;
    if (meeting == null) return;
    final options = await showModalBottomSheet<_MinutesOptions>(
      context: context,
      isScrollControlled: true,
      builder: (_) => _MinutesOptionsSheet(meeting: meeting),
    );
    if (options == null) return;
    await _generateMinutes(options);
  }

  Future<void> _generateMinutes([_MinutesOptions? options]) async {
    setState(() {
      _generating = true;
      _error = null;
    });
    try {
      final updated = await context.read<AppServices>().minutes.generate(
            widget.meetingId,
            title: options?.title,
            meetingDate: options?.meetingDate,
            participants: options?.participants,
            template: options?.template,
          );
      if (!mounted) return;
      setState(() {
        _meeting = updated;
        _transcriptDirty = false;
      });
      ScaffoldMessenger.of(context).showSnackBar(const SnackBar(content: Text('纪要已生成')));
    } on AppException catch (error) {
      if (mounted) setState(() => _error = error.message);
    } finally {
      if (mounted) setState(() => _generating = false);
    }
  }

  // ------------------------------------------------------------------ 编辑
  Future<void> _editSegment(AudioSegment segment) async {
    final controller = TextEditingController(text: segment.text ?? '');
    final text = await showDialog<String>(
      context: context,
      builder: (context) => AlertDialog(
        title: Text('修改第 ${segment.seq + 1} 段'),
        content: SizedBox(
          width: double.maxFinite,
          child: TextField(
            controller: controller,
            autofocus: true,
            maxLines: 8,
            minLines: 3,
            decoration: const InputDecoration(
              border: OutlineInputBorder(),
              hintText: '修正识别结果（人名、术语等）',
            ),
          ),
        ),
        actions: [
          TextButton(onPressed: () => Navigator.pop(context), child: const Text('取消')),
          FilledButton(
            onPressed: () => Navigator.pop(context, controller.text),
            child: const Text('保存'),
          ),
        ],
      ),
    );
    if (text == null || segment.id == null || !mounted) return;

    final services = context.read<AppServices>();
    await services.dao.updateSegmentText(segment.id!, text.trim());
    await services.dao.touchMeeting(widget.meetingId);
    await _load();
    if (!mounted) return;
    if (_meeting?.hasMinutes == true) {
      setState(() => _transcriptDirty = true);
    }
  }

  Future<void> _renameMeeting() async {
    final meeting = _meeting;
    if (meeting == null) return;
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
    final services = context.read<AppServices>();
    await services.dao.saveMeeting(meeting.copyWith(title: title, updatedAt: DateTime.now()));
    await _load();
  }

  // ------------------------------------------------------------------ 导出
  Future<void> _exportDocx({required bool includeTranscript}) async {
    final meeting = _meeting;
    if (meeting == null) return;
    setState(() {
      _exporting = true;
      _error = null;
    });
    try {
      final file = await context
          .read<AppServices>()
          .minutes
          .exportDocx(meeting, includeTranscript: includeTranscript);
      await SharePlus.instance.share(
        ShareParams(files: [XFile(file.path)], subject: meeting.title),
      );
    } on AppException catch (error) {
      if (mounted) setState(() => _error = error.message);
    } finally {
      if (mounted) setState(() => _exporting = false);
    }
  }

  Future<void> _retryFailed() async {
    final services = context.read<AppServices>();
    await services.uploadQueue.resume(widget.meetingId);
    await _load();
  }

  @override
  Widget build(BuildContext context) {
    final meeting = _meeting;
    final transcript = _segments
        .where((segment) => segment.hasText)
        .map((segment) => segment.text!.trim())
        .join('\n');
    final failed = _segments.where((segment) => segment.status == SegmentStatus.failed).length;
    final pending = _segments
        .where((segment) =>
            segment.status == SegmentStatus.pending || segment.status == SegmentStatus.uploading)
        .length;

    return DefaultTabController(
      length: 2,
      child: Scaffold(
        appBar: AppBar(
          title: Text(meeting?.title ?? '会议详情'),
          actions: [
            PopupMenuButton<String>(
              onSelected: (value) {
                switch (value) {
                  case 'generate':
                    _openMinutesOptions();
                  case 'rename':
                    _renameMeeting();
                  case 'docx':
                    _exportDocx(includeTranscript: false);
                  case 'docx_full':
                    _exportDocx(includeTranscript: true);
                }
              },
              itemBuilder: (_) => [
                PopupMenuItem(
                  value: 'generate',
                  enabled: transcript.isNotEmpty && !_generating,
                  child: Text(meeting?.hasMinutes == true ? '重新生成纪要…' : '生成纪要…'),
                ),
                const PopupMenuItem(value: 'rename', child: Text('重命名会议')),
                PopupMenuItem(
                  value: 'docx',
                  enabled: meeting?.hasMinutes == true && !_exporting,
                  child: const Text('导出 Word'),
                ),
                PopupMenuItem(
                  value: 'docx_full',
                  enabled: meeting?.hasMinutes == true && !_exporting,
                  child: const Text('导出 Word（含转写全文）'),
                ),
              ],
            ),
          ],
          bottom: const TabBar(
            tabs: [Tab(text: '转写'), Tab(text: '纪要')],
          ),
        ),
        body: Column(
          children: [
            if (_error != null)
              MaterialBanner(
                content: Text(_error!),
                actions: [
                  TextButton(
                    onPressed: () => setState(() => _error = null),
                    child: const Text('知道了'),
                  ),
                ],
              ),
            if (failed > 0)
              MaterialBanner(
                content: Text('有 $failed 段转写失败（音频已保存在本机，可重试）。'),
                actions: [
                  TextButton(onPressed: _retryFailed, child: const Text('重试')),
                ],
              ),
            if (_transcriptDirty)
              MaterialBanner(
                content: const Text('转写内容已修改，建议重新生成纪要。'),
                actions: [
                  TextButton(onPressed: _openMinutesOptions, child: const Text('重新生成')),
                ],
              ),
            if (pending > 0)
              LinearProgressIndicator(
                value: _segments.isEmpty ? null : (_segments.length - pending) / _segments.length,
              ),
            Expanded(
              child: TabBarView(
                children: [
                  _TranscriptTab(
                    segments: _segments,
                    transcript: transcript,
                    playingSeq: _playingSeq,
                    paused: _paused,
                    player: _player,
                    onToggle: (segment) {
                      if (_playingSeq == segment.seq) {
                        _togglePause();
                      } else {
                        _play(segment.seq);
                      }
                    },
                    onStop: _stopPlayback,
                    onEdit: _editSegment,
                  ),
                  _MinutesTab(
                    meeting: meeting,
                    generating: _generating,
                    onGenerate: _openMinutesOptions,
                    onExport: () => _exportDocx(includeTranscript: false),
                  ),
                ],
              ),
            ),
          ],
        ),
      ),
    );
  }
}

// ---------------------------------------------------------------------- 转写 tab
class _TranscriptTab extends StatelessWidget {
  const _TranscriptTab({
    required this.segments,
    required this.transcript,
    required this.playingSeq,
    required this.paused,
    required this.player,
    required this.onToggle,
    required this.onStop,
    required this.onEdit,
  });

  final List<AudioSegment> segments;
  final String transcript;
  final int? playingSeq;
  final bool paused;
  final AudioPlayer player;
  final void Function(AudioSegment segment) onToggle;
  final VoidCallback onStop;
  final void Function(AudioSegment segment) onEdit;

  @override
  Widget build(BuildContext context) {
    if (transcript.isEmpty) {
      return const Center(child: Text('还没有转写内容'));
    }
    return Column(
      children: [
        if (playingSeq != null)
          _PlayerBar(player: player, seq: playingSeq!, paused: paused, onStop: onStop),
        Expanded(
          child: ListView.separated(
            padding: const EdgeInsets.all(16),
            itemCount: segments.length,
            separatorBuilder: (_, __) => const SizedBox(height: 12),
            itemBuilder: (context, index) {
              final segment = segments[index];
              final playing = segment.seq == playingSeq;
              return Container(
                decoration: playing
                    ? BoxDecoration(
                        color: Theme.of(context).colorScheme.primaryContainer.withValues(alpha: 0.35),
                        borderRadius: BorderRadius.circular(8),
                      )
                    : null,
                padding: const EdgeInsets.symmetric(horizontal: 8, vertical: 6),
                child: Column(
                  crossAxisAlignment: CrossAxisAlignment.start,
                  children: [
                    Row(
                      children: [
                        Text(
                          '#${segment.seq + 1}  ${formatSeconds(segment.durationMs)}',
                          style: Theme.of(context).textTheme.bodySmall,
                        ),
                        const Spacer(),
                        IconButton(
                          tooltip: playing ? (paused ? '继续播放' : '暂停') : '试听本段',
                          visualDensity: VisualDensity.compact,
                          onPressed: () => onToggle(segment),
                          icon: Icon(
                            playing && !paused ? Icons.pause_circle : Icons.play_circle_outline,
                          ),
                        ),
                        IconButton(
                          tooltip: '修改本段文字',
                          visualDensity: VisualDensity.compact,
                          onPressed: () => onEdit(segment),
                          icon: const Icon(Icons.edit_outlined, size: 20),
                        ),
                      ],
                    ),
                    const SizedBox(height: 2),
                    SelectableText(
                      segment.hasText ? segment.text! : '（本段未识别到内容）',
                      style: const TextStyle(height: 1.45),
                    ),
                  ],
                ),
              );
            },
          ),
        ),
      ],
    );
  }
}

/// 播放状态条：显示进度并支持暂停/停止。
class _PlayerBar extends StatelessWidget {
  const _PlayerBar({
    required this.player,
    required this.seq,
    required this.paused,
    required this.onStop,
  });

  final AudioPlayer player;
  final int seq;
  final bool paused;
  final VoidCallback onStop;

  @override
  Widget build(BuildContext context) {
    return Material(
      color: Theme.of(context).colorScheme.surfaceContainerHighest,
      child: Padding(
        padding: const EdgeInsets.symmetric(horizontal: 12, vertical: 6),
        child: Row(
          children: [
            Icon(paused ? Icons.pause_circle : Icons.volume_up, size: 20),
            const SizedBox(width: 8),
            Expanded(
              child: StreamBuilder<Duration>(
                stream: player.positionStream,
                builder: (context, snapshot) {
                  final position = snapshot.data ?? Duration.zero;
                  final duration = player.duration ?? Duration.zero;
                  final value = duration.inMilliseconds == 0
                      ? 0.0
                      : (position.inMilliseconds / duration.inMilliseconds).clamp(0.0, 1.0);
                  return Column(
                    crossAxisAlignment: CrossAxisAlignment.start,
                    children: [
                      Text(
                        '正在播放第 ${seq + 1} 段 · ${formatClock(position)}',
                        style: Theme.of(context).textTheme.bodySmall,
                      ),
                      const SizedBox(height: 2),
                      LinearProgressIndicator(value: value, minHeight: 3),
                    ],
                  );
                },
              ),
            ),
            IconButton(
              tooltip: '停止播放',
              visualDensity: VisualDensity.compact,
              onPressed: onStop,
              icon: const Icon(Icons.stop_circle_outlined),
            ),
          ],
        ),
      ),
    );
  }
}

// ---------------------------------------------------------------------- 纪要 tab
class _MinutesTab extends StatelessWidget {
  const _MinutesTab({
    required this.meeting,
    required this.generating,
    required this.onGenerate,
    required this.onExport,
  });

  final Meeting? meeting;
  final bool generating;
  final VoidCallback onGenerate;
  final VoidCallback onExport;

  @override
  Widget build(BuildContext context) {
    final meeting = this.meeting;
    if (meeting == null || !meeting.hasMinutes) {
      return Center(
        child: Padding(
          padding: const EdgeInsets.all(32),
          child: Column(
            mainAxisAlignment: MainAxisAlignment.center,
            children: [
              const Icon(Icons.description_outlined, size: 64),
              const SizedBox(height: 16),
              const Text('用当前转写生成结构化纪要（议题/决议/待办/风险）'),
              const SizedBox(height: 20),
              FilledButton.icon(
                onPressed: generating ? null : onGenerate,
                icon: generating
                    ? const SizedBox(
                        width: 18,
                        height: 18,
                        child: CircularProgressIndicator(strokeWidth: 2),
                      )
                    : const Icon(Icons.auto_awesome),
                label: Text(generating ? '正在生成…' : '生成纪要'),
              ),
            ],
          ),
        ),
      );
    }

    final doc = MinutesDoc.fromJson(jsonDecodeObject(meeting.minutesJson ?? '{}'));

    return ListView(
      padding: const EdgeInsets.all(16),
      children: [
        Text(doc.title, style: Theme.of(context).textTheme.headlineSmall),
        const SizedBox(height: 4),
        Text(
          [
            if (doc.meetingDate != null) doc.meetingDate,
            if (doc.participants.isNotEmpty) '参会人：${doc.participants.join('、')}',
            if (meeting.minutesModel != null) '模型：${meeting.minutesModel}',
          ].join(' · '),
          style: Theme.of(context).textTheme.bodySmall,
        ),
        const SizedBox(height: 16),
        if (doc.overview.isNotEmpty)
          _Section(
            title: '会议摘要',
            child: Text(doc.overview, style: const TextStyle(height: 1.5)),
          ),
        if (doc.topics.isNotEmpty)
          _Section(
            title: '议题与讨论',
            child: Column(
              crossAxisAlignment: CrossAxisAlignment.start,
              children: [
                for (final topic in doc.topics) ...[
                  Text(topic.title, style: const TextStyle(fontWeight: FontWeight.w600)),
                  const SizedBox(height: 4),
                  for (final point in topic.points)
                    Padding(
                      padding: const EdgeInsets.only(left: 8, bottom: 2),
                      child: Text('· $point', style: const TextStyle(height: 1.45)),
                    ),
                  const SizedBox(height: 10),
                ],
              ],
            ),
          ),
        if (doc.decisions.isNotEmpty)
          _Section(
            title: '决议',
            child: Column(
              crossAxisAlignment: CrossAxisAlignment.start,
              children: [
                for (final decision in doc.decisions)
                  Padding(
                    padding: const EdgeInsets.only(bottom: 4),
                    child: Text('· $decision', style: const TextStyle(height: 1.45)),
                  ),
              ],
            ),
          ),
        if (doc.actionItems.isNotEmpty)
          _Section(
            title: '待办事项',
            child: Column(
              children: [
                for (final item in doc.actionItems)
                  Card(
                    margin: const EdgeInsets.only(bottom: 8),
                    child: ListTile(
                      dense: true,
                      title: Text(item.task),
                      subtitle: Text(
                        [
                          if (item.owner != null) '责任人：${item.owner}',
                          if (item.due != null) '期限：${item.due}',
                        ].join('   '),
                      ),
                    ),
                  ),
              ],
            ),
          ),
        if (doc.risks.isNotEmpty)
          _Section(
            title: '风险与遗留问题',
            child: Column(
              crossAxisAlignment: CrossAxisAlignment.start,
              children: [
                for (final risk in doc.risks)
                  Padding(
                    padding: const EdgeInsets.only(bottom: 4),
                    child: Text('· $risk', style: const TextStyle(height: 1.45)),
                  ),
              ],
            ),
          ),
        if (doc.nextSteps.isNotEmpty)
          _Section(
            title: '下一步',
            child: Column(
              crossAxisAlignment: CrossAxisAlignment.start,
              children: [
                for (final step in doc.nextSteps)
                  Padding(
                    padding: const EdgeInsets.only(bottom: 4),
                    child: Text('· $step', style: const TextStyle(height: 1.45)),
                  ),
              ],
            ),
          ),
        const SizedBox(height: 8),
        OutlinedButton.icon(
          onPressed: generating ? null : onGenerate,
          icon: const Icon(Icons.refresh),
          label: const Text('重新生成纪要'),
        ),
        const SizedBox(height: 8),
        FilledButton.icon(
          onPressed: onExport,
          icon: const Icon(Icons.download),
          label: const Text('导出 Word'),
        ),
        const SizedBox(height: 24),
      ],
    );
  }
}

class _Section extends StatelessWidget {
  const _Section({required this.title, required this.child});

  final String title;
  final Widget child;

  @override
  Widget build(BuildContext context) {
    return Padding(
      padding: const EdgeInsets.only(bottom: 20),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          Text(
            title,
            style: Theme.of(context).textTheme.titleMedium?.copyWith(fontWeight: FontWeight.bold),
          ),
          const Divider(),
          child,
        ],
      ),
    );
  }
}

// ------------------------------------------------------------------ 生成纪要弹层
class _MinutesOptions {
  const _MinutesOptions({
    required this.title,
    this.meetingDate,
    this.participants = const [],
    this.template,
  });

  final String title;
  final String? meetingDate;
  final List<String> participants;
  final String? template;
}

class _MinutesOptionsSheet extends StatefulWidget {
  const _MinutesOptionsSheet({required this.meeting});

  final Meeting meeting;

  @override
  State<_MinutesOptionsSheet> createState() => _MinutesOptionsSheetState();
}

class _MinutesOptionsSheetState extends State<_MinutesOptionsSheet> {
  late final TextEditingController _title;
  late final TextEditingController _date;
  late final TextEditingController _participants;
  late final TextEditingController _template;

  @override
  void initState() {
    super.initState();
    final meeting = widget.meeting;
    _title = TextEditingController(text: meeting.title);
    _date = TextEditingController(
      text: meeting.meetingDate ?? _formatDate(meeting.createdAt),
    );
    _participants = TextEditingController(text: meeting.participants.join('、'));
    _template = TextEditingController(text: meeting.template ?? '');
  }

  @override
  void dispose() {
    _title.dispose();
    _date.dispose();
    _participants.dispose();
    _template.dispose();
    super.dispose();
  }

  String _formatDate(DateTime time) =>
      '${time.year}-${two(time.month)}-${two(time.day)}';

  void _submit() {
    Navigator.of(context).pop(
      _MinutesOptions(
        title: _title.text.trim(),
        meetingDate: _date.text.trim(),
        participants: _participants.text
            .split(RegExp(r'[,，、;；\s]+'))
            .map((name) => name.trim())
            .where((name) => name.isNotEmpty)
            .toList(),
        template: _template.text.trim(),
      ),
    );
  }

  @override
  Widget build(BuildContext context) {
    return Padding(
      padding: EdgeInsets.only(
        left: 20,
        right: 20,
        top: 20,
        bottom: MediaQuery.of(context).viewInsets.bottom + 20,
      ),
      child: SingleChildScrollView(
        child: Column(
          mainAxisSize: MainAxisSize.min,
          crossAxisAlignment: CrossAxisAlignment.start,
          children: [
            Text('生成会议纪要', style: Theme.of(context).textTheme.titleLarge),
            const SizedBox(height: 4),
            Text(
              '这些信息会写进纪要抬头，留空也能生成。',
              style: Theme.of(context).textTheme.bodySmall,
            ),
            const SizedBox(height: 16),
            TextField(
              controller: _title,
              decoration: const InputDecoration(
                labelText: '会议标题',
                border: OutlineInputBorder(),
              ),
            ),
            const SizedBox(height: 12),
            TextField(
              controller: _date,
              decoration: const InputDecoration(
                labelText: '会议日期',
                hintText: '2026-09-21',
                border: OutlineInputBorder(),
              ),
            ),
            const SizedBox(height: 12),
            TextField(
              controller: _participants,
              decoration: const InputDecoration(
                labelText: '参会人（逗号/顿号分隔，可留空）',
                hintText: '张三、李四',
                border: OutlineInputBorder(),
              ),
            ),
            const SizedBox(height: 12),
            TextField(
              controller: _template,
              maxLines: 2,
              decoration: const InputDecoration(
                labelText: '附加要求（可选）',
                hintText: '例如：重点提炼风险与待办',
                border: OutlineInputBorder(),
              ),
            ),
            const SizedBox(height: 20),
            Row(
              children: [
                Expanded(
                  child: OutlinedButton(
                    onPressed: () => Navigator.of(context).pop(),
                    child: const Text('取消'),
                  ),
                ),
                const SizedBox(width: 12),
                Expanded(
                  child: FilledButton(
                    onPressed: _submit,
                    child: const Text('生成纪要'),
                  ),
                ),
              ],
            ),
          ],
        ),
      ),
    );
  }
}

/// 宽松解析 JSON 对象（纪要本地存档由后端写入，理论上总是合法）。
Map<String, dynamic> jsonDecodeObject(String raw) {
  try {
    final decoded = jsonDecode(raw);
    if (decoded is Map<String, dynamic>) return decoded;
  } catch (_) {
    // 忽略，返回空对象
  }
  return <String, dynamic>{};
}
