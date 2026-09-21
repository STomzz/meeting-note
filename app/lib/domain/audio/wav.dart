import 'dart:typed_data';

/// WAV（PCM 16bit）读写工具：纯 Dart，无第三方依赖，方便单测。
class WavCodec {
  WavCodec._();

  static const int _headerSize = 44;

  /// 为 PCM 数据加上标准 WAV 头。
  static Uint8List build({
    required Uint8List pcm,
    required int sampleRate,
    int channels = 1,
    int bitsPerSample = 16,
  }) {
    final byteRate = sampleRate * channels * bitsPerSample ~/ 8;
    final blockAlign = channels * bitsPerSample ~/ 8;
    final header = ByteData(_headerSize);

    void writeAscii(int offset, String value) {
      for (var i = 0; i < value.length; i++) {
        header.setUint8(offset + i, value.codeUnitAt(i));
      }
    }

    writeAscii(0, 'RIFF');
    header.setUint32(4, 36 + pcm.length, Endian.little);
    writeAscii(8, 'WAVE');
    writeAscii(12, 'fmt ');
    header.setUint32(16, 16, Endian.little);
    header.setUint16(20, 1, Endian.little); // PCM
    header.setUint16(22, channels, Endian.little);
    header.setUint32(24, sampleRate, Endian.little);
    header.setUint32(28, byteRate, Endian.little);
    header.setUint16(32, blockAlign, Endian.little);
    header.setUint16(34, bitsPerSample, Endian.little);
    writeAscii(36, 'data');
    header.setUint32(40, pcm.length, Endian.little);

    final output = Uint8List(_headerSize + pcm.length);
    output.setRange(0, _headerSize, header.buffer.asUint8List());
    output.setRange(_headerSize, output.length, pcm);
    return output;
  }

  /// 解析时长（毫秒）；解析失败返回 null。
  static int? durationMs(Uint8List bytes) {
    final info = _parse(bytes);
    if (info == null || info.byteRate <= 0) return null;
    return (info.dataLength / info.byteRate * 1000).round();
  }

  /// 按最大时长切分 WAV（用于导入长音频），返回若干独立 WAV。
  static List<Uint8List> splitByDuration(Uint8List bytes, Duration maxDuration) {
    final info = _parse(bytes);
    if (info == null) return [bytes];

    final bytesPerChunk = info.byteRate * maxDuration.inMilliseconds ~/ 1000;
    if (bytesPerChunk <= 0 || info.dataLength <= bytesPerChunk) return [bytes];

    final chunks = <Uint8List>[];
    final pcm = Uint8List.sublistView(bytes, info.dataOffset, info.dataOffset + info.dataLength);
    for (var offset = 0; offset < pcm.length; offset += bytesPerChunk) {
      final end = (offset + bytesPerChunk > pcm.length) ? pcm.length : offset + bytesPerChunk;
      chunks.add(
        build(
          pcm: Uint8List.sublistView(pcm, offset, end),
          sampleRate: info.sampleRate,
          channels: info.channels,
          bitsPerSample: info.bitsPerSample,
        ),
      );
    }
    return chunks;
  }

  static _WavInfo? _parse(Uint8List bytes) {
    if (bytes.length < _headerSize) return null;
    if (String.fromCharCodes(bytes.sublist(0, 4)) != 'RIFF') return null;
    if (String.fromCharCodes(bytes.sublist(8, 12)) != 'WAVE') return null;

    var offset = 12;
    int? sampleRate;
    int? channels;
    int? bitsPerSample;
    int? byteRate;
    int? dataOffset;
    int? dataLength;

    while (offset + 8 <= bytes.length) {
      final chunkId = String.fromCharCodes(bytes.sublist(offset, offset + 4));
      final view = ByteData.sublistView(bytes, offset + 4, offset + 8);
      final chunkSize = view.getUint32(0, Endian.little);
      final body = offset + 8;

      if (chunkId == 'fmt ' && body + 16 <= bytes.length) {
        final fmt = ByteData.sublistView(bytes, body, body + 16);
        channels = fmt.getUint16(2, Endian.little);
        sampleRate = fmt.getUint32(4, Endian.little);
        byteRate = fmt.getUint32(8, Endian.little);
        bitsPerSample = fmt.getUint16(14, Endian.little);
      } else if (chunkId == 'data') {
        dataOffset = body;
        dataLength = chunkSize.clamp(0, bytes.length - body);
        break;
      }

      offset = body + chunkSize + (chunkSize.isOdd ? 1 : 0);
    }

    if (sampleRate == null ||
        channels == null ||
        bitsPerSample == null ||
        byteRate == null ||
        dataOffset == null ||
        dataLength == null) {
      return null;
    }

    return _WavInfo(
      sampleRate: sampleRate,
      channels: channels,
      bitsPerSample: bitsPerSample,
      byteRate: byteRate,
      dataOffset: dataOffset,
      dataLength: dataLength,
    );
  }
}

class _WavInfo {
  const _WavInfo({
    required this.sampleRate,
    required this.channels,
    required this.bitsPerSample,
    required this.byteRate,
    required this.dataOffset,
    required this.dataLength,
  });

  final int sampleRate;
  final int channels;
  final int bitsPerSample;
  final int byteRate;
  final int dataOffset;
  final int dataLength;
}
