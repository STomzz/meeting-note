import { defineStore } from 'pinia'
import { modelAdapter } from '../platform/models'
import type { Capability, ModelConfigInput, PublicModelConfig } from '../core/models'

export const useSettingsStore = defineStore('settings', {
  state: () => ({
    config: {} as PublicModelConfig,
    loaded: false,
    saving: false,
    testing: {} as Record<string, boolean>,
    results: {} as Record<string, string>,
    savingMessage: '',
  }),

  getters: {
    configured(): Record<Capability, boolean> {
      const has = (c?: { baseUrl?: string } | null) => Boolean(c && (c.baseUrl ?? '').trim())
      return {
        chat: has(this.config.chat),
        embedding: has(this.config.embedding),
        rerank: has(this.config.rerank),
        asr: has(this.config.asr),
      }
    },
  },

  actions: {
    async load() {
      this.config = await modelAdapter().getConfig()
      this.loaded = true
    },

    async save(input: ModelConfigInput) {
      this.saving = true
      this.savingMessage = ''
      try {
        this.config = await modelAdapter().saveConfig(input)
        this.savingMessage = '已保存'
        return true
      } catch (e) {
        this.savingMessage = `保存失败：${String(e)}`
        return false
      } finally {
        this.saving = false
      }
    },

    async test(capability: Capability) {
      this.testing[capability] = true
      this.results[capability] = ''
      try {
        this.results[capability] = await modelAdapter().test(capability)
      } catch (e) {
        this.results[capability] = `❌ ${String(e)}`
      } finally {
        this.testing[capability] = false
      }
    },

    async listModels(baseUrl: string, apiKey?: string): Promise<string[]> {
      return modelAdapter().listModels(baseUrl, apiKey)
    },
  },
})
