<script setup lang="ts">
// P4 阶段：实体/关系抽取 + 本地图存储 + 力导图可视化 + 图谱增强检索
</script>

<template>
  <div class="page">
    <header class="page-header">
      <span class="page-title">知识图谱</span>
      <span class="page-sub">从笔记中抽取实体与关系，本地存储与可视化</span>
    </header>
    <div class="empty">
      <p>计划在 P4 实现：</p>
      <p>· LLM 抽取实体/关系（带强度），按笔记增量更新</p>
      <p>· SQLite 本地图存储（entities / relations / chunk_entity）</p>
      <p>· 力导图交互：类型着色、强度线宽、点击联动笔记</p>
      <p>· 图谱增强检索：命中片段 → 关联实体 → 邻居扩展</p>
    </div>
  </div>
</template>
