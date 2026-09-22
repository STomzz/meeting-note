//! 音频工具：WAV 编解码、重采样、RMS 与静音切段。
//!
//! 参数移植自既有会议 App 的 Dart 实现（`segment_recorder.dart` / `wav.dart`）：
//! 16kHz 单声道 16bit、100ms 帧、自适应噪声地板、静音 600ms 且段长 ≥8s 才切、单段最长 45s。
//!
//! 行号/时间基准：`start_ms` 都是相对该 WAV 起点的时间。

use anyhow::{bail, Result};

/// 目标采样率（与上游 ASR 对齐）。
pub const SAMPLE_RATE: u32 = 16_000;
/// 分析帧长（毫秒）。
pub const FRAME_MS: usize = 100;
/// 静音多少毫秒后允许切段。
pub const SILENCE_CUT_MS: usize = 600;
/// 单段最小时长（毫秒），低于此长度不切。
pub const MIN_SEGMENT_MS: usize = 8_000;
/// 单段最长时长（毫秒），超过强制切。
pub const MAX_SEGMENT_MS: usize = 45_000;
/// 收尾时低于该长度的残余段丢弃。
pub const MIN_FLUSH_MS: usize = 1_000;
/// 噪声地板初值。
pub const NOISE_FLOOR_INIT: f32 = 150.0;
/// 判定为静音的 RMS 绝对下限。
pub const MIN_RMS_THRESHOLD: f32 = 220.0;
/// 噪声地板的倍数（超过视为说话）。
pub const NOISE_FLOOR_RATIO: f32 = 2.5;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WavInfo {
    pub sample_rate: u32,
    pub channels: u16,
    pub bits_per_sample: u16,
    pub byte_rate: u32,
    pub data_offset: usize,
    pub data_length: usize,
}

/// 解析 WAV 头（支持 fmt/data 分块，容忍附加块）。
pub fn parse_wav(bytes: &[u8]) -> Option<WavInfo> {
    if bytes.len() < 44 || &bytes[0..4] != b"RIFF" || &bytes[8..12] != b"WAVE" {
        return None;
    }
    let mut offset = 12usize;
    let mut sample_rate = None;
    let mut channels = None;
    let mut bits = None;
    let mut byte_rate = None;
    let mut data_offset = None;
    let mut data_length = None;

    while offset + 8 <= bytes.len() {
        let chunk_id = &bytes[offset..offset + 4];
        let chunk_size = u32::from_le_bytes([
            bytes[offset + 4],
            bytes[offset + 5],
            bytes[offset + 6],
            bytes[offset + 7],
        ]) as usize;
        let body = offset + 8;

        if chunk_id == b"fmt " && body + 16 <= bytes.len() {
            channels = Some(u16::from_le_bytes([bytes[body + 2], bytes[body + 3]]));
            sample_rate = Some(u32::from_le_bytes([
                bytes[body + 4],
                bytes[body + 5],
                bytes[body + 6],
                bytes[body + 7],
            ]));
            byte_rate = Some(u32::from_le_bytes([
                bytes[body + 8],
                bytes[body + 9],
                bytes[body + 10],
                bytes[body + 11],
            ]));
            bits = Some(u16::from_le_bytes([bytes[body + 14], bytes[body + 15]]));
        } else if chunk_id == b"data" {
            data_offset = Some(body);
            data_length = Some(chunk_size.min(bytes.len().saturating_sub(body)));
            break;
        }

        offset = body + chunk_size + (chunk_size % 2);
    }

    Some(WavInfo {
        sample_rate: sample_rate?,
        channels: channels?,
        bits_per_sample: bits?,
        byte_rate: byte_rate?,
        data_offset: data_offset?,
        data_length: data_length?,
    })
}

/// 生成标准 WAV 头 + PCM 数据。
pub fn build_wav(pcm: &[u8], sample_rate: u32, channels: u16, bits_per_sample: u16) -> Vec<u8> {
    let byte_rate = sample_rate * channels as u32 * bits_per_sample as u32 / 8;
    let block_align = channels * bits_per_sample / 8;
    let mut out = Vec::with_capacity(44 + pcm.len());
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&((36 + pcm.len()) as u32).to_le_bytes());
    out.extend_from_slice(b"WAVEfmt ");
    out.extend_from_slice(&16u32.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes()); // PCM
    out.extend_from_slice(&channels.to_le_bytes());
    out.extend_from_slice(&sample_rate.to_le_bytes());
    out.extend_from_slice(&byte_rate.to_le_bytes());
    out.extend_from_slice(&block_align.to_le_bytes());
    out.extend_from_slice(&bits_per_sample.to_le_bytes());
    out.extend_from_slice(b"data");
    out.extend_from_slice(&(pcm.len() as u32).to_le_bytes());
    out.extend_from_slice(pcm);
    out
}

/// 时长（毫秒）。
pub fn duration_ms(bytes: &[u8]) -> Option<u64> {
    let info = parse_wav(bytes)?;
    if info.byte_rate == 0 {
        return None;
    }
    Some((info.data_length as u64 * 1000) / info.byte_rate as u64)
}

/// 一段音频的采样数（按声道计）。
pub fn sample_count(pcm_len: usize, channels: u16, bits: u16) -> usize {
    let frame_bytes = channels as usize * (bits as usize / 8).max(1);
    if frame_bytes == 0 {
        0
    } else {
        pcm_len / frame_bytes
    }
}

/// 16bit 小端 PCM → i16 采样。
pub fn bytes_to_i16(pcm: &[u8]) -> Vec<i16> {
    pcm.chunks_exact(2)
        .map(|c| i16::from_le_bytes([c[0], c[1]]))
        .collect()
}

/// i16 采样 → 16bit 小端 PCM。
pub fn i16_to_bytes(samples: &[i16]) -> Vec<u8> {
    let mut out = Vec::with_capacity(samples.len() * 2);
    for s in samples {
        out.extend_from_slice(&s.to_le_bytes());
    }
    out
}

/// 帧的 RMS（与既有 Dart 实现一致：直接对 i16 求均方根）。
pub fn rms_i16(pcm: &[u8]) -> f32 {
    let samples = bytes_to_i16(pcm);
    if samples.is_empty() {
        return 0.0;
    }
    let sum: f64 = samples.iter().map(|s| (*s as f64) * (*s as f64)).sum();
    (sum / samples.len() as f64).sqrt() as f32
}

/// 多声道 → 单声道（16bit）。
pub fn to_mono(samples: &[i16], channels: u16) -> Vec<i16> {
    let ch = channels.max(1) as usize;
    if ch == 1 {
        return samples.to_vec();
    }
    samples
        .chunks_exact(ch)
        .map(|frame| {
            let sum: i32 = frame.iter().map(|s| *s as i32).sum();
            (sum / ch as i32).clamp(-32768, 32767) as i16
        })
        .collect()
}

/// 线性插值重采样（语音够用；上游也只做识别）。
pub fn resample_linear(samples: &[i16], from: u32, to: u32) -> Vec<i16> {
    if from == to || samples.is_empty() || from == 0 || to == 0 {
        return samples.to_vec();
    }
    let ratio = to as f64 / from as f64;
    let out_len = ((samples.len() as f64) * ratio).round() as usize;
    let mut out = Vec::with_capacity(out_len);
    for i in 0..out_len {
        let src = i as f64 / ratio;
        let idx = src.floor() as usize;
        let frac = src - idx as f64;
        let a = samples.get(idx).copied().unwrap_or(0) as f64;
        let b = samples.get(idx + 1).copied().unwrap_or(a as i16) as f64;
        out.push((a + (b - a) * frac).round().clamp(-32768.0, 32767.0) as i16);
    }
    out
}

/// 任意 WAV → 16kHz 单声道 PCM16 字节（供 ASR 使用）。
pub fn normalize_to_16k_mono(bytes: &[u8]) -> Result<Vec<u8>> {
    let info = parse_wav(bytes).ok_or_else(|| anyhow::anyhow!("不是合法的 WAV（缺少 RIFF/WAVE 头）"))?;
    if info.bits_per_sample != 16 {
        bail!("仅支持 16bit PCM WAV，当前 {}bit", info.bits_per_sample);
    }
    let pcm = &bytes[info.data_offset..info.data_offset + info.data_length];
    let mono = to_mono(&bytes_to_i16(pcm), info.channels);
    let resampled = resample_linear(&mono, info.sample_rate, SAMPLE_RATE);
    Ok(i16_to_bytes(&resampled))
}

/// 切出来的一段语音（已带 WAV 头）。
#[derive(Debug, Clone)]
pub struct SpeechChunk {
    pub wav: Vec<u8>,
    pub start_ms: u64,
    pub duration_ms: u64,
}

fn make_chunk(samples: &[i16], start_sample: usize, end_sample: usize) -> Option<SpeechChunk> {
    if end_sample <= start_sample {
        return None;
    }
    let pcm = &samples[start_sample..end_sample];
    Some(SpeechChunk {
        wav: build_wav(&i16_to_bytes(pcm), SAMPLE_RATE, 1, 16),
        start_ms: (start_sample as u64 * 1000) / SAMPLE_RATE as u64,
        duration_ms: (pcm.len() as u64 * 1000) / SAMPLE_RATE as u64,
    })
}

/// 按静音切段（输入为 16kHz 单声道 PCM16 裸数据）。
///
/// 规则：静音 ≥600ms 且段长 ≥8s 时切，段长 ≥45s 强制切；收尾 ≥1s 也保留。
pub fn split_speech_pcm(pcm: &[u8]) -> Vec<SpeechChunk> {
    let samples = bytes_to_i16(pcm);
    let frame_len = (SAMPLE_RATE as usize * FRAME_MS) / 1000;

    let mut chunks = Vec::new();
    let mut noise_floor = NOISE_FLOOR_INIT;
    let mut silence_ms = 0usize;
    let mut seg_start = 0usize;
    let mut pos = 0usize;

    while pos + frame_len <= samples.len() {
        let frame = &samples[pos..pos + frame_len];
        let rms = {
            let sum: f64 = frame.iter().map(|s| (*s as f64) * (*s as f64)).sum();
            (sum / frame.len() as f64).sqrt() as f32
        };

        // 自适应噪声地板：安静时缓慢跟随
        if rms < noise_floor * 2.0 {
            noise_floor = noise_floor * 0.95 + rms * 0.05;
        }
        let threshold = MIN_RMS_THRESHOLD.max(noise_floor * NOISE_FLOOR_RATIO);
        if rms < threshold {
            silence_ms += FRAME_MS;
        } else {
            silence_ms = 0;
        }

        pos += frame_len;
        let seg_ms = (pos - seg_start) * 1000 / SAMPLE_RATE as usize;
        let cut_by_silence = silence_ms >= SILENCE_CUT_MS && seg_ms >= MIN_SEGMENT_MS;
        let cut_by_length = seg_ms >= MAX_SEGMENT_MS;
        if cut_by_silence || cut_by_length {
            if let Some(c) = make_chunk(&samples, seg_start, pos) {
                chunks.push(c);
            }
            seg_start = pos;
            silence_ms = 0;
        }
    }

    // 收尾：剩余 ≥1s 保留（<1s 丢弃）
    let tail_ms = (samples.len() - seg_start) * 1000 / SAMPLE_RATE as usize;
    if tail_ms >= MIN_FLUSH_MS {
        if let Some(c) = make_chunk(&samples, seg_start, samples.len()) {
            chunks.push(c);
        }
    }
    chunks
}

/// 按静音切段（输入为 WAV；要求 16kHz 单声道 16bit）。
pub fn split_speech(wav: &[u8]) -> Result<Vec<SpeechChunk>> {
    let info = parse_wav(wav).ok_or_else(|| anyhow::anyhow!("不是合法的 WAV"))?;
    if info.channels != 1 || info.bits_per_sample != 16 || info.sample_rate != SAMPLE_RATE {
        bail!(
            "静音切段要求 16kHz 单声道 16bit WAV（当前 {}Hz/{}ch/{}bit）",
            info.sample_rate,
            info.channels,
            info.bits_per_sample
        );
    }
    let pcm = &wav[info.data_offset..info.data_offset + info.data_length];
    Ok(split_speech_pcm(pcm))
}

/// 按固定时长切分（导入长音频时的兜底，不做静音分析）。
pub fn split_by_duration(wav: &[u8], max_ms: u64) -> Result<Vec<SpeechChunk>> {
    let info = parse_wav(wav).ok_or_else(|| anyhow::anyhow!("不是合法的 WAV"))?;
    if info.bits_per_sample != 16 {
        bail!("仅支持 16bit PCM WAV");
    }
    let pcm = &wav[info.data_offset..info.data_offset + info.data_length];
    let all = bytes_to_i16(pcm);
    let per_ms = (info.sample_rate as u64 * (info.channels as u64)) / 1000; // 每毫秒的 i16 采样数
    let step = (per_ms * max_ms).max(1) as usize;
    let mut chunks = Vec::new();
    let mut start = 0usize;
    while start < all.len() {
        let end = (start + step).min(all.len());
        let mono = to_mono(&all[start..end], info.channels);
        let resampled = resample_linear(&mono, info.sample_rate, SAMPLE_RATE);
        chunks.push(SpeechChunk {
            wav: build_wav(&i16_to_bytes(&resampled), SAMPLE_RATE, 1, 16),
            start_ms: start as u64 / per_ms.max(1),
            duration_ms: (end - start) as u64 / per_ms.max(1),
        });
        start = end;
    }
    Ok(chunks)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tone(ms: usize, amp: i16) -> Vec<i16> {
        let n = (SAMPLE_RATE as usize * ms) / 1000;
        (0..n)
            .map(|i| {
                let t = i as f32 / SAMPLE_RATE as f32;
                (amp as f32 * (2.0 * std::f32::consts::PI * 440.0 * t).sin()) as i16
            })
            .collect()
    }

    fn silence(ms: usize) -> Vec<i16> {
        vec![0i16; (SAMPLE_RATE as usize * ms) / 1000]
    }

    fn wav_of(samples: &[i16]) -> Vec<u8> {
        build_wav(&i16_to_bytes(samples), SAMPLE_RATE, 1, 16)
    }

    #[test]
    fn wav_roundtrip_and_duration() {
        let pcm = i16_to_bytes(&tone(1000, 8000));
        let wav = build_wav(&pcm, SAMPLE_RATE, 1, 16);
        let info = parse_wav(&wav).unwrap();
        assert_eq!((info.sample_rate, info.channels, info.bits_per_sample), (16000, 1, 16));
        assert_eq!(duration_ms(&wav), Some(1000));
        assert_eq!(info.data_length, pcm.len());
    }

    #[test]
    fn resample_halves_and_triples() {
        let s = tone(1000, 8000);
        assert_eq!(resample_linear(&s, 48000, 16000).len(), s.len() / 3);
        assert_eq!(resample_linear(&s, 16000, 48000).len(), s.len() * 3);
        assert_eq!(resample_linear(&s, 16000, 16000).len(), s.len());
    }

    #[test]
    fn rms_orders_silence_below_speech() {
        let quiet = i16_to_bytes(&silence(100));
        let loud = i16_to_bytes(&tone(100, 12000));
        assert!(rms_i16(&quiet) < 1.0);
        assert!(rms_i16(&loud) > 1000.0);
    }

    #[test]
    fn mono_mixdown_averages_channels() {
        let stereo = vec![1000i16, 3000, -1000, 1000];
        assert_eq!(to_mono(&stereo, 2), vec![2000, 0]);
    }

    #[test]
    fn split_by_silence_keeps_segments_between_pauses() {
        // 10s 说话 + 1.5s 静音 + 10s 说话 + 1.5s 静音 + 5s 说话
        let mut all = Vec::new();
        all.extend(tone(10_000, 9000));
        all.extend(silence(1_500));
        all.extend(tone(10_000, 9000));
        all.extend(silence(1_500));
        all.extend(tone(5_000, 9000));
        let wav = wav_of(&all);

        let chunks = split_speech(&wav).unwrap();
        assert_eq!(chunks.len(), 3, "应在两处静音后切段，收尾 5s 也保留");
        // 第一段 ≈ 10s 说话 + 0.6s 静音（静音累计到 600ms 才切）
        assert!((10_400..=11_600).contains(&chunks[0].duration_ms), "{:?}", chunks[0]);
        assert!((10_000..=11_600).contains(&chunks[1].start_ms), "{:?}", chunks[1]);
        assert!(chunks[2].start_ms >= 22_000);
        // 每段都是合法 WAV 且 16k 单声道
        for c in &chunks {
            let info = parse_wav(&c.wav).unwrap();
            assert_eq!((info.sample_rate, info.channels), (SAMPLE_RATE, 1));
        }
    }

    #[test]
    fn split_cuts_long_monologue_at_45s() {
        let wav = wav_of(&tone(60_000, 9000));
        let chunks = split_speech(&wav).unwrap();
        assert_eq!(chunks.len(), 2, "60s 连续说话应按 45s 强制切 + 15s 收尾");
        assert!((44_000..=46_000).contains(&chunks[0].duration_ms));
        assert!((14_000..=16_000).contains(&chunks[1].duration_ms));
    }

    #[test]
    fn split_drops_tiny_tail() {
        let mut all = tone(10_000, 9000);
        all.extend(silence(1_000));
        all.extend(tone(300, 9000)); // 300ms 尾巴，切段后剩余 0.7s，应丢弃
        let chunks = split_speech(&wav_of(&all)).unwrap();
        assert_eq!(chunks.len(), 1, "0.7s 残余不足 1s，应丢弃");
        assert!((10_400..=10_800).contains(&chunks[0].duration_ms), "{:?}", chunks[0]);
    }

    #[test]
    fn normalize_48k_stereo_to_16k_mono() {
        let left = tone(1000, 8000);
        let mut stereo = Vec::with_capacity(left.len() * 2);
        for s in &left {
            stereo.push(*s);
            stereo.push(*s);
        }
        let wav48 = build_wav(&i16_to_bytes(&stereo), 48_000, 2, 16);
        let out = normalize_to_16k_mono(&wav48).unwrap();
        // 数据量是 16k 的 1 秒，但声明成 48k → 实际 1/3 秒，重采样回 16k 后仍约 1/3 秒
        assert_eq!(out.len(), (SAMPLE_RATE as usize * 2) / 3);
        assert!(rms_i16(&out) > 1000.0);
    }

    #[test]
    fn split_by_duration_chunks_evenly() {
        let wav = wav_of(&tone(10_000, 8000));
        let chunks = split_by_duration(&wav, 4_000).unwrap();
        assert_eq!(chunks.len(), 3);
        assert!((3_900..=4_100).contains(&chunks[0].duration_ms));
    }
}
