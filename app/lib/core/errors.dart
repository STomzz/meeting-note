/// 统一的异常类型：UI 只处理 [AppException]。
class AppException implements Exception {
  AppException(this.message, {this.code, this.statusCode});

  final String message;
  final String? code;
  final int? statusCode;

  bool get isAuthError => statusCode == 401;

  @override
  String toString() => message;
}
