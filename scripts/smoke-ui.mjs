/**
 * 真浏览器 UI 冒烟（笔记块编辑 + `/v` 选择器 + 录音面板 + 图谱 2D/3D 切换）。
 *
 * 用途：改完前端后，在**真 Chrome** 里把关键交互点一遍（jsdom 测不出的那种：
 * 焦点/blur、v-for 里的模板 ref、canvas 尺寸、G6/three 的生命周期）。
 *
 * 用法：
 *   npm run dev                     # 另开一个终端，起 1420
 *   node scripts/smoke-ui.mjs       # 默认打 http://127.0.0.1:1420
 *
 * 环境变量：
 *   BASE            默认 http://127.0.0.1:1420
 *   CHROME          Chrome/Chromium 可执行文件路径（默认找 ~/.cache/puppeteer/chrome/*\/chrome-linux64/chrome）
 *   PUPPETEER_CORE  本机没装 puppeteer-core 时，指向它的安装目录（如某个项目的 node_modules）
 *   SHOTS           截图目录，默认 /tmp/opencode/shots
 *
 * 失败时进程退出码为 1（可直接接进 CI）。
 */
import { createRequire } from 'node:module'
import { existsSync, mkdirSync, readdirSync } from 'node:fs'
import { homedir } from 'node:os'
import { join } from 'node:path'

const BASE = process.env.BASE ?? 'http://127.0.0.1:1420'
const SHOTS = process.env.SHOTS ?? '/tmp/opencode/shots'

function findChrome() {
  if (process.env.CHROME) return process.env.CHROME
  const root = join(homedir(), '.cache/puppeteer/chrome')
  if (!existsSync(root)) return null
  for (const dir of readdirSync(root).sort().reverse()) {
    for (const name of ['chrome-linux64/chrome', 'chrome-linux/chrome', 'chrome']) {
      const p = join(root, dir, name)
      if (existsSync(p)) return p
    }
  }
  return null
}

async function loadPuppeteer() {
  if (process.env.PUPPETEER_CORE) {
    const require = createRequire(join(process.env.PUPPETEER_CORE, 'index.js'))
    return require('puppeteer-core')
  }
  try {
    return (await import('puppeteer-core')).default
  } catch {
    console.error('缺少 puppeteer-core：npm i -D puppeteer-core，或用 PUPPETEER_CORE=<安装目录> 指定')
    process.exit(2)
  }
}

const chrome = findChrome()
if (!chrome) {
  console.error('找不到 Chrome：用 CHROME=<可执行文件路径> 指定')
  process.exit(2)
}

const puppeteer = await loadPuppeteer()
mkdirSync(SHOTS, { recursive: true })

const browser = await puppeteer.launch({
  executablePath: chrome,
  headless: true,
  args: [
    '--no-sandbox',
    '--disable-dev-shm-usage',
    '--enable-unsafe-swiftshader',
    // 录音面板要能真的走一遍：用 Chrome 自带的假麦克风
    '--use-fake-ui-for-media-stream',
    '--use-fake-device-for-media-stream',
    '--autoplay-policy=no-user-gesture-required',
  ],
})
const page = await browser.newPage()
const errors = []
page.on('console', (m) => {
  if (m.type() === 'error') errors.push('console: ' + m.text())
})
page.on('pageerror', (e) => errors.push('pageerror: ' + e.message))
await page.setViewport({ width: 1500, height: 950 })

let pass = 0
let fail = 0
const check = (name, ok, extra = '') => {
  if (ok) {
    pass += 1
    console.log(`  ✓ ${name}`)
  } else {
    fail += 1
    console.log(`  ✗ ${name}${extra ? ' — ' + extra : ''}`)
  }
}
const sleep = (ms) => new Promise((r) => setTimeout(r, ms))
const count = (sel) => page.evaluate((s) => document.querySelectorAll(s).length, sel)
const text = (sel) => page.evaluate((s) => document.querySelector(s)?.textContent ?? null, sel)
const click = async (sel, nth = 0) => {
  const els = await page.$$(sel)
  if (!els[nth]) throw new Error(`找不到元素 ${sel} #${nth}`)
  await els[nth].click()
}
/** DOM click（不按坐标，面板展开时也不会点空） */
const domClick = async (sel, nth = 0) => {
  const ok = await page.evaluate(
    (s, n) => {
      const el = document.querySelectorAll(s)[n]
      if (!el) return false
      el.scrollIntoView({ block: 'center' })
      el.click()
      return true
    },
    sel,
    nth,
  )
  if (!ok) throw new Error(`点不到 ${sel} #${nth}`)
  await sleep(320)
}
const clickText = async (t, cls = '.tree-row') => {
  const ok = await page.evaluate(
    (t, cls) => {
      const hit = [...document.querySelectorAll(cls)].reverse().find((el) => (el.textContent || '').includes(t))
      if (!hit) return false
      hit.click()
      return true
    },
    t,
    cls,
  )
  if (!ok) throw new Error(`点不到「${t}」（${cls}）`)
  await sleep(400)
}
const until = async (fn, timeout = 5000) => {
  const started = Date.now()
  while (Date.now() - started < timeout) {
    if (await fn()) return true
    await sleep(120)
  }
  return false
}
/** 编辑器形态按钮：按符号点（[] = 编辑 / </> = 源码） */
const clickMode = async (mode) => {
  const ok = await page.evaluate((m) => {
    const glyph = m === 'edit' ? '[]' : '</>'
    const hit = [...document.querySelectorAll('.mode-switch .mode-part')].find(
      (el) => (el.textContent || '').trim() === glyph,
    )
    if (!hit) return false
    hit.click()
    return true
  }, mode)
  if (!ok) throw new Error(`点不到形态按钮 ${mode}`)
  await sleep(450)
}
const setTextarea = (sel, value) =>
  page.evaluate(
    (s, v) => {
      const el = document.querySelector(s)
      el.value = v
      el.dispatchEvent(new Event('input', { bubbles: true }))
    },
    sel,
    value,
  )

console.log('== 笔记：块编辑（所见即所得）==')
await page.goto(`${BASE}/#/notes`, { waitUntil: 'networkidle2' })
await sleep(1500)
await clickText('会议') // 展开示例文件夹（mock 数据）
await clickText('示例周会')
await sleep(1200)

check('块编辑器已挂载', (await count('.blocks .blk')) > 0)
check('标题渲染成 h1（不是 md 原文）', (await text('.blocks h1')) === '示例周会')
const bodyText = (await text('.blocks')) ?? ''
check('正文里看不到 md 标记', !bodyText.includes('# 示例周会') && !bodyText.includes('## 会议纪要'))
check('`/v` 行渲染成播放器行', (await count('.blocks .audio-ref')) === 1)
await page.screenshot({ path: `${SHOTS}/01-editor.png` })

console.log('== 笔记：整场引用 = 一条长语音 ==')
check(
  '整场引用渲染成场次播放器（自定义元素已升级）',
  await until(async () => (await count('bnu-session-player')) === 1),
)
const player = await page.evaluate(() => {
  const el = document.querySelector('bnu-session-player')
  if (!el) return null
  return {
    play: (el.querySelector('.sp-btn')?.textContent ?? '').trim(),
    time: el.querySelector('.sp-time')?.textContent ?? '',
    seg: el.querySelector('.sp-seg')?.textContent ?? '',
    label: el.querySelector('.sp-name')?.textContent ?? '',
    bar: !!el.querySelector('.sp-bar'),
    audio: el.querySelectorAll('audio').length,
  }
})
check(
  '播放器有播放键 / 进度条 / 段号 / 场次名',
  !!player && player.bar && player.play === '▶' && player.seg.includes('第 1/2 段'),
  JSON.stringify(player),
)
await page.evaluate(() => document.querySelector('bnu-session-player .sp-btn')?.click())
await sleep(800)
const playing = await page.evaluate(() => {
  const el = document.querySelector('bnu-session-player')
  const a = el?.querySelector('audio')
  return {
    glyph: (el?.querySelector('.sp-btn')?.textContent ?? '').trim(),
    paused: a?.paused ?? null,
    dur: a ? +a.duration.toFixed(2) : -1,
  }
})
check(
  '点播放进入播放态（按钮变暂停、媒体已就绪）',
  playing.glyph === '❚❚' && playing.paused === false && playing.dur > 0,
  JSON.stringify(playing),
)
// 无头 Chrome 没有音频输出设备，媒体时钟不前进；用真实事件来验证连播与整场定位
await page.evaluate(() =>
  document.querySelector('bnu-session-player audio')?.dispatchEvent(new Event('ended')),
)
await sleep(700)
const advanced = await page.evaluate(() => {
  const el = document.querySelector('bnu-session-player')
  return {
    seg: el?.querySelector('.sp-seg')?.textContent ?? '',
    glyph: (el?.querySelector('.sp-btn')?.textContent ?? '').trim(),
  }
})
check('一段放完自动接下一段（连播）', advanced.seg.includes('第 2/2 段'), JSON.stringify(advanced))
await page.evaluate(() => {
  const bar = document.querySelector('bnu-session-player .sp-bar')
  if (!bar) return
  bar.value = '1000'
  bar.dispatchEvent(new Event('input', { bubbles: true }))
  bar.dispatchEvent(new Event('change', { bubbles: true }))
})
await sleep(600)
const seeked = await page.evaluate(() => {
  const el = document.querySelector('bnu-session-player')
  const a = el?.querySelector('audio')
  return {
    seg: el?.querySelector('.sp-seg')?.textContent ?? '',
    time: el?.querySelector('.sp-time')?.textContent ?? '',
    cur: a ? +a.currentTime.toFixed(2) : -1,
  }
})
check(
  '进度条是整场时间轴：拖到末尾定位到第 2 段',
  seeked.seg.includes('第 2/2 段') && seeked.cur > 1,
  JSON.stringify(seeked),
)
await page.evaluate(() => document.querySelector('bnu-session-player .sp-btn')?.click())
await page.screenshot({ path: `${SHOTS}/01b-session-player.png` })

const blocksBefore = await count('.blocks .blk')
await click('.blocks .blk', 1)
check('点段落出现输入框', (await count('.blk-ta')) === 1)
check(
  '输入框里是该段原文',
  (await page.evaluate(() => document.querySelector('.blk-ta')?.value)) ===
    '随手写的要点：镜像拉取超时，改用镜像站。',
)
const editStyle = await page.evaluate(() => {
  const el = document.querySelector('.blk-ta')
  const cs = getComputedStyle(el)
  return {
    cls: el.className,
    border: cs.borderTopWidth,
    bg: cs.backgroundColor,
    tip: document.querySelectorAll('.blk-tip').length,
    hint: document.querySelectorAll('.blk-hint').length,
  }
})
check(
  '编辑态不是「输入框」：无边框、背景透明',
  editStyle.border === '0px' && editStyle.bg === 'rgba(0, 0, 0, 0)',
  JSON.stringify(editStyle),
)
check('编辑态按块类型套字号（class）', /k-[a-z0-9]+/.test(editStyle.cls), editStyle.cls)
check('常驻的「输入 # 换块类型」提示行已去掉', editStyle.tip === 0)
check('第一次编辑给一次性操作提示', editStyle.hint === 1)
await setTextarea('.blk-ta', '随手写的要点：镜像拉取超时，改用镜像站（已改）。')
await page.keyboard.down('Control')
await page.keyboard.press('Enter')
await page.keyboard.up('Control')
await sleep(500)
check('写回后回到排版', (await count('.blk-ta')) === 0)
check('改动已生效', ((await text('.blocks')) ?? '').includes('（已改）'))
check('只替换了被编辑的块（块数不变）', (await count('.blocks .blk')) === blocksBefore)
check('引用行仍在', (await count('.blocks .audio-ref')) === 1)
check('纪要标题仍在', ((await text('.blocks')) ?? '').includes('会议纪要（AI 整理）'))

// 真机反馈（Android）：长行折行后输入框高度不跟着长，光标走到第二个视觉行时
// 浏览器把输入框内部一滚，行首就被顶出去（overflow: hidden 还拉不回来）
await click('.blocks .blk', 1)
await setTextarea('.blk-ta', '')
await page.keyboard.type('长行折行测试：' + '这是一句用来测试折行的话，'.repeat(8))
await sleep(200)
const fit = await page.evaluate(() => {
  const el = document.querySelector('.blk-ta')
  return { sh: el.scrollHeight, ch: el.clientHeight, sw: el.scrollWidth, cw: el.clientWidth }
})
check('长行折行后输入框高度跟着长（行首不会被裁）', fit.sh <= fit.ch + 1, JSON.stringify(fit))
check('长行按文档宽度折行（不横向溢出）', fit.sw <= fit.cw + 1, JSON.stringify(fit))
await page.keyboard.down('Control')
await page.keyboard.press('Enter')
await page.keyboard.up('Control')
await sleep(400)

await click('.add-row')
await sleep(300)
await setTextarea('.blk-ta', '##')
await sleep(200)
check('输入 ## 弹块类型菜单', (await count('.blk-menu')) === 1)
check('菜单含各级标题 / 列表 / 引用', ((await text('.blk-menu')) ?? '').includes('二级标题'))
await page.screenshot({ path: `${SHOTS}/02-block-menu.png` })
await page.keyboard.press('Enter')
await sleep(200)
check('应用后输入框是 `## `', (await page.evaluate(() => document.querySelector('.blk-ta')?.value)) === '## ')
await setTextarea('.blk-ta', '## 测试小节')
await page.keyboard.press('Enter')
await sleep(500)
const h2s = await page.evaluate(() => [...document.querySelectorAll('.blocks h2')].map((h) => h.textContent))
check('渲染出新的二级标题', h2s.includes('测试小节'), JSON.stringify(h2s))
check('回车后新段落仍在编辑（旧输入框的 blur 不关新会话）', (await count('.blk-ta')) === 1)
await page.keyboard.press('Escape')

// 一次性提示不常驻：等它自己消失
await page.waitForFunction(() => !document.querySelector('.blk-hint'), { timeout: 12000 })
check('操作提示会自动消失（不常驻）', (await count('.blk-hint')) === 0)

console.log('== 笔记：/v 录音选择器 ==')
const refsBefore = await count('.blocks .audio-ref')
await click('.add-row')
await sleep(300)
await setTextarea('.blk-ta', '/v')
await sleep(300)
check('输入 /v 弹出录音选择器', (await count('.ref-menu')) === 1)
const pickerCount = await count('.ref-item')
check('选择器列出候选（带时长）', pickerCount >= 1, String(pickerCount))
check('候选带「已引用 / 未引用」标签', ((await text('.ref-menu')) ?? '').includes('引用'))
await page.screenshot({ path: `${SHOTS}/04-ref-picker.png` })
await setTextarea('.blk-ta', '/v seg')
await sleep(250)
const filtered = await count('.ref-item')
check('继续打字按文件名过滤', filtered >= 1 && filtered <= pickerCount, `${filtered}/${pickerCount}`)
await page.keyboard.press('Enter')
await sleep(500)
const refsAfterInsert = await count('.blocks .audio-ref')
check('Enter 选中后插入引用行（多一个播放器行）', refsAfterInsert === refsBefore + 1, `${refsBefore} -> ${refsAfterInsert}`)
check('插入后回到排版（没有输入框）', (await count('.blk-ta')) === 0)

console.log('== 笔记：引用行「⋯」→ 删除引用 ==')
const barBefore = (await text('.editor-bar')) ?? ''
const toolCount = await count('.blk-tools .tool-btn')
check('只有引用行才有「⋯」按钮', toolCount === refsAfterInsert, `${toolCount} / ${refsAfterInsert}`)
await page.evaluate(() => {
  const slot = [...document.querySelectorAll('.blk-slot')].find((s) => (s.textContent || '').includes('seg_0002'))
  slot?.querySelector('.tool-btn')?.click()
})
await sleep(350)
check('点「⋯」出现引用操作菜单', (await count('.ref-actions')) === 1)
const actions = (await text('.ref-actions')) ?? ''
check('菜单含「删除引用 / 复制音频路径」', actions.includes('删除引用') && actions.includes('复制音频路径'), actions)
await page.screenshot({ path: `${SHOTS}/05-ref-actions.png` })
await page.evaluate(() => {
  const item = [...document.querySelectorAll('.act-item')].find((b) => b.textContent.includes('删除引用'))
  item?.click()
})
await sleep(500)
check('删除后文档里少了一行引用', (await count('.blocks .audio-ref')) === refsAfterInsert - 1)
const segs = (t) => Number(/(\d+)\s*段/.exec(t)?.[1] ?? -1)
// 等自动保存落盘（工具栏的录音计数是照存盘内容算的）
await page.waitForFunction(() => /已保存/.test(document.querySelector('.editor-bar')?.textContent ?? ''), {
  timeout: 8000,
})
const barAfter = (await text('.editor-bar')) ?? ''
if ((await count('.clips-panel')) === 0) {
  await page.evaluate(() => {
    const btn = [...document.querySelectorAll('.editor-bar .t-button')].find((b) => /\d+\s*段/.test(b.textContent))
    btn?.click()
  })
  await sleep(400)
}
const clipRows = await count('.clips-panel .clip-row')
check(
  '音频文件没被删（片段总数不变、录音面板里还在）',
  segs(barAfter) === segs(barBefore) && clipRows === segs(barAfter),
  `${barBefore} -> ${barAfter}，片段行 ${clipRows}`,
)
check('删除引用不动其它内容', ((await text('.blocks')) ?? '').includes('会议纪要（AI 整理）'))

console.log('== 录音面板（假麦克风）==')
await click('.capsule')
await sleep(400)
check('展开录音面板', (await count('.panel')) === 1)
// v0.3.6：面板压成「一行小条」，别挡内容
const idleBox = await page.evaluate(() => {
  const r = document.querySelector('.panel').getBoundingClientRect()
  return { w: Math.round(r.width), h: Math.round(r.height) }
})
check('展开态是一行小条（宽 ≤ 260 / 高 ≤ 84）', idleBox.w <= 260 && idleBox.h <= 84, JSON.stringify(idleBox))
await page.screenshot({ path: `${SHOTS}/06-recorder-idle.png` })
const recReady = await page.evaluate(() => {
  const b = document.querySelector('[title="开始录音"]')
  return {
    exists: !!b,
    disabled: b?.disabled ?? null,
    panel: (document.querySelector('.panel')?.textContent ?? '').slice(0, 60),
  }
})
check('开始按钮可用（录音目标笔记已在）', recReady.exists && recReady.disabled === false, JSON.stringify(recReady))
await click('[title="开始录音"]')
await sleep(2600)
const liveState = await page.evaluate(() => ({
  panel: (document.querySelector('.panel')?.textContent ?? '').slice(0, 90),
  timer: document.querySelector('.panel .timer')?.textContent ?? null,
  chips: document.querySelectorAll('.panel .chip').length,
  level: document.querySelector('.panel .level i')?.style.width ?? '',
  stop: !!document.querySelector('.panel [title="停止录音"]'),
  start: !!document.querySelector('.panel [title="开始录音"]'),
  h: Math.round(document.querySelector('.panel').getBoundingClientRect().height),
}))
check('录音中有计时', !!liveState.timer && liveState.timer !== '0:00', String(liveState.timer))
check('录音中有电平与分段信息', liveState.chips >= 2 && liveState.level.endsWith('%'), JSON.stringify(liveState))
check(
  '按钮变成「停止」且录音态也仍是一行小条',
  liveState.stop && !liveState.start && liveState.h <= 84,
  JSON.stringify(liveState),
)
await page.screenshot({ path: `${SHOTS}/07-recorder-live.png` })
await page.evaluate(() => document.querySelector('.panel .icon-btn')?.click())
await sleep(600)
check('收起后是录音中的胶囊（红点 + 计时）', (await count('.capsule.live')) === 1)
await page.screenshot({ path: `${SHOTS}/08-capsule-live.png` })
await click('.capsule.live')
await sleep(300)
await click('[title="停止录音"]')
await sleep(1200)
const saved = (await text('.panel')) ?? ''
check(
  '停止后提示本场已存段数，并给「插入整场」入口',
  saved.includes('本场已存') && saved.includes('插入整场'),
  saved.slice(0, 80),
)

// 面板「插入整场」：插一行目录引用，存盘后应该变成真播放器（中途不能闪「分段不在」）
const beforeInsert = await page.evaluate(() => ({
  refs: document.querySelectorAll('.blocks .audio-ref').length,
  players: document.querySelectorAll('bnu-session-player').length,
}))
await page.evaluate(() => {
  const btn = [...document.querySelectorAll('.panel .link')].find((b) =>
    b.textContent.includes('插入整场'),
  )
  btn?.click()
})
const insertedOk = await until(async () => {
  const now = await page.evaluate(() => ({
    players: document.querySelectorAll('bnu-session-player').length,
    broken: [...document.querySelectorAll('.blocks .audio-ref')].some((el) =>
      el.textContent.includes('分段不在'),
    ),
  }))
  return now.players === beforeInsert.players + 1 && !now.broken
}, 8000)
const afterInsert = await page.evaluate(() => ({
  refs: document.querySelectorAll('.blocks .audio-ref').length,
  players: document.querySelectorAll('bnu-session-player').length,
  broken: [...document.querySelectorAll('.blocks .audio-ref')].some((el) =>
    el.textContent.includes('分段不在'),
  ),
}))
check(
  '「插入整场」插成一行引用，存盘后变成真播放器（不闪「分段不在」）',
  insertedOk && afterInsert.refs === beforeInsert.refs + 1,
  JSON.stringify({ beforeInsert, afterInsert }),
)

// （v0.3.5 去掉了「预览」：块编辑本身就是最终排版，这里改测「编辑态宽度 == 渲染态宽度」）
const modeInfo = await page.evaluate(() => ({
  parts: [...document.querySelectorAll('.mode-switch .mode-part')].map((el) => ({
    glyph: (el.textContent || '').trim(),
    on: el.className.includes('on'),
  })),
  radios: document.querySelectorAll('.editor-bar .t-radio-button').length,
  barText: (document.querySelector('.mode-switch')?.textContent ?? '').trim(),
}))
check(
  '形态开关：只有一个圆角按钮，里面是 [] 与 </> 两个符号（没有文字）',
  modeInfo.parts.length === 2 &&
    modeInfo.parts.some((p) => p.glyph === '[]' && p.on) &&
    modeInfo.parts.some((p) => p.glyph === '</>') &&
    modeInfo.radios === 0 &&
    !/[\u4e00-\u9fa5]/.test(modeInfo.barText),
  JSON.stringify(modeInfo),
)
await clickMode('edit')
// 录音面板/片段面板会把正文挤下去，坐标点击可能落空：这里用 DOM click 点段落
await domClick('.blocks .blk', 1)
await domClick('.blocks .blk', 1)
await sleep(350)
const widthPair = await page.evaluate(() => {
  const ta = document.querySelector('.blk-ta')
  const bodies = document.querySelectorAll('.blocks .blk.md-body')
  const body = bodies[1] ?? bodies[0]
  const bodyCs = body ? getComputedStyle(body) : null
  return {
    // textarea 没有内边距，所以和渲染块「去掉左右 padding 的内容宽」比
    ta: !!ta,
    blocks: document.querySelectorAll('.blocks .blk').length,
    edit: ta ? +ta.getBoundingClientRect().width.toFixed(1) : -1,
    render: body
      ? +(body.getBoundingClientRect().width - parseFloat(bodyCs.paddingLeft) - parseFloat(bodyCs.paddingRight)).toFixed(1)
      : -1,
    pane: +document.querySelector('.editor-body').getBoundingClientRect().width.toFixed(1),
  }
})
check(
  '点段落进入编辑态',
  widthPair.ta === true,
  JSON.stringify(widthPair),
)
check(
  '编辑态与渲染态同宽（不再「半屏就换行」）',
  Math.abs(widthPair.edit - widthPair.render) <= 1 && widthPair.edit > widthPair.pane * 0.8,
  JSON.stringify(widthPair),
)
await page.keyboard.press('Escape')
await sleep(200)
await clickMode('source')
await sleep(400)
check('源码形态出现 textarea', (await count('textarea.editor')) === 1)
const raw = await page.evaluate(() => document.querySelector('textarea.editor')?.value ?? '')
check('源码里能看到原文与新小节', raw.includes('## 测试小节') && raw.includes('/v 会议音频/'))
check(
  '源码形态下形态按钮高亮切到 </>',
  await page.evaluate(() => {
    const on = [...document.querySelectorAll('.mode-switch .mode-part')].find((el) =>
      el.className.includes('on'),
    )
    return (on?.textContent ?? '').trim() === '</>'
  }),
)
await page.screenshot({ path: `${SHOTS}/03-source.png` })
await clickMode('edit')

console.log('== 布局：左侧文件树 / 导航收起展开 ==')
await page.evaluate(() => document.querySelector('.list-head .pane-btn')?.click())
await sleep(350)
check(
  '收起文件树：列表隐藏，编辑区吃满宽度',
  await page.evaluate(() => {
    const pane = document.querySelector('.list-pane')
    const editor = document.querySelector('.editor-pane')
    return (
      !!pane &&
      getComputedStyle(pane).display === 'none' &&
      editor.getBoundingClientRect().width > 900
    )
  }),
)
await page.screenshot({ path: `${SHOTS}/13-pane-collapsed.png` })
await page.evaluate(() => document.querySelector('.page-header .pane-btn')?.click())
await sleep(350)
check(
  '页头按钮能把文件树再展开',
  await page.evaluate(() => {
    const pane = document.querySelector('.list-pane')
    return !!pane && getComputedStyle(pane).display !== 'none'
  }),
)

await page.evaluate(() => document.querySelector('.nav-toggle')?.click())
await sleep(350)
check(
  '收起左侧导航（只留图标）',
  await page.evaluate(() => {
    const s = document.querySelector('.sidebar')
    return s.classList.contains('collapsed') && s.getBoundingClientRect().width <= 60
  }),
)
await page.screenshot({ path: `${SHOTS}/14-nav-collapsed.png` })
await page.evaluate(() => document.querySelector('.nav-toggle')?.click())
await sleep(350)
check(
  '再点一次展开回原样',
  await page.evaluate(() => {
    const s = document.querySelector('.sidebar')
    return !s.classList.contains('collapsed') && s.getBoundingClientRect().width > 150
  }),
)

console.log('== 图谱：3D ↔ 2D 切换 ==')
await page.goto(`${BASE}/#/graph`, { waitUntil: 'networkidle2' })
await sleep(2500)
check('2D 力导图有画布', (await count('.canvas canvas')) > 0)
await page.screenshot({ path: `${SHOTS}/10-graph-2d.png` })

await clickText('3D 星球', '.t-radio-button')
await sleep(4000)
check('3D 场景已渲染', (await count('.g3d canvas')) > 0)
await page.screenshot({ path: `${SHOTS}/11-graph-3d.png` })

// 用户报过的 bug 路径：3D → 去设置 → 回图谱（此时仍是 3D）→ 切回 2D
await page.goto(`${BASE}/#/settings`, { waitUntil: 'networkidle2' })
await sleep(900)

console.log('== 设置：录音与转写 ==')
const asrCard = await page.evaluate(() => {
  const card = [...document.querySelectorAll('.card')].find(
    (c) => c.querySelector('.card-title')?.textContent?.trim() === '录音与转写',
  )
  const sw = card?.querySelector('.t-switch')
  return {
    found: !!card,
    titles: [...document.querySelectorAll('.card-title')].map((el) => el.textContent?.trim()),
    on: sw ? sw.className.includes('checked') : null,
    label: card?.querySelector('.switch-label')?.textContent?.trim() ?? '',
    note: (card?.textContent ?? '').includes('不经中间服务器'),
  }
})
check(
  '设置页有「录音与转写」卡片：自动转写默认开、说明写清直连与只写缓存',
  asrCard.found && asrCard.on === true && asrCard.label.includes('自动转写') && asrCard.note,
  JSON.stringify(asrCard),
)
const toggled = await page.evaluate(async () => {
  const card = [...document.querySelectorAll('.card')].find(
    (c) => c.querySelector('.card-title')?.textContent?.trim() === '录音与转写',
  )
  const sw = card?.querySelector('.t-switch')
  sw?.click()
  await new Promise((r) => setTimeout(r, 200))
  const off = sw ? sw.className.includes('checked') : null
  const savedOff = localStorage.getItem('bnu-notes-auto-transcribe')
  sw?.click()
  await new Promise((r) => setTimeout(r, 200))
  return { off, savedOff, on: sw ? sw.className.includes('checked') : null, savedOn: localStorage.getItem('bnu-notes-auto-transcribe') }
})
check(
  '开关能切换并记住（关 = localStorage 0，开 = 1）',
  toggled.off === false && toggled.savedOff === '0' && toggled.on === true && toggled.savedOn === '1',
  JSON.stringify(toggled),
)
await page.screenshot({ path: `${SHOTS}/15-settings-asr.png` })

await page.goto(`${BASE}/#/graph`, { waitUntil: 'networkidle2' })
await page.goto(`${BASE}/#/graph`, { waitUntil: 'networkidle2' })
await sleep(3500)
const state = await page.evaluate(() => {
  const radios = [...document.querySelectorAll('.t-radio-button')].map((r) => ({
    text: r.textContent.trim(),
    on: r.className.includes('t-is-checked'),
  }))
  const twoD = document.querySelector('.canvas')
  return {
    radios,
    g3d: document.querySelectorAll('.g3d canvas').length,
    twoDCanvases: twoD ? twoD.querySelectorAll('canvas').length : -1,
  }
})
check(
  '回到图谱仍是 3D 选中',
  state.radios.some((r) => r.text.includes('3D') && r.on),
  JSON.stringify(state.radios),
)
check('3D 场景在渲染', state.g3d > 0)
check('3D 模式下 2D 容器里没有画布（不会两个视图同屏）', state.twoDCanvases <= 0, JSON.stringify(state))
await page.screenshot({ path: `${SHOTS}/12-graph-3d-return.png` })

// 悬停节点暂停自转，移开继续（`__bnuGraph3D` 是开发模式的把手）
const spot = await page.evaluate(() => {
  const g = window.__bnuGraph3D
  if (!g) return null
  const node = g.graphData().nodes.filter((n) => Number.isFinite(n.x) && Number.isFinite(n.y))[0]
  if (!node) return null
  const p = g.graph2ScreenCoords(node.x, node.y, node.z)
  const r = document.querySelector('.canvas3d').getBoundingClientRect()
  return { x: r.x + p.x, y: r.y + p.y, rotate: g.controls().autoRotate }
})
if (spot) {
  await page.mouse.move(spot.x, spot.y)
  await sleep(400)
  const hoverRotate = await page.evaluate(() => window.__bnuGraph3D.controls().autoRotate)
  await page.mouse.move(spot.x, spot.y + 300)
  await sleep(500)
  const leaveRotate = await page.evaluate(() => window.__bnuGraph3D.controls().autoRotate)
  check('悬停节点时暂停自转', spot.rotate === true && hoverRotate === false, `${spot.rotate} -> ${hoverRotate}`)
  check('移开节点后继续自转', leaveRotate === true, String(leaveRotate))
  await page.screenshot({ path: `${SHOTS}/14-graph-3d-hover.png` })
} else {
  check('悬停节点时暂停自转', false, '没拿到 3D 图实例或节点坐标')
}

await clickText('力导图', '.t-radio-button')
await sleep(3500)
const two = await page.evaluate(() => {
  const box = document.querySelector('.canvas')
  return {
    canvases: box?.querySelectorAll('canvas').length ?? 0,
    w: box?.clientWidth ?? 0,
    h: box?.clientHeight ?? 0,
  }
})
check('切回 2D 后有画布且容器有尺寸', two.canvases > 0 && two.w > 300 && two.h > 300, JSON.stringify(two))
await page.screenshot({ path: `${SHOTS}/13-graph-back-2d.png` })

console.log(`\n== ${pass} 通过 / ${fail} 失败 ==`)
console.log('页面错误：', errors.length ? JSON.stringify(errors, null, 2) : '（无）')
await browser.close()
process.exit(fail ? 1 : 0)
