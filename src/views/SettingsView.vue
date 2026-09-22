<script setup lang="ts">
// P2 阶段：四类模型端点配置 + 连接测试 + 预设 + 安全存储
const capabilities = [
  { key: 'chat', name: '对话 / 纪要', hint: 'OpenAI 兼容 /chat/completions，例如 new-api 网关' },
  { key: 'embedding', name: '嵌入（可选）', hint: 'bge-m3 等，配置后启用语义检索' },
  { key: 'rerank', name: '重排（可选）', hint: 'bge-reranker-v2-m3 等，配置后提升答案质量' },
  { key: 'asr', name: '语音转写', hint: 'OpenAI 兼容 /audio/transcriptions，例如 qwen3-asr' },
]
</script>

<template>
  <div class="page">
    <header class="page-header">
      <span class="page-title">设置</span>
      <span class="page-sub">模型端点、存储与备份</span>
    </header>
    <div class="settings-body">
      <div class="card">
        <div class="card-title">模型配置（P2 实现）</div>
        <div class="cap-list">
          <div v-for="c in capabilities" :key="c.key" class="cap">
            <div class="cap-name">{{ c.name }}</div>
            <div class="cap-hint">{{ c.hint }}</div>
            <div class="cap-fields">
              <span class="field">baseURL</span>
              <span class="field">API Key</span>
              <span class="field">模型名</span>
              <span class="field">测试连接</span>
            </div>
          </div>
        </div>
        <div class="note">
          未配置 embedding / rerank 时，检索会自动降级为全文搜索；未配置 LLM 时问答与纪要不可用，其余功能正常。
        </div>
      </div>
      <div class="card">
        <div class="card-title">存储与备份（P7 实现）</div>
        <div class="note">
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
  max-width: 860px;
}

.card-title {
  font-size: 15px;
  font-weight: 600;
  margin-bottom: 12px;
}

.cap-list {
  display: grid;
  grid-template-columns: repeat(auto-fill, minmax(260px, 1fr));
  gap: 12px;
}

.cap {
  border: 1px dashed var(--border);
  border-radius: 8px;
  padding: 12px;
}

.cap-name {
  font-size: 14px;
  font-weight: 500;
}

.cap-hint {
  font-size: 12px;
  color: var(--text-3);
  margin: 6px 0 10px;
  line-height: 1.5;
}

.cap-fields {
  display: flex;
  flex-wrap: wrap;
  gap: 6px;
}

.field {
  font-size: 12px;
  color: var(--text-3);
  border: 1px solid var(--border);
  border-radius: 6px;
  padding: 2px 8px;
}

.note {
  font-size: 12px;
  color: var(--text-3);
  line-height: 1.7;
  margin-top: 12px;
}
</style>
