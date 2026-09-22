import { beforeEach, describe, expect, it } from 'vitest'
import { mount } from '@vue/test-utils'
import MarkdownIt from 'markdown-it'
import BlockEditor from './BlockEditor.vue'

const md = new MarkdownIt({ html: false, linkify: true })
const render = (body: string) => md.render(body)

beforeEach(() => {
  // jsdom 没有 scrollIntoView
  Element.prototype.scrollIntoView = () => {}
})

function mountEditor(content: string) {
  return mount(BlockEditor, { props: { content, render } })
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
    await ta.setValue('/v 会议音频/a.wav')
    expect(wrapper.find('.blk-menu').exists()).toBe(false)
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
