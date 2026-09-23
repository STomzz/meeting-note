/**
 * 场次播放器：把一场录音的多个分段当**一条长语音**播（自定义元素）。
 *
 * - 笔记里只有一行引用（`/v 会议音频/<笔记>/<场次>/`），编辑器把它渲染成本元素；
 * - 底层仍是分段 WAV 顺序播放，不转码、不重复占空间；
 * - 进度条是**整场时间轴**：能直接拖到任意位置（内部换算成「第几段 + 段内偏移」）；
 * - 用自定义元素是因为块编辑器把每块渲染成 HTML 字符串（`v-html`），
 *   插进来的元素会被浏览器升级（custom element 的升级机制），Vue 组件做不到这点。
 */
import { formatDur } from './meetingNote'
import {
  fromPermille,
  globalPosition,
  locateGlobal,
  progressPermille,
  totalDuration,
} from './playlist'

export const SESSION_PLAYER_TAG = 'bnu-session-player'

interface PlayerSegment {
  src: string
  name?: string
  durationMs?: number
}

interface PlayerPayload {
  segments?: PlayerSegment[]
}

const GLYPH_PLAY = '▶'
const GLYPH_PAUSE = '❚❚'

class SessionPlayerElement extends HTMLElement {
  private segs: PlayerSegment[] = []
  private durs: number[] = []
  private index = 0
  private offsetMs = 0
  private playing = false
  private dragging = false
  private error = ''
  private audio: HTMLAudioElement | null = null
  private bar: HTMLInputElement | null = null
  private playBtn: HTMLButtonElement | null = null
  private timeEl: HTMLElement | null = null
  private segEl: HTMLElement | null = null
  private nameEl: HTMLElement | null = null
  private errEl: HTMLElement | null = null
  private built = false

  connectedCallback() {
    this.setAttribute('data-no-edit', '')
    if (!this.built) {
      try {
        const payload = JSON.parse(this.getAttribute('data-playlist') ?? '{}') as PlayerPayload
        this.segs = (payload.segments ?? []).filter((s) => !!s.src)
      } catch {
        this.segs = []
      }
      this.durs = this.segs.map((s) => Math.max(0, s.durationMs ?? 0))
      this.build()
      this.built = true
    }
    this.render()
  }

  disconnectedCallback() {
    this.audio?.pause()
    this.playing = false
  }

  // ---------------------------------------------------------------- DOM

  private build() {
    const row = document.createElement('span')
    row.className = 'sp-row'

    const play = document.createElement('button')
    play.type = 'button'
    play.className = 'sp-btn'
    play.title = '播放 / 暂停（整场连播）'
    play.setAttribute('aria-label', '播放')
    play.textContent = GLYPH_PLAY
    play.addEventListener('click', () => void this.toggle())

    const bar = document.createElement('input')
    bar.type = 'range'
    bar.className = 'sp-bar'
    bar.min = '0'
    bar.max = '1000'
    bar.step = '1'
    bar.value = '0'
    bar.setAttribute('aria-label', '整场进度')
    bar.addEventListener('pointerdown', () => {
      this.dragging = true
    })
    bar.addEventListener('input', () => {
      const ms = fromPermille(Number(bar.value), this.durs)
      this.paintTime(ms)
    })
    bar.addEventListener('change', () => {
      this.dragging = false
      void this.seekMs(fromPermille(Number(bar.value), this.durs))
    })

    const time = document.createElement('span')
    time.className = 'sp-time'

    row.append(play, bar, time)

    const sub = document.createElement('span')
    sub.className = 'sp-sub'
    const name = document.createElement('span')
    name.className = 'sp-name'
    name.textContent = this.getAttribute('data-label') ?? ''
    const seg = document.createElement('span')
    seg.className = 'sp-seg'
    sub.append(name, seg)

    const err = document.createElement('span')
    err.className = 'sp-err'

    this.append(row, sub, err)
    this.playBtn = play
    this.bar = bar
    this.timeEl = time
    this.segEl = seg
    this.nameEl = name
    this.errEl = err
  }

  private audioEl(): HTMLAudioElement {
    if (!this.audio) {
      const a = document.createElement('audio')
      a.preload = 'metadata'
      a.addEventListener('timeupdate', () => {
        if (!this.dragging && this.audio) this.offsetMs = this.audio.currentTime * 1000
        this.paint()
      })
      a.addEventListener('ended', () => void this.next())
      a.addEventListener('loadedmetadata', () => {
        const d = a.duration
        if (Number.isFinite(d) && d > 0) {
          this.durs[this.index] = Math.round(d * 1000)
        }
        this.paint()
      })
      a.addEventListener('error', () => this.onError())
      a.hidden = true
      this.append(a)
      this.audio = a
    }
    return this.audio
  }

  // ---------------------------------------------------------------- 播放

  private async toggle() {
    if (this.playing) {
      this.audio?.pause()
      this.playing = false
      this.paint()
      return
    }
    const a = this.audioEl()
    if (!a.getAttribute('src')) {
      await this.load(0, true)
      return
    }
    try {
      await a.play()
      this.playing = true
      this.error = ''
    } catch {
      this.error = '播放被系统拦住了，再点一次试试'
    }
    this.paint()
  }

  private async load(index: number, autoplay: boolean) {
    if (!this.segs.length) return
    const i = Math.min(Math.max(0, index), this.segs.length - 1)
    const seg = this.segs[i]
    const a = this.audioEl()
    if (this.index !== i || a.getAttribute('src') !== seg.src) {
      this.index = i
      if (a.getAttribute('src') !== seg.src) {
        a.src = seg.src
        a.load()
        await this.waitMeta(a)
      }
    }
    this.offsetMs = 0
    if (autoplay) {
      try {
        await a.play()
        this.playing = true
        this.error = ''
      } catch {
        this.playing = false
        this.error = '播放被系统拦住了，再点一次试试'
      }
    }
    this.paint()
  }

  private waitMeta(a: HTMLAudioElement): Promise<void> {
    if (a.readyState >= 1) return Promise.resolve()
    return new Promise((resolve) => {
      const done = () => {
        a.removeEventListener('loadedmetadata', done)
        a.removeEventListener('error', done)
        resolve()
      }
      a.addEventListener('loadedmetadata', done)
      a.addEventListener('error', done)
      window.setTimeout(done, 4000)
    })
  }

  private async next() {
    if (this.index + 1 < this.segs.length) {
      await this.load(this.index + 1, true)
      return
    }
    this.playing = false
    this.offsetMs = totalDuration(this.durs)
    this.paint()
  }

  private async seekMs(ms: number) {
    const target = locateGlobal(ms, this.durs)
    const wasPlaying = this.playing
    if (target.index !== this.index) await this.load(target.index, false)
    const a = this.audioEl()
    const durMs = this.durs[target.index] || 0
    const seconds = target.offsetMs / 1000
    a.currentTime = durMs > 0 ? Math.min(seconds, durMs / 1000) : seconds
    this.offsetMs = target.offsetMs
    if (wasPlaying) {
      try {
        await a.play()
        this.playing = true
      } catch {
        this.playing = false
      }
    }
    this.paint()
  }

  private onError() {
    this.error = '有一段音频读不出来，已跳到下一段'
    if (this.playing || this.index + 1 < this.segs.length) void this.next()
    this.paint()
  }

  // ---------------------------------------------------------------- 绘制

  private render() {
    this.paint()
  }

  private paint() {
    const total = totalDuration(this.durs)
    const pos = globalPosition(this.index, this.offsetMs, this.durs)
    if (this.bar && !this.dragging) this.bar.value = String(progressPermille(pos, this.durs))
    if (this.timeEl) this.timeEl.textContent = `${formatDur(pos)} / ${formatDur(total)}`
    if (this.playBtn) {
      const glyph = this.playing ? GLYPH_PAUSE : GLYPH_PLAY
      if (this.playBtn.textContent !== glyph) this.playBtn.textContent = glyph
      this.playBtn.setAttribute('aria-label', this.playing ? '暂停' : '播放')
    }
    if (this.segEl) {
      this.segEl.textContent = this.segs.length
        ? `第 ${this.index + 1}/${this.segs.length} 段`
        : '没有音频'
    }
    if (this.nameEl && !this.nameEl.textContent) {
      this.nameEl.textContent = this.getAttribute('data-label') ?? ''
    }
    if (this.errEl) this.errEl.textContent = this.error
  }

  /** 拖动时只刷时间文字，别跟播放进度打架。 */
  private paintTime(ms: number) {
    if (!this.timeEl) return
    this.timeEl.textContent = `${formatDur(ms)} / ${formatDur(totalDuration(this.durs))}`
  }
}

let defined = false

/** 注册自定义元素（只需一次；块编辑器渲染出来的标签会被浏览器自动升级）。 */
export function defineSessionPlayer(): void {
  if (defined || typeof window === 'undefined' || !window.customElements) return
  if (!window.customElements.get(SESSION_PLAYER_TAG)) {
    window.customElements.define(SESSION_PLAYER_TAG, SessionPlayerElement)
  }
  defined = true
}
