import { describe, expect, it } from 'vitest'
import { CAPABILITIES, RECOMMENDED_PRESET } from './models'

/**
 * 集群网关地址变更的回归护栏：2026-09-23 门户融合后机器通道挪到 `/bnuapi/v1/*`
 * （客户端 `api_root()` 会自动补 `/v1`），推荐预填必须跟着走。
 */
describe('RECOMMENDED_PRESET（集群网关默认地址）', () => {
  const KEY = 'https://chatapi.bnu.edu.cn/bnuapi'

  it('四项都预填新前缀 /bnuapi', () => {
    for (const cap of ['chat', 'embedding', 'rerank', 'asr'] as const) {
      expect(RECOMMENDED_PRESET[cap]?.baseUrl, cap).toBe(KEY)
    }
  })

  it('不写 /v1（交给 api_root 补，避免出现 /v1/v1）', () => {
    for (const cap of ['chat', 'embedding', 'rerank', 'asr'] as const) {
      expect(RECOMMENDED_PRESET[cap]?.baseUrl?.endsWith('/v1'), cap).toBe(false)
    }
  })

  it('对话默认关思考（Qwen-Inno-35B-v1）', () => {
    expect(RECOMMENDED_PRESET.chat?.model).toBe('Qwen-Inno-35B-v1')
    expect(RECOMMENDED_PRESET.chat?.params).toEqual({
      chat_template_kwargs: { enable_thinking: false },
    })
  })

  it('能力名与推荐模型对得上（设置页按 CAPABILITIES 渲染）', () => {
    const keys = CAPABILITIES.map((c) => c.key).sort()
    expect(keys).toEqual(['asr', 'chat', 'embedding', 'rerank'])
    expect(RECOMMENDED_PRESET.asr?.model).toBe('qwen3-asr-1.7b')
    expect(RECOMMENDED_PRESET.embedding?.model).toBe('bge-m3')
    expect(RECOMMENDED_PRESET.rerank?.model).toBe('bge-reranker-v2-m3')
  })
})
