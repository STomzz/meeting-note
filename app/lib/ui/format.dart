/// UI 里通用的小格式化工具。
library;

String two(int value) => value.toString().padLeft(2, '0');

String formatDateTime(DateTime time) =>
    '${time.year}-${two(time.month)}-${two(time.day)} ${two(time.hour)}:${two(time.minute)}';

String formatClock(Duration duration) =>
    '${two(duration.inHours)}:${two(duration.inMinutes % 60)}:${two(duration.inSeconds % 60)}';

String formatSeconds(int milliseconds) {
  final seconds = (milliseconds / 1000).round();
  if (seconds < 60) return '$seconds 秒';
  return '${seconds ~/ 60} 分 ${seconds % 60} 秒';
}
