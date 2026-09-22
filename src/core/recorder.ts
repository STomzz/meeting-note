/**
 * 麦克风采集：直接用 Web Audio 拿 PCM，不依赖 MediaRecorder 的编码器。
 *
 * 原因：上游 ASR 只吃 WAV，而 WebView 里 MediaRecorder 输出的 webm/opus 在
 * WebKitGTK 上支持不稳定；直接用 ScriptProcessorNode 拿 Float32 帧再转 PCM16，
 * 跨平台一致，也方便边录边把数据写进 Rust 侧的 WAV 文件。
 *
 * 说明：ScriptProcessorNode 已标记废弃，但在 WebKitGTK 与 Android WebView 上都可用；
 * 采样率尽量请求 16k，浏览器不支持时按设备采样率（常见 48k）采集，
 * 送 ASR 前由 Rust 侧重采样到 16k。
 */

export interface RecorderCallbacks {
  /** 每次拿到一小段 PCM16（约 0.1~0.2 秒） */
  onChunk: (pcm: Int16Array) => void
  /** 音量（RMS，0~32767） */
  onLevel: (rms: number) => void
  onError: (message: string) => void
}

export interface RecorderInfo {
  sampleRate: number
  channelCount: number
  /** 实际采集精度（16 表示 PCM16） */
  bitsPerSample: number
}

function floatToInt16(input: Float32Array): Int16Array {
  const out = new Int16Array(input.length)
  for (let i = 0; i < input.length; i++) {
    const v = Math.max(-1, Math.min(1, input[i]))
    out[i] = v < 0 ? v * 0x8000 : v * 0x7fff
  }
  return out
}

function rmsOf(pcm: Int16Array): number {
  if (!pcm.length) return 0
  let sum = 0
  for (let i = 0; i < pcm.length; i++) sum += pcm[i] * pcm[i]
  return Math.sqrt(sum / pcm.length)
}

export function pcmToBase64(pcm: Int16Array): string {
  const bytes = new Uint8Array(pcm.buffer, pcm.byteOffset, pcm.byteLength)
  let binary = ''
  const CHUNK = 0x8000
  for (let i = 0; i < bytes.length; i += CHUNK) {
    binary += String.fromCharCode.apply(null, Array.from(bytes.subarray(i, i + CHUNK)))
  }
  return btoa(binary)
}

/** 把 PCM16 打包成 WAV，返回可直接塞进 <audio> 的 Blob URL（自检面板回放用）。 */
export function pcmToWavUrl(pcm: Int16Array, sampleRate: number): string {
  const dataBytes = pcm.byteLength
  const buffer = new ArrayBuffer(44 + dataBytes)
  const view = new DataView(buffer)
  const writeAscii = (offset: number, text: string) => {
    for (let i = 0; i < text.length; i++) view.setUint8(offset + i, text.charCodeAt(i))
  }
  writeAscii(0, 'RIFF')
  view.setUint32(4, 36 + dataBytes, true)
  writeAscii(8, 'WAVE')
  writeAscii(12, 'fmt ')
  view.setUint32(16, 16, true)
  view.setUint16(20, 1, true)
  view.setUint16(22, 1, true)
  view.setUint32(24, sampleRate, true)
  view.setUint32(28, sampleRate * 2, true)
  view.setUint16(32, 2, true)
  view.setUint16(34, 16, true)
  writeAscii(36, 'data')
  view.setUint32(40, dataBytes, true)
  new Uint8Array(buffer, 44).set(new Uint8Array(pcm.buffer, pcm.byteOffset, dataBytes))
  return URL.createObjectURL(new Blob([buffer], { type: 'audio/wav' }))
}

export class MicRecorder {
  private ctx: AudioContext | null = null
  private stream: MediaStream | null = null
  private source: MediaStreamAudioSourceNode | null = null
  private node: ScriptProcessorNode | null = null
  private sink: GainNode | null = null
  private wakeLock: WakeLockSentinel | null = null

  info: RecorderInfo = { sampleRate: 0, channelCount: 1, bitsPerSample: 16 }
  recording = false

  /** 环境自检：给 Android spike 用，报告权限与采集参数。 */
  static async selfCheck(): Promise<string[]> {
    const notes: string[] = []
    if (!navigator.mediaDevices?.getUserMedia) {
      notes.push('❌ 当前 WebView 不支持 getUserMedia（无法录音）')
      return notes
    }
    notes.push('✅ 支持 getUserMedia')
    notes.push(`AudioContext 采样率支持自定义：${'AudioContext' in window ? '是' : '否'}`)
    if ('wakeLock' in navigator) {
      notes.push('✅ 支持屏幕常亮 Wake Lock')
    } else {
      notes.push('⚠️ 不支持 Wake Lock（Android 上可能自动息屏）')
    }
    if (navigator.userAgent.includes('Android')) {
      notes.push(`Android WebView：${navigator.userAgent}`)
    }
    return notes
  }

  async start(cb: RecorderCallbacks): Promise<RecorderInfo> {
    if (this.recording) return this.info
    if (!navigator.mediaDevices?.getUserMedia) {
      throw new Error('当前环境不支持录音（WebView 缺少 getUserMedia）')
    }
    this.stream = await navigator.mediaDevices.getUserMedia({
      audio: {
        channelCount: 1,
        echoCancellation: true,
        noiseSuppression: true,
        autoGainControl: true,
      },
    })

    // 尽量用 16k；不支持时浏览器会回落到设备默认采样率
    try {
      this.ctx = new AudioContext({ sampleRate: 16000 })
    } catch {
      this.ctx = new AudioContext()
    }
    if (this.ctx.state === 'suspended') await this.ctx.resume()

    const track = this.stream.getAudioTracks()[0]
    const settings = track?.getSettings?.() ?? {}
    this.info = {
      sampleRate: this.ctx.sampleRate,
      channelCount: settings.channelCount ?? 1,
      bitsPerSample: 16,
    }

    this.source = this.ctx.createMediaStreamSource(this.stream)
    const node = this.ctx.createScriptProcessor(4096, 1, 1)
    node.onaudioprocess = (event) => {
      if (!this.recording) return
      const input = event.inputBuffer.getChannelData(0)
      const pcm = floatToInt16(input)
      cb.onLevel(rmsOf(pcm))
      cb.onChunk(pcm)
    }
    // 静音输出（GainNode 增益 0），避免回授；WebKit 需要连到 destination 才会持续回调
    this.sink = this.ctx.createGain()
    this.sink.gain.value = 0
    this.source.connect(node)
    node.connect(this.sink)
    this.sink.connect(this.ctx.destination)
    this.node = node

    this.recording = true
    await this.requestWakeLock()
    return this.info
  }

  async stop(): Promise<void> {
    this.recording = false
    try {
      this.node?.disconnect()
      this.source?.disconnect()
      this.sink?.disconnect()
    } catch {
      /* 忽略断开异常 */
    }
    this.node = null
    this.source = null
    this.sink = null
    this.stream?.getTracks().forEach((t) => t.stop())
    this.stream = null
    try {
      await this.ctx?.close()
    } catch {
      /* 忽略关闭异常 */
    }
    this.ctx = null
    await this.releaseWakeLock()
  }

  /** Android：录音期间保持屏幕常亮。 */
  private async requestWakeLock() {
    try {
      const nav = navigator as Navigator & { wakeLock?: { request: (t: string) => Promise<WakeLockSentinel> } }
      if (nav.wakeLock) this.wakeLock = await nav.wakeLock.request('screen')
    } catch {
      /* 不支持就算了，不阻断录音 */
    }
  }

  private async releaseWakeLock() {
    try {
      await this.wakeLock?.release()
    } catch {
      /* 忽略 */
    }
    this.wakeLock = null
  }
}
