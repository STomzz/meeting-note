import 'package:flutter/material.dart';
import 'package:provider/provider.dart';
import 'package:wakelock_plus/wakelock_plus.dart';

import '../../core/errors.dart';
import '../../data/models/segment.dart';
import '../../state/app_state.dart';
import '../../state/recording_controller.dart';
import '../../state/services.dart';
import '../format.dart';
import 'meeting_detail_screen.dart';

/// 录音页：边录边切段边转写，实时显示文字。
class RecordingScreen extends StatefulWidget {
  const RecordingScreen({super.key});

  @override
  State<RecordingScreen> createState() => _RecordingScreenState();
}

class _RecordingScreenState extends State<RecordingScreen> {
  late final RecordingController _controller;
  bool _stopping = false;

  @override
  void initState() {
    super.initState();
    final services = context.read<AppServices>();
    _controller = RecordingController(dao: services.dao, uploadQueue: services.uploadQueue)
      ..onStoppedExternally = _finishAndOpenDetail;
    WidgetsBinding.instance.addPostFrameCallback((_) => _start());
  }

  Future<void> _start() async {
    final keepScreenOn = context.read<AppState>().settings.keepScreenOn;
    try {
      // 前台服务负责锁屏保活；只有用户显式开启时才让屏幕常亮
      if (keepScreenOn) {
        await WakelockPlus.enable();
      }
      await _controller.start();
    } on AppException catch (error) {
      if (!mounted) return;
      ScaffoldMessenger.of(context).showSnackBar(SnackBar(content: Text(error.message)));
      Navigator.of(context).pop();
    }
  }

  /// 收尾（按钮停止 / 通知栏停止都走这里）。
  Future<void> _finishAndOpenDetail() async {
    if (_stopping) return;
    setState(() => _stopping = true);

    final wasRecording = _controller.recording;
    if (wasRecording) {
      await _controller.stop();
    }
    await WakelockPlus.disable();
    if (!mounted) return;

    final meetingId = _controller.meeting?.id;
    if (meetingId == null) {
      Navigator.of(context).pop();
      return;
    }
    Navigator.of(context).pushReplacement(
      MaterialPageRoute(builder: (_) => MeetingDetailScreen(meetingId: meetingId)),
    );
  }

  Future<void> _confirmStop() async {
    final shouldStop = await showDialog<bool>(
      context: context,
      builder: (context) => AlertDialog(
        title: const Text('结束录音？'),
        content: const Text('结束后会生成本次会议记录，未完成的分段会继续在后台转写。'),
        actions: [
          TextButton(onPressed: () => Navigator.pop(context, false), child: const Text('继续录音')),
          FilledButton(onPressed: () => Navigator.pop(context, true), child: const Text('结束')),
        ],
      ),
    );
    if (shouldStop == true) await _finishAndOpenDetail();
  }

  @override
  void dispose() {
    WakelockPlus.disable();
    _controller.dispose();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    return PopScope(
      canPop: false,
      onPopInvokedWithResult: (didPop, _) {
        if (!didPop) _confirmStop();
      },
      child: Scaffold(
        appBar: AppBar(
          title: ListenableBuilder(
            listenable: _controller,
            builder: (context, _) => Text('录音中 ${formatClock(_controller.elapsed)}'),
          ),
        ),
        body: ListenableBuilder(
          listenable: _controller,
          builder: (context, _) => Column(
            children: [
              _Header(controller: _controller),
              if (_controller.error != null)
                MaterialBanner(
                  content: Text(_controller.error!),
                  actions: [
                    TextButton(
                      onPressed: _controller.clearError,
                      child: const Text('知道了'),
                    ),
                  ],
                ),
              Expanded(child: _SegmentList(controller: _controller)),
            ],
          ),
        ),
        bottomNavigationBar: SafeArea(
          child: Padding(
            padding: const EdgeInsets.all(16),
            child: FilledButton.icon(
              onPressed: _stopping ? null : _confirmStop,
              icon: _stopping
                  ? const SizedBox(
                      width: 18,
                      height: 18,
                      child: CircularProgressIndicator(strokeWidth: 2),
                    )
                  : const Icon(Icons.stop_circle_outlined),
              label: Text(_stopping ? '正在收尾…' : '结束录音'),
              style: FilledButton.styleFrom(minimumSize: const Size.fromHeight(52)),
            ),
          ),
        ),
      ),
    );
  }
}

class _Header extends StatelessWidget {
  const _Header({required this.controller});

  final RecordingController controller;

  @override
  Widget build(BuildContext context) {
    final level = (controller.level / 3000).clamp(0.0, 1.0);
    return Padding(
      padding: const EdgeInsets.fromLTRB(16, 12, 16, 4),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          Row(
            children: [
              Icon(Icons.graphic_eq, color: Theme.of(context).colorScheme.primary),
              const SizedBox(width: 8),
              Expanded(
                child: ClipRRect(
                  borderRadius: BorderRadius.circular(4),
                  child: LinearProgressIndicator(value: level, minHeight: 6),
                ),
              ),
            ],
          ),
          const SizedBox(height: 8),
          Text(
            '锁屏也能继续录音；说话停顿约 0.6 秒即自动分段并转写，最长 45 秒强制切段。',
            style: Theme.of(context).textTheme.bodySmall,
          ),
        ],
      ),
    );
  }
}

class _SegmentList extends StatelessWidget {
  const _SegmentList({required this.controller});

  final RecordingController controller;

  @override
  Widget build(BuildContext context) {
    final segments = controller.segments.reversed.toList();
    if (segments.isEmpty) {
      return const Center(child: Text('正在听…'));
    }
    return ListView.separated(
      padding: const EdgeInsets.all(16),
      itemCount: segments.length,
      separatorBuilder: (_, __) => const SizedBox(height: 10),
      itemBuilder: (context, index) {
        final segment = segments[index];
        return Row(
          crossAxisAlignment: CrossAxisAlignment.start,
          children: [
            _StatusIcon(status: segment.status),
            const SizedBox(width: 12),
            Expanded(
              child: Column(
                crossAxisAlignment: CrossAxisAlignment.start,
                children: [
                  Text(
                    segment.hasText ? segment.text! : _placeholder(segment),
                    style: TextStyle(
                      height: 1.4,
                      color: segment.hasText ? null : Theme.of(context).colorScheme.outline,
                    ),
                  ),
                  const SizedBox(height: 2),
                  Text(
                    '#${segment.seq + 1} · ${formatSeconds(segment.durationMs)}',
                    style: Theme.of(context).textTheme.bodySmall,
                  ),
                ],
              ),
            ),
          ],
        );
      },
    );
  }

  String _placeholder(AudioSegment segment) {
    switch (segment.status) {
      case SegmentStatus.pending:
        return '排队中…';
      case SegmentStatus.uploading:
        return '转写中…';
      case SegmentStatus.failed:
        return '转写失败：${segment.error ?? '未知错误'}';
      case SegmentStatus.done:
        return '（本段没有识别到内容）';
    }
  }
}

class _StatusIcon extends StatelessWidget {
  const _StatusIcon({required this.status});

  final SegmentStatus status;

  @override
  Widget build(BuildContext context) {
    switch (status) {
      case SegmentStatus.pending:
        return const Icon(Icons.schedule, size: 20);
      case SegmentStatus.uploading:
        return const SizedBox(
          width: 20,
          height: 20,
          child: CircularProgressIndicator(strokeWidth: 2),
        );
      case SegmentStatus.failed:
        return Icon(Icons.error_outline, size: 20, color: Theme.of(context).colorScheme.error);
      case SegmentStatus.done:
        return Icon(Icons.check_circle_outline, size: 20, color: Colors.green.shade600);
    }
  }
}
