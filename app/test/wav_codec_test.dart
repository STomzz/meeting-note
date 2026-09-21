import 'dart:typed_data';

import 'package:flutter_test/flutter_test.dart';

import 'package:bnu_meeting/domain/audio/wav.dart';

void main() {
  group('WavCodec.build', () {
    test('生成标准 44 字节头与正确的长度字段', () {
      final wav = WavCodec.build(
        pcm: Uint8List(3200),
        sampleRate: 16000,
      );

      expect(wav.length, 44 + 3200);
      expect(String.fromCharCodes(wav.sublist(0, 4)), 'RIFF');
      expect(String.fromCharCodes(wav.sublist(8, 12)), 'WAVE');
      expect(String.fromCharCodes(wav.sublist(36, 40)), 'data');
      expect(WavCodec.durationMs(wav), 100); // 3200 字节 / 32000 Bps = 0.1s
    });
  });

  group('WavCodec.durationMs', () {
    test('解析 1 秒音频', () {
      final wav = WavCodec.build(pcm: Uint8List(32000), sampleRate: 16000);
      expect(WavCodec.durationMs(wav), 1000);
    });

    test('非法数据返回 null', () {
      expect(WavCodec.durationMs(Uint8List(100)), isNull);
    });
  });

  group('WavCodec.splitByDuration', () {
    test('按 5 秒切分 12 秒音频 → 3 段且每段都有合法头', () {
      final wav = WavCodec.build(pcm: Uint8List(32000 * 12), sampleRate: 16000);
      final chunks = WavCodec.splitByDuration(wav, const Duration(seconds: 5));

      expect(chunks.length, 3);
      expect(WavCodec.durationMs(chunks[0]), 5000);
      expect(WavCodec.durationMs(chunks[1]), 5000);
      expect(WavCodec.durationMs(chunks[2]), 2000);
    });

    test('短音频不切分', () {
      final wav = WavCodec.build(pcm: Uint8List(32000), sampleRate: 16000);
      expect(WavCodec.splitByDuration(wav, const Duration(minutes: 5)).length, 1);
    });
  });
}
