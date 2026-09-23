/** 模型配置相关类型（与 Rust 侧 camelCase 契约一致）。 */

export type Capability = 'chat' | 'embedding' | 'rerank' | 'asr'

export interface PublicEndpoint {
  baseUrl: string
  model: string
  params: unknown
  hasKey: boolean
}

export interface PublicModelConfig {
  chat?: PublicEndpoint | null
  embedding?: PublicEndpoint | null
  rerank?: PublicEndpoint | null
  asr?: PublicEndpoint | null
}

export interface EndpointInput {
  baseUrl: string
  model: string
  params: unknown
  /** 留空/不传表示保持已保存的 key 不变 */
  apiKey?: string | null
}

export interface ModelConfigInput {
  chat?: EndpointInput
  embedding?: EndpointInput
  rerank?: EndpointInput
  asr?: EndpointInput
}

export const CAPABILITIES: Array<{
  key: Capability
  name: string
  hint: string
  required: boolean
  recommended: string
}> = [
  {
    key: 'chat',
    name: '对话 / 纪要',
    hint: 'OpenAI 兼容 /chat/completions。问答与会议纪要使用；未配置时这两项不可用，其余功能正常。',
    required: true,
    recommended: 'Qwen-Inno-35B-v1（推理模型，建议关思考提速）',
  },
  {
    key: 'asr',
    name: '语音转写',
    hint: 'OpenAI 兼容 /audio/transcriptions（当前上游只支持 WAV）。',
    required: true,
    recommended: 'qwen3-asr-1.7b',
  },
  {
    key: 'embedding',
    name: '嵌入（可选）',
    hint: '配置后启用语义检索；不配置自动降级为全文搜索。',
    required: false,
    recommended: 'bge-m3（1024 维）',
  },
  {
    key: 'rerank',
    name: '重排（可选）',
    hint: '配置后对检索结果精排；不配置直接用融合排序。',
    required: false,
    recommended: 'bge-reranker-v2-m3',
  },
]

/** 集群网关的推荐预填（一键填入）。
 *
 * 2026-09-23 起公网入口的机器通道是 `/bnuapi/v1/*`（门户融合后 BNUAPI 控制台挪到 `/bnuapi`；
 * 同域的旧路径 `/v1/*` 仍兼容保留）——`api_root()` 会自动补 `/v1`。
 */
export const RECOMMENDED_PRESET: ModelConfigInput = {
  chat: {
    baseUrl: 'https://chatapi.bnu.edu.cn/bnuapi',
    model: 'Qwen-Inno-35B-v1',
    params: { chat_template_kwargs: { enable_thinking: false } },
  },
  embedding: { baseUrl: 'https://chatapi.bnu.edu.cn/bnuapi', model: 'bge-m3', params: {} },
  rerank: { baseUrl: 'https://chatapi.bnu.edu.cn/bnuapi', model: 'bge-reranker-v2-m3', params: {} },
  asr: { baseUrl: 'https://chatapi.bnu.edu.cn/bnuapi', model: 'qwen3-asr-1.7b', params: { language: 'zh' } },
}
