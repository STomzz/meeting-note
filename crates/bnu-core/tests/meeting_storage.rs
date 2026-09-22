//! 集成测试：录音分段文件的生命周期（对应 Tauri 命令
//! `meeting_start_segment` / `meeting_append_pcm` / `meeting_close_segment`）。
//!
//! 验证点：占位 WAV 头 → 追加 PCM → 回写长度，全程都是合法 WAV，
//! 且能被转写链路（`normalize_to_16k_mono` + `split_speech`）直接读取。

use bnu_core::{audio, db, meetings};

#[test]
fn segment_file_lifecycle_is_valid_wav() {
    let dir = tempfile::tempdir().unwrap();
    let conn = db::open_memory().unwrap();
    let meeting = meetings::create(&conn, "录音分段生命周期").unwrap();

    // 设备采样率通常不是 16k（这里用 48k 模拟），录音阶段按原采样率保存
    let rate = 48_000u32;
    meetings::start_segment(&conn, dir.path(), &meeting.id, 1, rate).unwrap();
    let path = dir
        .path()
        .join("meetings")
        .join(&meeting.id)
        .join("seg_0001.wav");
    assert_eq!(std::fs::metadata(&path).unwrap().len(), 44, "开始时只有占位头");

    // 1 秒 48k 单声道 PCM16
    let pcm = vec![0x11u8; rate as usize * 2];
    let stat = meetings::append_pcm(&conn, dir.path(), &meeting.id, 1, rate, &pcm).unwrap();
    assert_eq!(stat.bytes, pcm.len() as i64);
    assert_eq!(stat.duration_ms, 1000);

    let seg = meetings::close_segment(&conn, dir.path(), &meeting.id, 1).unwrap();
    assert_eq!(seg.status, meetings::SEG_RECORDED);
    assert_eq!(seg.src_rate, rate as i64);

    // 文件仍是合法 WAV，且时长可解析
    let wav = meetings::read_segment(dir.path(), &meeting.id, 1).unwrap();
    let info = audio::parse_wav(&wav).expect("合法 WAV");
    assert_eq!((info.sample_rate, info.channels, info.bits_per_sample), (rate, 1, 16));
    assert_eq!(audio::duration_ms(&wav), Some(1000));

    // 转写链路：重采样到 16k 单声道后可直接做静音切段
    let normalized = audio::normalize_to_16k_mono(&wav).unwrap();
    assert_eq!(normalized.len(), 16_000 * 2);
    let chunks = audio::split_speech_pcm(&normalized);
    assert_eq!(chunks.len(), 1, "1 秒语音应作为收尾段保留");
    assert!(chunks[0].duration_ms >= 900);

    // 会议详情里的时长来自分段汇总
    let detail = meetings::detail(&conn, dir.path(), &meeting.id).unwrap();
    assert_eq!(detail.meeting.duration_ms, 1000);
    assert_eq!(detail.segments.len(), 1);
}
