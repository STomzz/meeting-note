//! 真实网关联调测试（默认 `#[ignore]`，需要密钥，勿在 CI 里跑）。
//!
//! ```bash
//! BNU_TEST_API_KEY=sk-xxx \
//!   cargo test -p bnu-core --test gateway_live -- --ignored --nocapture
//! ```
//!
//! 可用环境变量覆盖：BNU_TEST_BASE_URL / BNU_TEST_API_KEY /
//! BNU_TEST_CHAT_MODEL / BNU_TEST_EMBED_MODEL / BNU_TEST_RERANK_MODEL / BNU_TEST_ASR_MODEL

use bnu_core::models::{self, ChatMessage, ChatOptions, EndpointConfig, ModelConfig};
use serde_json::json;

fn env_or(name: &str, default: &str) -> String {
    match std::env::var(name) {
        Ok(v) if !v.trim().is_empty() => v,
        _ => default.to_string(),
    }
}

fn endpoint(model_env: &str, model_default: &str, params: serde_json::Value) -> EndpointConfig {
    EndpointConfig {
        base_url: env_or("BNU_TEST_BASE_URL", "https://chatapi.bnu.edu.cn"),
        model: env_or(model_env, model_default),
        params,
        api_key: Some(env_or("BNU_TEST_API_KEY", "")),
    }
}

fn live_config() -> ModelConfig {
    ModelConfig {
        chat: Some(endpoint(
            "BNU_TEST_CHAT_MODEL",
            "Qwen-Inno-35B-v1",
            json!({ "chat_template_kwargs": { "enable_thinking": false } }),
        )),
        embedding: Some(endpoint("BNU_TEST_EMBED_MODEL", "bge-m3", json!({}))),
        rerank: Some(endpoint("BNU_TEST_RERANK_MODEL", "bge-reranker-v2-m3", json!({}))),
        asr: Some(endpoint("BNU_TEST_ASR_MODEL", "qwen3-asr-1.7b", json!({ "language": "zh" }))),
    }
}

fn assert_key(cfg: &ModelConfig) {
    let key = cfg
        .chat
        .as_ref()
        .and_then(|c| c.api_key.as_deref())
        .unwrap_or("");
    assert!(!key.is_empty(), "需要设置 BNU_TEST_API_KEY");
}

#[tokio::test]
#[ignore]
async fn live_list_models() {
    let cfg = live_config();
    assert_key(&cfg);
    let ids = models::list_models(cfg.chat.as_ref().unwrap()).await.unwrap();
    println!("模型列表（{} 个）: {}", ids.len(), ids.join(", "));
    assert!(!ids.is_empty());
}

#[tokio::test]
#[ignore]
async fn live_chat_returns_content_without_thinking() {
    let cfg = live_config();
    assert_key(&cfg);
    let started = std::time::Instant::now();
    let reply = models::chat(
        cfg.chat.as_ref().unwrap(),
        &[ChatMessage::user("只回复两个字：正常")],
        &ChatOptions { max_tokens: Some(64), ..Default::default() },
    )
    .await
    .unwrap();
    println!(
        "chat {} ms | finish={} | tokens={:?} | content={}",
        started.elapsed().as_millis(),
        reply.finish_reason,
        reply.completion_tokens,
        reply.content
    );
    assert!(!reply.content.trim().is_empty());
}

#[tokio::test]
#[ignore]
async fn live_embed_rerank_and_asr() {
    let cfg = live_config();
    assert_key(&cfg);

    let vectors = models::embed(
        cfg.embedding.as_ref().unwrap(),
        &["向量检索".to_string(), "会议纪要".to_string()],
    )
    .await
    .unwrap();
    let dim = vectors.first().map(|v| v.len()).unwrap_or(0);
    println!("embed: {} 条，维度 {}", vectors.len(), dim);
    assert!(dim > 0);

    let docs = vec![
        "向量检索通过嵌入相似度召回语义相近的片段".to_string(),
        "今天的午饭是牛肉面".to_string(),
    ];
    let hits = models::rerank(cfg.rerank.as_ref().unwrap(), "什么是向量检索", &docs, Some(2))
        .await
        .unwrap();
    for h in &hits {
        println!("rerank: #{} score={:.6}", h.index, h.score);
    }
    assert_eq!(hits.first().map(|h| h.index), Some(0));

    let wav = models::silent_wav(0.5, 16000);
    let text = models::transcribe(cfg.asr.as_ref().unwrap(), wav, "probe.wav")
        .await
        .unwrap();
    println!("asr: 静音 0.5s -> {:?}", text);
}

#[tokio::test]
#[ignore]
async fn live_test_capability_all() {
    let cfg = live_config();
    assert_key(&cfg);
    for cap in ["chat", "embedding", "rerank", "asr"] {
        let started = std::time::Instant::now();
        match models::test_capability(cap, &cfg).await {
            Ok(msg) => println!("[{cap}] {} ms -> {msg}", started.elapsed().as_millis()),
            Err(e) => panic!("[{cap}] 失败: {e}"),
        }
    }
}

/// 端到端：录音分段（合成会议音频）→ 静音切段 → 转写 → 纪要生成。
#[tokio::test]
#[ignore]
async fn live_meeting_transcribe_and_minutes() {
    use bnu_core::{audio, db, meetings, minutes, transcribe};
    use std::sync::atomic::AtomicBool;
    use std::sync::Mutex;

    let cfg = live_config();
    assert_key(&cfg);
    let audio_path = std::env::var("BNU_TEST_AUDIO")
        .unwrap_or_else(|_| "/tmp/opencode/tts16k.wav".to_string());
    let src = std::fs::read(&audio_path)
        .unwrap_or_else(|e| panic!("需要一段中文语音 WAV（可用 BNU_TEST_AUDIO 指定）：{audio_path}: {e}"));

    // 拼一场"会议"：语音 + 2s 静音 ×3
    let pcm = audio::normalize_to_16k_mono(&src).unwrap();
    let silence = vec![0u8; 16_000 * 2 * 2];
    let mut all: Vec<u8> = Vec::new();
    for i in 0..3 {
        all.extend_from_slice(&pcm);
        if i < 2 {
            all.extend_from_slice(&silence);
        }
    }
    let wav = audio::build_wav(&all, 16_000, 1, 16);
    let chunks = audio::split_speech(&wav).unwrap();
    let durations: Vec<u64> = chunks.iter().map(|c| c.duration_ms).collect();
    println!("静音切段：{} 段，时长 {:?}", chunks.len(), durations);
    assert!(chunks.len() >= 2, "应在静音处切开");

    // 写入会议分段（模拟前端录音）
    let dir = tempfile::tempdir().unwrap();
    let conn = db::open_memory().unwrap();
    let m = meetings::create(&conn, "联调会议").unwrap();
    meetings::start_segment(&conn, dir.path(), &m.id, 1, 16_000).unwrap();
    meetings::append_pcm(&conn, dir.path(), &m.id, 1, 16_000, &all).unwrap();
    meetings::close_segment(&conn, dir.path(), &m.id, 1).unwrap();
    let mm = Mutex::new(conn);

    let cancel = AtomicBool::new(false);
    let out = transcribe::transcribe_meeting(
        &mm,
        dir.path(),
        &cfg,
        &m.id,
        &|p| {
            if !p.message.is_empty() {
                println!("  progress: {}", p.message);
            }
        },
        &cancel,
    )
    .await
    .unwrap();
    println!(
        "转写：{} 块，失败 {} 段，{} ms\n{}",
        out.chunks, out.failed_segments, out.elapsed_ms, out.transcript
    );
    assert_eq!(out.failed_segments, 0, "转写失败: {:?}", out.errors);
    assert!(!out.transcript.trim().is_empty());

    let res = minutes::generate(&mm, &cfg, &m.id).await.unwrap();
    println!(
        "纪要（{}，{} ms，修复={}，map-reduce={}）：\n{}",
        res.model, res.elapsed_ms, res.repaired, res.used_map_reduce, res.markdown
    );
    assert!(!res.minutes.overview.trim().is_empty(), "摘要不应为空");
    assert!(res.markdown.starts_with('#'));
}

/// 端到端：索引 → 构建向量 → 混合检索 + 重排 → 问答（带引用）。
#[tokio::test]
#[ignore]
async fn live_qa_end_to_end() {
    use bnu_core::{db, notes, qa, retrieval, vectors};
    use std::sync::Mutex;

    let cfg = live_config();
    assert_key(&cfg);
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("周会.md"),
        "# 周会纪要\n\n2026-09-20 周会：讨论了镜像拉取超时的问题，决定改用镜像站并重试；下周三前完成部署。\n",
    )
    .unwrap();
    std::fs::write(
        dir.path().join("读书.md"),
        "# 读书笔记\n\n今天读了 Rust 所有权的章节，理解了借用检查。\n",
    )
    .unwrap();

    let conn = db::open_memory().unwrap();
    let stats = notes::scan_vault(&conn, dir.path()).unwrap();
    println!("索引：{} 篇", stats.indexed);
    let m = Mutex::new(conn);

    let progress = vectors::build(&m, cfg.embedding.as_ref().unwrap(), 16, 64).await.unwrap();
    println!(
        "向量索引：embedded={} remaining={} dim={} {}ms",
        progress.embedded, progress.remaining, progress.dim, progress.elapsed_ms
    );
    assert_eq!(progress.remaining, 0);

    let status = {
        let c = m.lock().unwrap();
        retrieval::status(&c, &cfg).unwrap()
    };
    println!(
        "状态：chunks={} vectors={} ready={}",
        status.chunks, status.vectors, status.vector_ready
    );
    assert!(status.vector_ready);

    let answer = qa::answer(&m, &cfg, "镜像拉取超时最后是怎么解决的？", 6).await.unwrap();
    println!(
        "模式={} 耗时={}ms tokens={:?}\n回答：{}",
        answer.trace.mode, answer.elapsed_ms, answer.completion_tokens, answer.answer
    );
    for (i, s) in answer.sources.iter().enumerate() {
        println!(
            "  [{}] {} 第 {}-{} 行 来源={:?}",
            i + 1,
            s.note_id,
            s.start_line,
            s.end_line,
            s.sources
        );
    }
    assert!(!answer.answer.is_empty());
    assert!(answer.sources.iter().any(|s| s.note_id == "周会.md"));
    assert_eq!(answer.trace.mode, "hybrid+rerank");
}

/// 端到端：两篇中文笔记 → LLM 抽取实体/关系 → 图查询（并校验实体名都在原文里）。
#[tokio::test]
#[ignore]
async fn live_graph_extract_and_query() {
    use bnu_core::{db, graph, notes};
    use std::sync::atomic::AtomicBool;
    use std::sync::Mutex;

    let cfg = live_config();
    assert_key(&cfg);

    let note_a = "# 项目周会纪要\n\n\
2026年9月18日，北京师范大学人工智能学院召开了知识库项目周会，会议由张伟主持，李娜、王强参加。\n\n\
会上确定了三件事：\n\
1. 李娜负责在十月底前完成 Milvus 向量库的部署，部署节点为 gpu-node2。\n\
2. 王强负责对接 new-api 网关，把 Qwen-Inno-35B-v1 接入知识库项目。\n\
3. 张伟提出用 AntV G6 实现知识图谱可视化，并在下一次评审上演示。\n\n\
会议决定：知识库项目采用本地优先的架构，笔记数据保存在用户本地，云端只提供模型能力。\n";
    let note_b = "# 部署记录\n\n\
李娜在 gpu-node2 上完成了 Milvus 部署，用 Docker 启动，端口是 19530。\n\
王强在 new-api 网关里新建了渠道，模型别名为 Qwen-Inno-35B-v1。\n";

    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("周会.md"), note_a).unwrap();
    std::fs::write(dir.path().join("部署记录.md"), note_b).unwrap();
    let conn = db::open_memory().unwrap();
    let stats = notes::scan_vault(&conn, dir.path()).unwrap();
    println!("索引：{} 篇", stats.indexed);
    let m = Mutex::new(conn);

    let cancel = AtomicBool::new(false);
    let out = graph::extract_all(
        &m,
        &cfg,
        false,
        &|p| {
            if !p.message.is_empty() {
                println!("  progress: {}", p.message);
            }
        },
        &cancel,
    )
    .await
    .unwrap();
    println!(
        "抽取：完成 {} 跳过 {} 失败 {}｜批次 {} 实体 {} 关系 {}｜{} ms\n错误：{:?}",
        out.notes_done,
        out.notes_skipped,
        out.notes_failed,
        out.batches,
        out.entities,
        out.relations,
        out.elapsed_ms,
        out.errors
    );
    assert_eq!(out.notes_failed, 0, "抽取失败: {:?}", out.errors);
    assert!(out.entities >= 4, "实体太少：{}", out.entities);
    assert!(out.relations >= 3, "关系太少：{}", out.relations);

    // 增量：第二次应全部跳过、零调用
    let out2 = graph::extract_all(&m, &cfg, false, &|_| {}, &cancel).await.unwrap();
    println!("二次抽取：done={} skipped={} batches={}", out2.notes_done, out2.notes_skipped, out2.batches);
    assert_eq!((out2.notes_done, out2.notes_skipped, out2.batches), (0, 2, 0));

    let (snap, top, detail) = {
        let c = m.lock().unwrap();
        let snap = graph::snapshot(&c, 500, 0).unwrap();
        let top = snap
            .nodes
            .iter()
            .max_by_key(|n| (n.note_count, n.degree))
            .cloned()
            .unwrap();
        let detail = graph::node_detail(&c, top.id).unwrap();
        (snap, top, detail)
    };
    println!("图：节点 {} 边 {}（截断 {}）", snap.nodes.len(), snap.edges.len(), snap.truncated);
    for n in &snap.nodes {
        println!("  节点 [{}] {}（{} 篇笔记，度 {})", n.kind, n.name, n.note_count, n.degree);
    }
    for e in snap.edges.iter().take(15) {
        println!("  边 {} -> {} · {} × {}", e.src, e.dst, e.kinds, e.weight);
    }
    assert!(snap.nodes.len() >= 4, "节点太少：{}", snap.nodes.len());
    assert!(!snap.edges.is_empty(), "没有边");

    // 防幻觉：所有实体名都必须是原文子串
    let hay = graph::normalize_name(&format!("{note_a}\n{note_b}"));
    for n in &snap.nodes {
        assert!(hay.contains(&graph::normalize_name(&n.name)), "实体「{}」不在原文里", n.name);
    }

    // 跨笔记实体：李娜/王强在两篇里都出现
    let cross = snap.nodes.iter().filter(|n| n.note_count >= 2).count();
    println!("跨笔记实体：{cross} 个（最多的是「{}」）", top.name);
    assert!(cross >= 1, "应抽到跨笔记的公共实体");

    println!("「{}」的邻居：", top.name);
    for nb in detail.neighbors.iter().take(8) {
        println!("  {} {} · {} × {}", nb.direction, nb.name, nb.rel_kind, nb.weight);
    }
    println!("「{}」的出处：", top.name);
    for men in detail.mentions.iter().take(5) {
        println!("  {} 第 {}-{} 行｜{}", men.note_id, men.start_line, men.end_line, men.snippet);
    }
    assert!(!detail.neighbors.is_empty() && !detail.mentions.is_empty());
}

/// 端到端：抽取图谱 → 邻居扩展生效（检索与问答的 trace 都带标注）。
#[tokio::test]
#[ignore]
async fn live_graph_enhanced_retrieval() {
    use bnu_core::{db, graph, notes, qa, retrieval, vectors};
    use std::sync::atomic::AtomicBool;
    use std::sync::Mutex;

    let cfg = live_config();
    assert_key(&cfg);

    // A 命中查询；B 语义相近；C 只与 A/B 共享实体「张伟」（多跳目标）
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("A.md"), "# 周会\n\n镜像拉取超时的问题由张伟跟进，下周给结论。\n").unwrap();
    std::fs::write(
        dir.path().join("B.md"),
        "# 部署记录\n\n张伟在 gpu-node3 上用 hf-mirror.com 验证断点续传，问题已解决。\n",
    )
    .unwrap();
    std::fs::write(
        dir.path().join("C.md"),
        "# 读书笔记\n\n张伟摘抄：沈括在《梦溪笔谈》里记录过指南针的用法。\n",
    )
    .unwrap();

    let conn = db::open_memory().unwrap();
    notes::scan_vault(&conn, dir.path()).unwrap();
    let m = Mutex::new(conn);

    let cancel = AtomicBool::new(false);
    let out = graph::extract_all(&m, &cfg, false, &|_| {}, &cancel).await.unwrap();
    println!("抽取：实体 {} 关系 {} 失败 {}｜{:?}", out.entities, out.relations, out.notes_failed, out.errors);
    assert!(out.entities > 0, "图谱没有抽到实体");

    if let Some(ec) = cfg.embedding.as_ref() {
        let p = vectors::build(&m, ec, 16, 64).await.unwrap();
        println!("向量索引：{} 条（dim {}）", p.embedded, p.dim);
    }

    // ① 机制证明：只用全文 + 只取 1 条候选，看图谱能否把「共享实体」的另两篇补回来
    let pure = retrieval::RetrieveOptions {
        top_k: 5,
        candidate_k: 1,
        use_vectors: false,
        use_rerank: false,
        ..Default::default()
    };
    let (chunks, trace) = retrieval::retrieve(&m, &cfg, "镜像拉取超时", &pure).await.unwrap();
    println!(
        "[机制] mode={} fts={}｜图谱 entities={} hits={} added={}",
        trace.mode, trace.fts_hits, trace.graph_entities, trace.graph_hits, trace.graph_added
    );
    for c in &chunks {
        println!("  [{}] {} sources={:?}", c.chunk_id, c.note_id, c.sources);
    }
    assert_eq!(trace.fts_hits, 1, "全文应只命中 A.md");
    assert!(trace.graph_entities > 0, "命中片段没有关联实体 → 图谱没参与");
    assert!(trace.graph_hits > 0, "图谱邻居扩展没有召回候选（检查抽取是否漏了「张伟」）");
    assert!(trace.graph_added > 0, "扩展候选没进入最终结果");
    assert!(trace.mode.ends_with("+graph"), "mode 应标注 +graph：{}", trace.mode);
    assert!(
        chunks.iter().any(|c| c.sources == vec!["graph".to_string()]),
        "应有片段来源标注为 graph"
    );

    // ② 完整链路（向量 + 重排 + 图谱）
    let opts = retrieval::RetrieveOptions { top_k: 5, candidate_k: 2, ..Default::default() };
    let (chunks, trace) = retrieval::retrieve(
        &m,
        &cfg,
        "镜像拉取超时的问题是谁跟进、怎么解决的？",
        &opts,
    )
    .await
    .unwrap();
    println!(
        "[完整] mode={} fts={} vector={}｜图谱 entities={} hits={} added={}",
        trace.mode, trace.fts_hits, trace.vector_hits, trace.graph_entities, trace.graph_hits, trace.graph_added
    );
    for c in &chunks {
        println!("  [{}] {} 第 {}-{} 行 sources={:?}", c.chunk_id, c.note_id, c.start_line, c.end_line, c.sources);
    }
    assert!(trace.graph_entities > 0, "混合检索下图谱也应参与");

    let answer = qa::answer(&m, &cfg, "镜像拉取超时的问题是谁跟进、怎么解决的？", 5).await.unwrap();
    println!(
        "问答：mode={} graph_added={} 耗时={}ms\n{}",
        answer.trace.mode, answer.trace.graph_added, answer.elapsed_ms, answer.answer
    );
    assert!(!answer.answer.is_empty());
}


/// 会议笔记端到端（P7）：新建 md → `/v` 引用两段录音 → 一键处理（转写 + 纪要）→ 幂等重跑。
#[tokio::test]
#[ignore]
async fn live_meeting_note_process() {
    use bnu_core::{audio, audio_clip, db, meeting_note, notes};
    use std::sync::atomic::AtomicBool;
    use std::sync::Mutex;

    let cfg = live_config();
    assert_key(&cfg);
    let audio_path = std::env::var("BNU_TEST_AUDIO")
        .unwrap_or_else(|_| "/tmp/opencode/tts16k.wav".to_string());
    let src = std::fs::read(&audio_path).unwrap_or_else(|e| {
        panic!("需要一段中文语音 WAV（可用 BNU_TEST_AUDIO 指定）：{audio_path}: {e}")
    });
    let pcm = audio::normalize_to_16k_mono(&src).unwrap();
    let silence = vec![0u8; 16_000 * 2 * 2]; // 2s 静音

    let tmp = tempfile::tempdir().unwrap();
    let vault = tmp.path();
    let conn = db::open_memory().unwrap();

    let (note_id, body) = meeting_note::create_note(&conn, vault, "联调周会", "2026-09-22").unwrap();
    println!("新建会议笔记：{note_id}");

    // 录两段（模拟录音面板落盘）
    let clip_dir = audio_clip::dir_for_note(&note_id);
    let mut refs: Vec<String> = Vec::new();
    for seq in 1..=2i64 {
        audio_clip::start(vault, &clip_dir, seq, 16_000).unwrap();
        let mut all = pcm.clone();
        all.extend_from_slice(&silence);
        all.extend_from_slice(&pcm);
        audio_clip::append(vault, &clip_dir, seq, &all).unwrap();
        let stat = audio_clip::close(vault, &clip_dir, seq).unwrap();
        println!("录音 seg_{seq:04}.wav：{:.1}s，{} 字节", stat.duration_ms as f64 / 1000.0, stat.bytes);
        refs.push(format!("/v {}", audio_clip::rel_path(&clip_dir, seq)));
    }

    // 手写正文 + 插在纪要段之前的两条引用
    let handwritten = "我在会上记的：镜像拉取超时的问题要换镜像站；下周一补部署文档。";
    let insertion = format!(
        "## 我的记录\n\n{handwritten}\n{}\n\n第二段录音记的是结论部分。\n{}\n\n",
        refs[0], refs[1]
    );
    let with_notes =
        body.replace(meeting_note::MINUTES_HEADING, &format!("{insertion}{}", meeting_note::MINUTES_HEADING));
    notes::write_note(&conn, vault, &note_id, &with_notes).unwrap();

    let mm = Mutex::new(conn);
    let cancel = AtomicBool::new(false);
    let run = |force: bool| {
        let mm = &mm;
        let cancel = &cancel;
        let vault = vault;
        let cfg = &cfg;
        let note_id = note_id.clone();
        async move {
            meeting_note::process(
                mm,
                vault,
                cfg,
                &note_id,
                force,
                &|p: meeting_note::ProcessProgress| {
                    if !p.message.is_empty() {
                        println!("  progress[{}]: {}", p.phase, p.message);
                    }
                },
                cancel,
            )
            .await
            .unwrap()
        }
    };

    let out = run(false).await;
    println!(
        "处理：转写 {}（复用 {}、失败 {}），转写 {} 字，纪要 {} 字，{} ms，模型 {}",
        out.audio_done,
        out.audio_skipped,
        out.audio_failed,
        out.transcript_chars,
        out.minutes_chars,
        out.elapsed_ms,
        out.model
    );
    assert_eq!(out.audio_failed, 0, "有音频处理失败: {:?}", out.errors);
    assert_eq!(out.audio_done, 2, "两段都应转写: {:?}", out.errors);
    assert!(out.minutes_chars > 0, "应生成纪要: {:?}", out.errors);

    let final_body = notes::read_note(vault, &note_id).unwrap();
    println!("--- 处理后的笔记 ---\n{final_body}\n--- 结束 ---");
    assert!(final_body.contains("🎙 转写"), "应有转写块");
    assert_eq!(final_body.matches("🎙 转写").count(), 2, "每个引用一个转写块");
    assert!(final_body.contains(handwritten), "手写内容必须原样保留");
    assert!(!final_body.contains(meeting_note::MINUTES_HINT), "占位提示应被纪要替换");
    assert!(final_body.contains("会议纪要（AI 整理）"));

    // 幂等重跑：音频全部命中缓存，笔记不重复堆叠
    let out2 = run(false).await;
    println!(
        "重跑：转写 {}（复用 {}、失败 {}），{} ms",
        out2.audio_done, out2.audio_skipped, out2.audio_failed, out2.elapsed_ms
    );
    assert_eq!(out2.audio_done, 0, "重跑不应再调 ASR: {:?}", out2.errors);
    assert_eq!(out2.audio_skipped, 2);
    let again = notes::read_note(vault, &note_id).unwrap();
    assert_eq!(again.matches("🎙 转写").count(), 2, "转写块不应堆叠");
}
