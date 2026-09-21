import 'dart:async';
import 'dart:math';
import 'dart:typed_data';

import 'package:record/record.dart';

import '../../core/app_config.dart';
import '../../core/errors.dart';
import '../audio/wav.dart';

/// 一个录好的分段（已在内存里打包成 WAV）。
class RecordedSegment {
  const RecordedSegment({required this.seq, required this.wavBytes, required this.durationMs});

  final int seq;
  final Uint8List wavBytes;
  final int durationMs;
}

/// 连续录音 + 静音自动切段。
///
/// 设计要点：
/// - 边录边切，段与段之间不中断（保证"上一段在上传、下一段继续录"）；
/// - 切点尽量落在静音处，避免把词切断；
/// - 段音频以 WAV 输出（后端/上游 ASR 只支持 WAV）。
class SegmentRecorder {
  SegmentRecorder();

  final AudioRecorder _recorder = AudioRecorder();

  final StreamController<RecordedSegment> _segmentController =
      StreamController<RecordedSegment>.broadcast(sync: true);
  final StreamController<double> _levelController = StreamController<double>.broadcast(sync: true);

  Stream<RecordedSegment> get segments => _segmentController.stream;
  Stream<double> get levels => _levelController.stream;

  StreamSubscription<Uint8List>? _subscription;

  final List<Uint8List> _frames = <Uint8List>[];
  final List<int> _carry = <int>[];
  double _noiseFloor = 150;
  int _silenceMs = 0;
  int _seq = 0;
  bool _recording = false;

  bool get isRecording => _recording;

  Future<void> start() async {
    if (_recording) return;
    if (!await _recorder.hasPermission()) {
      throw AppException('没有麦克风权限，请在系统设置中允许录音');
    }

    final stream = await _recorder.startStream(
      const RecordConfig(
        encoder: AudioEncoder.pcm16bits,
        sampleRate: AppConfig.sampleRate,
        numChannels: AppConfig.channels,
      ),
    );

    _frames.clear();
    _carry.clear();
    _silenceMs = 0;
    _seq = 0;
    _recording = true;
    _subscription = stream.listen(_onData, onError: _onError, cancelOnError: false);
  }

  /// 停止录音并吐出最后一段（若有效）。
  Future<void> stop() async {
    if (!_recording) return;
    _recording = false;
    await _subscription?.cancel();
    _subscription = null;
    await _recorder.stop();
    _flush(force: true);
    _emitLevel(0);
  }

  Future<void> dispose() async {
    await stop();
    await _segmentController.close();
    await _levelController.close();
    await _recorder.dispose();
  }

  // ------------------------------------------------------------------ 内部
  void _onError(Object error, StackTrace stackTrace) {
    _recording = false;
  }

  void _onData(Uint8List data) {
    _carry.addAll(data);
    const frameBytes = AppConfig.sampleRate * AppConfig.frameMs ~/ 1000 * 2;
    while (_carry.length >= frameBytes) {
      final frame = Uint8List.fromList(_carry.sublist(0, frameBytes));
      _carry.removeRange(0, frameBytes);
      _processFrame(frame);
    }
  }

  void _processFrame(Uint8List frame) {
    if (!_recording) return;
    final rms = _rms(frame);
    _emitLevel(rms);

    // 自适应噪声地板：安静时缓慢跟随
    if (rms < _noiseFloor * 2) {
      _noiseFloor = _noiseFloor * 0.95 + rms * 0.05;
    }
    const minThreshold = 220.0;
    final threshold = max(minThreshold, _noiseFloor * 2.5);

    if (rms < threshold) {
      _silenceMs += AppConfig.frameMs;
    } else {
      _silenceMs = 0;
    }

    _frames.add(frame);
    final segmentMs = _frames.length * AppConfig.frameMs;

    final shouldCutBySilence =
        _silenceMs >= AppConfig.silenceCutMs && segmentMs >= AppConfig.minSegmentSeconds * 1000;
    final shouldCutByLength = segmentMs >= AppConfig.maxSegmentSeconds * 1000;

    if (shouldCutBySilence || shouldCutByLength) {
      _flush();
    }
  }

  void _flush({bool force = false}) {
    if (_frames.isEmpty) return;
    final totalBytes = _frames.fold<int>(0, (sum, frame) => sum + frame.length);
    final durationMs = _frames.length * AppConfig.frameMs;

    if (durationMs < 1000 && !force) {
      _frames.clear();
      _silenceMs = 0;
      return;
    }

    final pcm = Uint8List(totalBytes);
    var offset = 0;
    for (final frame in _frames) {
      pcm.setRange(offset, offset + frame.length, frame);
      offset += frame.length;
    }

    _frames.clear();
    _silenceMs = 0;

    if (durationMs < 1000) return;
    _segmentController.add(
      RecordedSegment(
        seq: _seq++,
        wavBytes: WavCodec.build(pcm: pcm, sampleRate: AppConfig.sampleRate),
        durationMs: durationMs,
      ),
    );
  }

  double _rms(Uint8List frame) {
    final samples = frame.buffer.asInt16List(frame.offsetInBytes, frame.length ~/ 2);
    if (samples.isEmpty) return 0;
    var sum = 0.0;
    for (final sample in samples) {
      sum += sample * sample;
    }
    return sqrt(sum / samples.length);
  }

  void _emitLevel(double value) {
    if (!_levelController.isClosed) _levelController.add(value);
  }
}
