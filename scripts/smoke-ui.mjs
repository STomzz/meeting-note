/**
 * 真浏览器 UI 冒烟（笔记块编辑 + 图谱 2D/3D 切换）。
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
  args: ['--no-sandbox', '--disable-dev-shm-usage', '--enable-unsafe-swiftshader'],
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

const blocksBefore = await count('.blocks .blk')
await click('.blocks .blk', 1)
check('点段落出现输入框', (await count('.blk-ta')) === 1)
check(
  '输入框里是该段原文',
  (await page.evaluate(() => document.querySelector('.blk-ta')?.value)) ===
    '随手写的要点：镜像拉取超时，改用镜像站。',
)
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

await clickText('预览', '.t-radio-button')
await sleep(400)
await click('.blocks .blk', 1)
check('预览形态点段落不进编辑', (await count('.blk-ta')) === 0)
await clickText('源码', '.t-radio-button')
await sleep(500)
check('源码形态出现 textarea', (await count('textarea.editor')) === 1)
const raw = await page.evaluate(() => document.querySelector('textarea.editor')?.value ?? '')
check('源码里能看到原文与新小节', raw.includes('## 测试小节') && raw.includes('/v 会议音频/'))
await page.screenshot({ path: `${SHOTS}/03-source.png` })
await clickText('编辑', '.t-radio-button')

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
await sleep(800)
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
