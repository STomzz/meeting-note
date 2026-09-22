<script setup lang="ts">
// P2：四类模型端点配置 —— 本机加密存储 + 连通性测试
import { onMounted, reactive, ref } from 'vue'
import { MessagePlugin } from 'tdesign-vue-next'
import {
  CAPABILITIES,
  RECOMMENDED_PRESET,
  type Capability,
  type ModelConfigInput,
} from '../core/models'
import { useSettingsStore } from '../stores/settings'
import { isTauri } from '../platform'

const store = useSettingsStore()
const inTauri = isTauri()

interface Draft {
  baseUrl: string
  model: string
  apiKey: string
  paramsText: string
}

const drafts = reactive<Record<Capability, Draft>>({
  chat: { baseUrl: '', model: '', apiKey: '', paramsText: '' },
  embedding: { baseUrl: '', model: '', apiKey: '', paramsText: '' },
  rerank: { baseUrl: '', model: '', apiKey: '', paramsText: '' },
  asr: { baseUrl: '', model: '', apiKey: '', paramsText: '' },
})

const models = reactive<Record<string, string[]>>({})
const loadingModels = ref('')

onMounted(async () => {
  await store.load()
  fillFromConfig()
  devPrefill()
})

/** 开发模式便利：未配置的能力自动填入推荐端点与 .env.local 里的测试 Key（仍需手动点保存）。 */
function devPrefill() {
  if (!import.meta.env.DEV) return
  const key = import.meta.env.VITE_DEV_API_KEY as string | undefined
  for (const cap of CAPABILITIES) {
    const d = drafts[cap.key]
    if (store.config[cap.key]) {
      if (key && !store.config[cap.key]?.hasKey && !d.apiKey) d.apiKey = key
      continue
    }
    const p = RECOMMENDED_PRESET[cap.key]
    if (!p) continue
    d.baseUrl = p.baseUrl
    d.model = p.model
    const params = p.params as object
    d.paramsText = params && Object.keys(params).length ? JSON.stringify(params, null, 2) : ''
    if (key) d.apiKey = key
  }
}

function fillFromConfig() {
  for (const cap of CAPABILITIES) {
    const e = store.config[cap.key]
    const d = drafts[cap.key]
    d.baseUrl = e?.baseUrl ?? ''
    d.model = e?.model ?? ''
    d.apiKey = ''
    const p = e?.params
    d.paramsText =
      p && typeof p === 'object' && Object.keys(p as object).length
        ? JSON.stringify(p, null, 2)
        : ''
  }
}

function applyPreset() {
  for (const cap of CAPABILITIES) {
    const p = RECOMMENDED_PRESET[cap.key]
    if (!p) continue
    const d = drafts[cap.key]
    d.baseUrl = p.baseUrl
    d.model = p.model
    const params = p.params as object
    d.paramsText = params && Object.keys(params).length ? JSON.stringify(params, null, 2) : ''
  }
  void MessagePlugin.info('已填入集群网关推荐配置，补上 API Key 后点保存')
}

function buildInput(): ModelConfigInput | null {
  const out: ModelConfigInput = {}
  for (const cap of CAPABILITIES) {
    const d = drafts[cap.key]
    let params: unknown = {}
    const text = d.paramsText.trim()
    if (text) {
      try {
        params = JSON.parse(text)
      } catch {
        void MessagePlugin.error(`${cap.name}：附加参数不是合法 JSON`)
        return null
      }
    }
    // baseURL 留空 = 不启用该能力（后端据此移除配置）
    out[cap.key] = {
      baseUrl: d.baseUrl.trim(),
      model: d.model.trim(),
      params,
      apiKey: d.apiKey.trim() || null,
    }
  }
  return out
}

async function save(quiet = false): Promise<boolean> {
  const input = buildInput()
  if (!input) return false
  const ok = await store.save(input)
  if (ok) {
    fillFromConfig()
    if (!quiet) void MessagePlugin.success('配置已保存（API Key 本机加密存储）')
  } else {
    void MessagePlugin.error(store.savingMessage)
  }
  return ok
}

async function saveAndTest(cap: Capability) {
  const ok = await save(true)
  if (!ok) return
  await store.test(cap)
}

async function loadModels(cap: Capability) {
  const d = drafts[cap]
  if (!d.baseUrl.trim()) {
    void MessagePlugin.warning('先填写 baseURL')
    return
  }
  loadingModels.value = cap
  try {
    models[cap] = await store.listModels(d.baseUrl.trim(), d.apiKey.trim() || undefined)
    void MessagePlugin.success(`拉到 ${models[cap].length} 个模型`)
  } catch (e) {
    void MessagePlugin.error(String(e))
  } finally {
    loadingModels.value = ''
  }
}

function paramsPlaceholder(cap: Capability): string {
  if (cap === 'chat') return '{"chat_template_kwargs": {"enable_thinking": false}}'
  if (cap === 'asr') return '{"language": "zh"}'
  return '{}'
}
</script>

<template>
  <div class="page">
    <header class="page-header">
      <span class="page-title">设置</span>
      <span class="page-sub">模型端点、存储与备份</span>
      <span class="spacer" />
      <t-button size="small" :loading="store.saving" @click="applyPreset">填入推荐配置</t-button>
      <t-button size="small" variant="outline" @click="fillFromConfig">撤销修改</t-button>
      <t-button size="small" theme="primary" :loading="store.saving" @click="save()">保存配置</t-button>
    </header>

    <div class="settings-body">
      <div v-if="!inTauri" class="card tip">
        浏览器预览模式：配置只存在浏览器本地，不能发起真实请求（测试连接请在桌面客户端里做）。
      </div>

      <div class="card">
        <div class="card-title">模型配置</div>
        <div class="card-note">
          所有请求由本机客户端直连端点，不经过任何中间服务器；API Key 用本机密钥加密后存进本地数据库。
        </div>

        <div v-for="cap in CAPABILITIES" :key="cap.key" class="cap-card">
          <div class="cap-head">
            <span class="cap-name">{{ cap.name }}</span>
            <t-tag v-if="!cap.required" size="small" variant="light">可选</t-tag>
            <t-tag v-if="store.configured[cap.key]" size="small" theme="success" variant="light">
              已启用
            </t-tag>
            <t-tag v-if="store.config[cap.key]?.hasKey" size="small" variant="outline">
              Key 已保存
            </t-tag>
            <span class="spacer" />
            <t-button
              size="small"
              variant="text"
              :loading="loadingModels === cap.key"
              @click="loadModels(cap.key)"
            >
              拉取模型列表
            </t-button>
          </div>
          <div class="cap-hint">{{ cap.hint }}</div>

          <div class="fields">
            <div class="field">
              <span class="lbl">baseURL</span>
              <t-input
                v-model="drafts[cap.key].baseUrl"
                size="small"
                placeholder="https://chatapi.bnu.edu.cn"
              />
            </div>
            <div class="field">
              <span class="lbl">API Key</span>
              <t-input
                v-model="drafts[cap.key].apiKey"
                type="password"
                size="small"
                :placeholder="store.config[cap.key]?.hasKey ? '已保存（留空保持不变）' : 'sk-…'"
              />
            </div>
            <div class="field">
              <span class="lbl">模型</span>
              <t-select
                v-if="models[cap.key]?.length"
                v-model="drafts[cap.key].model"
                size="small"
                filterable
                creatable
                :options="models[cap.key].map((m) => ({ label: m, value: m }))"
                placeholder="选择或输入模型名"
              />
              <t-input
                v-else
                v-model="drafts[cap.key].model"
                size="small"
                :placeholder="cap.recommended"
              />
            </div>
            <div class="field">
              <span class="lbl">附加参数（JSON，可空）</span>
              <t-textarea
                v-model="drafts[cap.key].paramsText"
                size="small"
                :autosize="{ minRows: 1, maxRows: 4 }"
                :placeholder="paramsPlaceholder(cap.key)"
              />
            </div>
          </div>

          <div class="cap-foot">
            <t-button
              size="small"
              theme="primary"
              variant="outline"
              :loading="store.testing[cap.key]"
              :disabled="!inTauri"
              @click="saveAndTest(cap.key)"
            >
              保存并测试连接
            </t-button>
            <span v-if="store.results[cap.key]" class="test-result">
              {{ store.results[cap.key] }}
            </span>
          </div>
        </div>

        <div class="card-note">
          未配置 embedding / rerank 时，检索自动降级为全文搜索；未配置对话模型时，问答与会议纪要不可用，其余功能正常。
        </div>
      </div>

      <div class="card">
        <div class="card-title">存储与备份（P7 实现）</div>
        <div class="card-note">
          笔记 vault 目录、zip 导出/导入、数据占用统计。全部数据只保存在本机。
        </div>
      </div>
    </div>
  </div>
</template>

<style scoped>
.settings-body {
  padding: 18px;
  overflow: auto;
}

.card {
  background: var(--panel);
  border: 1px solid var(--border);
  border-radius: 10px;
  padding: 16px;
  margin-bottom: 16px;
  max-width: 880px;
}

.card.tip {
  background: #fffbe6;
  border-color: #ffe58f;
  color: #7c5e00;
  font-size: 12px;
}

.card-title {
  font-size: 15px;
  font-weight: 600;
  margin-bottom: 8px;
}

.card-note {
  font-size: 12px;
  color: var(--text-3);
  line-height: 1.7;
  margin-top: 10px;
}

.cap-card {
  border: 1px solid var(--border);
  border-radius: 8px;
  padding: 12px;
  margin-top: 12px;
}

.cap-head {
  display: flex;
  align-items: center;
  gap: 6px;
}

.cap-name {
  font-size: 14px;
  font-weight: 500;
}

.cap-hint {
  font-size: 12px;
  color: var(--text-3);
  margin: 6px 0 10px;
  line-height: 1.6;
}

.fields {
  display: grid;
  grid-template-columns: 1fr 1fr;
  gap: 10px 12px;
}

.field {
  display: flex;
  flex-direction: column;
  gap: 4px;
}

.lbl {
  font-size: 12px;
  color: var(--text-2);
}

.cap-foot {
  display: flex;
  align-items: center;
  gap: 10px;
  margin-top: 12px;
}

.test-result {
  font-size: 12px;
  color: var(--text-2);
  line-height: 1.6;
  word-break: break-all;
}

.spacer {
  flex: 1;
}

@media (max-width: 720px) {
  .fields {
    grid-template-columns: 1fr;
  }
}
</style>
