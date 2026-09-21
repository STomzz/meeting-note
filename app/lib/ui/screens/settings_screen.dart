import 'package:flutter/material.dart';
import 'package:provider/provider.dart';

import '../../core/app_config.dart';
import '../../core/errors.dart';
import '../../data/models/app_settings.dart';
import '../../state/app_state.dart';

/// 设置页：用户只需要填「BNUAPI 地址 + Key」；后端地址等放在高级里。
class SettingsScreen extends StatefulWidget {
  const SettingsScreen({super.key});

  @override
  State<SettingsScreen> createState() => _SettingsScreenState();
}

class _SettingsScreenState extends State<SettingsScreen> {
  late final TextEditingController _upstream;
  late final TextEditingController _apiKey;
  late final TextEditingController _backend;
  late final TextEditingController _asrModel;
  late final TextEditingController _minutesModel;

  bool _testing = false;
  String? _testMessage;
  bool _testOk = false;
  bool _keepScreenOn = false;

  @override
  void initState() {
    super.initState();
    final settings = context.read<AppState>().settings;
    _upstream = TextEditingController(text: settings.upstreamBaseUrl);
    _apiKey = TextEditingController(text: settings.apiKey);
    _backend = TextEditingController(text: settings.backendBaseUrl);
    _asrModel = TextEditingController(text: settings.asrModel);
    _minutesModel = TextEditingController(text: settings.minutesModel);
    _keepScreenOn = settings.keepScreenOn;
  }

  @override
  void dispose() {
    _upstream.dispose();
    _apiKey.dispose();
    _backend.dispose();
    _asrModel.dispose();
    _minutesModel.dispose();
    super.dispose();
  }

  AppSettings _collect() => AppSettings(
        backendBaseUrl: _backend.text.trim().isEmpty
            ? AppConfig.defaultBackendBaseUrl
            : _backend.text.trim(),
        upstreamBaseUrl: _upstream.text.trim().isEmpty
            ? AppConfig.defaultUpstreamBaseUrl
            : _upstream.text.trim(),
        apiKey: _apiKey.text.trim(),
        asrModel: _asrModel.text.trim().isEmpty ? AppConfig.defaultAsrModel : _asrModel.text.trim(),
        minutesModel: _minutesModel.text.trim().isEmpty
            ? AppConfig.defaultMinutesModel
            : _minutesModel.text.trim(),
        keepScreenOn: _keepScreenOn,
      );

  Future<void> _save() async {
    await context.read<AppState>().updateSettings(_collect());
    if (!mounted) return;
    ScaffoldMessenger.of(context).showSnackBar(const SnackBar(content: Text('已保存')));
    Navigator.of(context).pop();
  }

  /// 测试连接：用当前填写的 Key 拉一次上游模型列表（不消耗模型额度）。
  Future<void> _test() async {
    setState(() {
      _testing = true;
      _testMessage = null;
    });

    final settings = _collect();
    final api = context.read<AppState>().createApiFor(settings);
    try {
      final models = await api.listUpstreamModels();
      if (!mounted) return;
      final missing = <String>[];
      if (!models.contains(settings.asrModel)) missing.add('ASR 模型 ${settings.asrModel}');
      if (!models.contains(settings.minutesModel)) missing.add('纪要模型 ${settings.minutesModel}');
      setState(() {
        _testOk = missing.isEmpty;
        _testMessage = missing.isEmpty
            ? '连接成功，共 ${models.length} 个模型可用'
            : '连接成功（${models.length} 个模型），但未找到：${missing.join('、')}';
      });
    } on AppException catch (error) {
      if (!mounted) return;
      setState(() {
        _testOk = false;
        _testMessage = error.message;
      });
    } finally {
      if (mounted) setState(() => _testing = false);
    }
  }

  @override
  Widget build(BuildContext context) {
    return Scaffold(
      appBar: AppBar(title: const Text('设置')),
      body: ListView(
        padding: const EdgeInsets.all(16),
        children: [
          Text('BNUAPI（必填）', style: Theme.of(context).textTheme.titleMedium),
          const SizedBox(height: 8),
          TextField(
            controller: _upstream,
            decoration: const InputDecoration(
              labelText: 'BNUAPI 地址',
              hintText: AppConfig.defaultUpstreamBaseUrl,
              border: OutlineInputBorder(),
            ),
            keyboardType: TextInputType.url,
          ),
          const SizedBox(height: 12),
          TextField(
            controller: _apiKey,
            decoration: const InputDecoration(
              labelText: 'API Key',
              hintText: 'sk-...',
              border: OutlineInputBorder(),
            ),
            obscureText: true,
          ),
          const SizedBox(height: 12),
          Row(
            children: [
              FilledButton.tonalIcon(
                onPressed: _testing ? null : _test,
                icon: _testing
                    ? const SizedBox(
                        width: 16,
                        height: 16,
                        child: CircularProgressIndicator(strokeWidth: 2),
                      )
                    : const Icon(Icons.wifi_tethering),
                label: const Text('测试连接'),
              ),
            ],
          ),
          if (_testMessage != null)
            Padding(
              padding: const EdgeInsets.only(top: 8),
              child: Text(
                _testMessage!,
                style: TextStyle(
                  color: _testOk ? Colors.green.shade700 : Theme.of(context).colorScheme.error,
                ),
              ),
            ),
          const SizedBox(height: 24),
          Text('模型', style: Theme.of(context).textTheme.titleMedium),
          const SizedBox(height: 8),
          TextField(
            controller: _asrModel,
            decoration: const InputDecoration(
              labelText: 'ASR 模型 id',
              border: OutlineInputBorder(),
            ),
          ),
          const SizedBox(height: 12),
          TextField(
            controller: _minutesModel,
            decoration: const InputDecoration(
              labelText: '纪要模型 id',
              border: OutlineInputBorder(),
            ),
          ),
          const SizedBox(height: 24),
          Text('录音', style: Theme.of(context).textTheme.titleMedium),
          SwitchListTile(
            contentPadding: EdgeInsets.zero,
            title: const Text('录音时保持屏幕常亮'),
            subtitle: const Text('默认关闭：锁屏也能继续录音（前台服务）；开启更稳但更费电'),
            value: _keepScreenOn,
            onChanged: (value) => setState(() => _keepScreenOn = value),
          ),
          const SizedBox(height: 24),
          ExpansionTile(
            title: const Text('高级设置'),
            tilePadding: EdgeInsets.zero,
            children: [
              TextField(
                controller: _backend,
                decoration: const InputDecoration(
                  labelText: '后端服务地址',
                  helperText: '无状态编排层（默认已指向校内服务器）',
                  border: OutlineInputBorder(),
                ),
                keyboardType: TextInputType.url,
              ),
              const SizedBox(height: 8),
            ],
          ),
          const SizedBox(height: 16),
          FilledButton(onPressed: _save, child: const Text('保存')),
          const SizedBox(height: 16),
          Text(
            '说明：音频只用于转写，服务端不保存任何文件；录音、转写与纪要全部保存在手机本地。',
            style: Theme.of(context).textTheme.bodySmall,
          ),
        ],
      ),
    );
  }
}
