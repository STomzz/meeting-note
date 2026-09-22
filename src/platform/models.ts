import { invoke } from '@tauri-apps/api/core'
import type { Capability, ModelConfigInput, PublicModelConfig } from '../core/models'

/** 模型配置读写 + 连通性测试。 */
export interface ModelAdapter {
  getConfig(): Promise<PublicModelConfig>
  saveConfig(input: ModelConfigInput): Promise<PublicModelConfig>
  test(capability: Capability): Promise<string>
  listModels(baseUrl: string, apiKey?: string): Promise<string[]>
}

class TauriModelAdapter implements ModelAdapter {
  getConfig() {
    return invoke<PublicModelConfig>('get_model_config')
  }
  saveConfig(input: ModelConfigInput) {
    return invoke<PublicModelConfig>('save_model_config', { input })
  }
  test(capability: Capability) {
    return invoke<string>('test_model', { capability })
  }
  listModels(baseUrl: string, apiKey?: string) {
    return invoke<string[]>('list_available_models', { baseUrl, apiKey: apiKey ?? null })
  }
}

const LS_KEY = 'bnu-notes.model-config'

class BrowserModelAdapter implements ModelAdapter {
  private cache: PublicModelConfig = load()

  async getConfig() {
    return this.cache
  }
  async saveConfig(input: ModelConfigInput) {
    const next: PublicModelConfig = {}
    for (const cap of ['chat', 'embedding', 'rerank', 'asr'] as Capability[]) {
      const e = input[cap]
      if (!e || !e.baseUrl.trim()) continue
      next[cap] = {
        baseUrl: e.baseUrl,
        model: e.model,
        params: e.params,
        hasKey: Boolean(e.apiKey) || Boolean(this.cache[cap]?.hasKey),
      }
    }
    this.cache = next
    localStorage.setItem(LS_KEY, JSON.stringify(next))
    return next
  }
  async test(): Promise<string> {
    throw new Error('浏览器预览模式不能发起真实请求，请在桌面客户端里测试连接')
  }
  async listModels(): Promise<string[]> {
    return [
      'Qwen-Inno-35B-v1',
      'deepseek-v4.1-flash',
      'qwen3.8-27b',
      'glm5.3-flash',
      'bge-m3',
      'bge-reranker-v2-m3',
      'qwen3-asr-1.7b',
    ]
  }
}

function load(): PublicModelConfig {
  try {
    const raw = localStorage.getItem(LS_KEY)
    return raw ? (JSON.parse(raw) as PublicModelConfig) : {}
  } catch {
    return {}
  }
}

export function modelAdapter(): ModelAdapter {
  return '__TAURI_INTERNALS__' in window ? new TauriModelAdapter() : new BrowserModelAdapter()
}
