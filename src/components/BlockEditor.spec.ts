import { beforeEach, describe, expect, it, vi } from 'vitest'
import { mount } from '@vue/test-utils'
import MarkdownIt from 'markdown-it'
import BlockEditor from './BlockEditor.vue'
import type { RefOption } from '../core/refPicker'

const md = new MarkdownIt({ html: false, linkify: true })
const render = (body: string) => md.render(body)

beforeEach(() => {
  // jsdom 没有 scrollIntoView
  Element.prototype.scrollIntoView = () => {}
})

/** 两条候选：一条未引用、一条已引用（顺序就是父组件排好的顺序）。 */
const OPTS: RefOption[] = [
  {
    path: '会议音频/周会/seg_0002.wav',
    dir: '会议音频/周会',
    file: 'seg_0002.wav',
    label: '会议音频/周会/seg_0002.wav',
    durationMs: 65000,
    used: false,
    own: true,
    modifiedAt: 200,
  },
  {
    path: '会议音频/周会/seg_0001.wav',
    dir: '会议音频/周会',
    file: 'seg_0001.wav',
    label: '会议音频/周会/seg_0001.wav',
    durationMs: 4000,
    used: true,
    own: true,
    modifiedAt: 100,
  },
]

function mountEditor(content: string, refOptions: RefOption[] = []) {
  return mount(BlockEditor, { props: { content, render, refOptions } })
}

/** 取最后一次 emit 的 markdown。 */
function lastEmitted(wrapper: ReturnType<typeof mountEditor>): string | undefined {
  const events = wrapper.emitted('update:content') as string[][] | undefined
  if (!events || !events.length) return undefined
  return events[events.length - 1][0]
}

const MEETING = [
  '正文第一段',
  '/v 会议音频/周会/seg_0001.wav',
  '',
  '## 会议纪要（AI 整理）',
  '',
  '- 结论：继续观察',
  '',
].join('\n')

describe('BlockEditor 渲染', () => {
  it('渲染排版结果，不显示 md 原文', () => {
    const wrapper = mountEditor('# 标题\n\n- 一\n- 二')
    const html = wrapper.html()
    expect(html).toContain('<h1>标题</h1>')
    expect(html).toContain('<li>一</li>')
    expect(wrapper.text()).not.toContain('# 标题')
    expect(wrapper.text()).not.toContain('- 一')
  })

  it('点某一段进入编辑，textarea 里是该段原文', async () => {
    const wrapper = mountEditor('A\n\nB')
    expect(wrapper.find('textarea').exists()).toBe(false)
    await wrapper.findAll('.blk')[0].trigger('click')
    const ta = wrapper.find('textarea')
    expect(ta.exists()).toBe(true)
    expect((ta.element as HTMLTextAreaElement).value).toBe('A')
  })

  it('改完点别处（blur）写回，只动这一段', async () => {
    const wrapper = mountEditor(MEETING)
    await wrapper.findAll('.blk')[0].trigger('click')
    const ta = wrapper.find('textarea')
    await ta.setValue('正文（改过）')
    await ta.trigger('blur')
    const out = lastEmitted(wrapper)
    expect(out).toBe(MEETING.replace('正文第一段', '正文（改过）'))
    // 引用行与纪要标题逐字节保留
    expect(out).toContain('/v 会议音频/周会/seg_0001.wav')
    expect(out).toContain('## 会议纪要（AI 整理）')
  })

  it('只读形态（预览）点段落不进编辑', async () => {
    const wrapper = mount(BlockEditor, { props: { content: 'A', render, editable: false } })
    await wrapper.find('.blk').trigger('click')
    expect(wrapper.find('textarea').exists()).toBe(false)
    expect(wrapper.find('.add-row').exists()).toBe(false)
    expect(wrapper.text()).toContain('A')
  })

  it('Esc 取消编辑，不发内容', async () => {
    const wrapper = mountEditor('A\n\nB')
    await wrapper.findAll('.blk')[0].trigger('click')
    await wrapper.find('textarea').setValue('不要了')
    await wrapper.find('textarea').trigger('keydown', { key: 'Escape' })
    expect(wrapper.find('textarea').exists()).toBe(false)
    expect(lastEmitted(wrapper)).toBeUndefined()
  })
})

describe('BlockEditor 块类型菜单', () => {
  it('空文档点提示开始写，输入 # 召唤菜单，选一级标题', async () => {
    const wrapper = mountEditor('')
    await wrapper.find('.empty-tip').trigger('mousedown')
    const ta = wrapper.find('textarea')
    expect(ta.exists()).toBe(true)

    await ta.setValue('#')
    const menu = wrapper.find('.blk-menu')
    expect(menu.exists()).toBe(true)
    expect(menu.text()).toContain('一级标题')
    expect(menu.text()).toContain('二级标题')
    expect(menu.text()).toContain('引用')

    const h2 = wrapper.findAll('.menu-item').find((b) => b.text().includes('二级标题'))!
    await h2.trigger('mousedown')
    expect((wrapper.find('textarea').element as HTMLTextAreaElement).value).toBe('## ')
    expect(wrapper.find('.blk-menu').exists()).toBe(false)

    // 补齐文字后回车：写回 + 新起一段
    const ta2 = wrapper.find('textarea')
    await ta2.setValue('## 会议记录')
    ;(ta2.element as HTMLTextAreaElement).setSelectionRange(7, 7)
    await ta2.trigger('keydown', { key: 'Enter' })
    expect(lastEmitted(wrapper)).toBe('## 会议记录\n\n')
    // 光标已在新段落里
    const ta3 = wrapper.find('textarea')
    expect((ta3.element as HTMLTextAreaElement).value).toBe('')
  })

  it('打字后不再是 # 就收起菜单', async () => {
    const wrapper = mountEditor('')
    await wrapper.find('.empty-tip').trigger('mousedown')
    const ta = wrapper.find('textarea')
    await ta.setValue('/')
    expect(wrapper.find('.blk-menu').exists()).toBe(true)
    await ta.setValue('# 标题')
    expect(wrapper.find('.blk-menu').exists()).toBe(false)
  })
})

describe('BlockEditor /v 录音选择器', () => {
  it('输入 /v 弹出候选：已引用 / 未引用 + 时长', async () => {
    const wrapper = mountEditor('', OPTS)
    await wrapper.find('.empty-tip').trigger('mousedown')
    const ta = wrapper.find('textarea')
    await ta.setValue('/')
    expect(wrapper.find('.menu-item').exists()).toBe(true)
    await ta.setValue('/v')
    // 块类型菜单让位给录音选择器
    expect(wrapper.find('.menu-item').exists()).toBe(false)
    const menu = wrapper.find('.ref-menu')
    expect(menu.exists()).toBe(true)
    const items = wrapper.findAll('.ref-item')
    expect(items).toHaveLength(2)
    expect(items[0].text()).toContain('未引用')
    expect(items[0].text()).toContain('seg_0002.wav')
    expect(items[0].text()).toContain('1:05')
    expect(items[1].text()).toContain('已引用')
  })

  it('继续打字按文件名过滤', async () => {
    const wrapper = mountEditor('', OPTS)
    await wrapper.find('.empty-tip').trigger('mousedown')
    const ta = wrapper.find('textarea')
    await ta.setValue('/v seg_0001')
    const items = wrapper.findAll('.ref-item')
    expect(items).toHaveLength(1)
    expect(items[0].text()).toContain('seg_0001.wav')
  })

  it('↑↓ 选择 + Enter 插入：正在输入的 /v 换成完整引用行', async () => {
    const wrapper = mountEditor('', OPTS)
    await wrapper.find('.empty-tip').trigger('mousedown')
    const ta = wrapper.find('textarea')
    await ta.setValue('/v')
    await ta.trigger('keydown', { key: 'ArrowDown' })
    expect(wrapper.findAll('.ref-item')[1].classes()).toContain('on')
    await wrapper.find('textarea').trigger('keydown', { key: 'Enter' })
    expect(lastEmitted(wrapper)).toBe('/v 会议音频/周会/seg_0001.wav')
    expect(wrapper.find('.ref-menu').exists()).toBe(false)
    expect(wrapper.find('textarea').exists()).toBe(false)
  })

  it('点候选也能插入（mousedown 在 blur 之前）', async () => {
    const wrapper = mountEditor('', OPTS)
    await wrapper.find('.empty-tip').trigger('mousedown')
    await wrapper.find('textarea').setValue('/v')
    await wrapper.findAll('.ref-item')[0].trigger('mousedown')
    expect(lastEmitted(wrapper)).toBe('/v 会议音频/周会/seg_0002.wav')
  })

  it('段落里换行后写 /v 也能插（只替换那一行）', async () => {
    const wrapper = mountEditor('前半段\n\n后半段', OPTS)
    await wrapper.findAll('.blk')[0].trigger('click')
    const ta = wrapper.find('textarea')
    await ta.setValue('前半段\n/v')
    await ta.trigger('keydown', { key: 'Enter' })
    expect(lastEmitted(wrapper)).toContain('/v 会议音频/周会/seg_0002.wav')
    expect(lastEmitted(wrapper)).toContain('前半段')
  })

  it('Esc 关掉选择器，不发内容', async () => {
    const wrapper = mountEditor('', OPTS)
    await wrapper.find('.empty-tip').trigger('mousedown')
    const ta = wrapper.find('textarea')
    await ta.setValue('/v')
    const before = (wrapper.emitted('update:content') ?? []).length
    await ta.trigger('keydown', { key: 'Escape' })
    expect(wrapper.find('.ref-menu').exists()).toBe(false)
    expect((wrapper.emitted('update:content') ?? []).length).toBe(before)
    // 还在编辑这一块（Esc 只关弹层，不取消编辑）
    expect(wrapper.find('textarea').exists()).toBe(true)
  })

  it('没有录音时给一句提示', async () => {
    const wrapper = mountEditor('', [])
    await wrapper.find('.empty-tip').trigger('mousedown')
    await wrapper.find('textarea').setValue('/v')
    expect(wrapper.find('.ref-empty').text()).toContain('会议音频/')
    expect(wrapper.findAll('.ref-item')).toHaveLength(0)
  })
})

describe('BlockEditor 引用行的「⋯」菜单', () => {
  function refSlot(wrapper: ReturnType<typeof mountEditor>) {
    const slot = wrapper.findAll('.blk-slot').find((s) => s.text().includes('seg_0001.wav'))
    if (!slot) throw new Error('没找到引用块')
    return slot
  }

  it('只有引用行才有「⋯」，点开有删除 / 复制路径', async () => {
    const wrapper = mountEditor(MEETING, OPTS)
    expect(wrapper.findAll('.tool-btn')).toHaveLength(1)
    await refSlot(wrapper).find('.tool-btn').trigger('click')
    const actions = wrapper.findAll('.act-item').map((b) => b.text())
    expect(actions[0]).toContain('删除引用')
    expect(actions[1]).toContain('复制音频路径')
  })

  it('删除引用：只删这一行，音频文件与其它内容都不动', async () => {
    const wrapper = mountEditor(MEETING, OPTS)
    await refSlot(wrapper).find('.tool-btn').trigger('click')
    await wrapper.findAll('.act-item')[0].trigger('click')
    const out = lastEmitted(wrapper)!
    expect(out).not.toContain('/v ')
    expect(out).toContain('正文第一段')
    expect(out).toContain('## 会议纪要（AI 整理）')
    expect(out).toContain('- 结论：继续观察')
    // 空行合成一个，不留双空行
    expect(out).toContain('正文第一段\n\n## 会议纪要（AI 整理）')
    expect(wrapper.find('.ref-actions').exists()).toBe(false)
    expect(wrapper.findAll('.tool-btn')).toHaveLength(0)
  })

  it('复制路径：菜单不关，按钮变成「已复制」', async () => {
    const wrapper = mountEditor(MEETING, OPTS)
    await refSlot(wrapper).find('.tool-btn').trigger('click')
    await wrapper.findAll('.act-item')[1].trigger('click')
    await wrapper.vm.$nextTick()
    expect(wrapper.findAll('.act-item')[1].text()).toBe('已复制')
    expect(lastEmitted(wrapper)).toBeUndefined()
  })

  it('只读形态没有「⋯」', async () => {
    const wrapper = mount(BlockEditor, { props: { content: MEETING, render, editable: false, refOptions: OPTS } })
    expect(wrapper.findAll('.tool-btn')).toHaveLength(0)
  })
})

describe('BlockEditor 编辑态形态', () => {
  it('输入框按块类型套字号 class（不是方框提示）', async () => {
    const wrapper = mountEditor('# 标题\n\n- 一\n\n> 引用')
    await wrapper.findAll('.blk')[0].trigger('click')
    expect(wrapper.find('textarea').classes()).toContain('k-h1')
    await wrapper.find('textarea').trigger('keydown', { key: 'Escape' })
    await wrapper.findAll('.blk')[1].trigger('click')
    expect(wrapper.find('textarea').classes()).toContain('k-ul')
    await wrapper.find('textarea').trigger('keydown', { key: 'Escape' })
    await wrapper.findAll('.blk')[2].trigger('click')
    expect(wrapper.find('textarea').classes()).toContain('k-quote')
    // 常驻提示行已经去掉
    expect(wrapper.find('.blk-tip').exists()).toBe(false)
  })

  it('操作提示只在第一次进编辑时露一下', async () => {
    vi.useFakeTimers()
    try {
      const wrapper = mountEditor('A\n\nB')
      await wrapper.findAll('.blk')[0].trigger('click')
      expect(wrapper.find('.blk-hint').text()).toContain('/v')
      await wrapper.find('textarea').trigger('keydown', { key: 'Escape' })
      vi.advanceTimersByTime(9000)
      await wrapper.vm.$nextTick()
      await wrapper.findAll('.blk')[0].trigger('click')
      expect(wrapper.find('.blk-hint').exists()).toBe(false)
    } finally {
      vi.useRealTimers()
    }
  })
})

describe('BlockEditor 回车拆分与引用插入', () => {
  it('单行段落回车：以光标为界拆成两段', async () => {
    const wrapper = mountEditor('前半后半')
    await wrapper.findAll('.blk')[0].trigger('click')
    const ta = wrapper.find('textarea')
    ;(ta.element as HTMLTextAreaElement).setSelectionRange(2, 2)
    await ta.trigger('keydown', { key: 'Enter' })
    expect(lastEmitted(wrapper)).toBe('前半\n\n后半')
  })

  it('列表项回车自动续标记，空标记回车退出列表', async () => {
    const wrapper = mountEditor('- 第一项')
    await wrapper.findAll('.blk')[0].trigger('click')
    const ta = wrapper.find('textarea')
    ;(ta.element as HTMLTextAreaElement).setSelectionRange(5, 5)
    await ta.trigger('keydown', { key: 'Enter' })
    expect(lastEmitted(wrapper)).toBe('- 第一项\n\n- ')

    const ta2 = wrapper.find('textarea')
    expect((ta2.element as HTMLTextAreaElement).value).toBe('- ')
    await ta2.trigger('keydown', { key: 'Enter' })
    // 只写了标记就回车 → 标记清掉，变成普通段落
    expect(lastEmitted(wrapper)).toBe('- 第一项\n\n')
  })

  it('回车拆段后仍在新段落里编辑（旧输入框的 blur 不关新会话）', async () => {
    const wrapper = mountEditor('标题行')
    await wrapper.findAll('.blk')[0].trigger('click')
    const old = wrapper.find('textarea')
    ;(old.element as HTMLTextAreaElement).setSelectionRange(3, 3)
    await old.trigger('keydown', { key: 'Enter' })
    // 浏览器在旧输入框被移除时会补一个 blur（jsdom 不会，手动补上）
    old.element.dispatchEvent(new FocusEvent('blur'))
    await wrapper.vm.$nextTick()
    const ta = wrapper.find('textarea')
    expect(ta.exists()).toBe(true)
    expect((ta.element as HTMLTextAreaElement).value).toBe('')
  })

  it('插入引用时落到纪要标题之前', async () => {
    const wrapper = mountEditor(MEETING)
    wrapper.vm.insertLines(['/v 会议音频/周会/seg_0002.wav'])
    const out = lastEmitted(wrapper)!
    expect(out.indexOf('seg_0002.wav')).toBeLessThan(out.indexOf('## 会议纪要（AI 整理）'))
    expect(out).toContain('/v 会议音频/周会/seg_0002.wav')
  })

  it('locate 给目标块加高亮', async () => {
    const wrapper = mountEditor(MEETING)
    wrapper.vm.locate(4, 4)
    await wrapper.vm.$nextTick()
    expect(wrapper.find('.blk.flash').text()).toContain('会议纪要')
  })
})
