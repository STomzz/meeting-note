<script setup lang="ts">
// P5 阶段：分段录音/播放 + 直连 ASR 转写 + 直连 LLM 纪要
</script>

<template>
  <div class="page">
    <header class="page-header">
      <span class="page-title">会议</span>
      <span class="page-sub">录音（可播放）→ 转写 → 纪要，全部保存本地</span>
    </header>
    <div class="empty">
      <p>计划在 P5 实现：</p>
      <p>· 分段录音（默认 4 分钟/段）+ 分段播放</p>
      <p>· 直连 ASR 端点逐段转写（WAV，带时间戳拼接）</p>
      <p>· 直连 LLM 生成结构化纪要（模板可编辑）</p>
      <p>· 导出 Markdown / 一键存为笔记</p>
    </div>
  </div>
</template>
