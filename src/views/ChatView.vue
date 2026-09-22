<script setup lang="ts">
// P3 阶段：本地检索 + 模型问答（带引用回跳）
</script>

<template>
  <div class="page">
    <header class="page-header">
      <span class="page-title">问答</span>
      <span class="page-sub">基于本地笔记检索，调用你配置的 LLM 回答（带引用）</span>
    </header>
    <div class="empty">
      <p>计划在 P3 实现：</p>
      <p>· 关键词 + 向量混合检索（embedding 可选，未配置自动降级）</p>
      <p>· 可选重排（rerank）</p>
      <p>· 流式回答 + 引用回跳笔记行号</p>
    </div>
  </div>
</template>
