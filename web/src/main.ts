import { Editor } from '@tiptap/core'
import type { AttachmentUploader } from './editor/attachments'
import { NATIVE_REQUEST_EVENT, runCommand, type AttachmentItem, type CommandArgs } from './editor/commands'
import { createFormattingToolbar } from './editor/formattingToolbar'
import { createExtensions } from './extensions'
import type { ChildPageSummary, PageSummary } from './extensions/folioContext'
import { collectHeadings } from './extensions/headings'
import { normalizeStatusColor } from './extensions/statusColors'
import './styles.css'

const SAVE_DELAY_MS = 400
const MAX_ATTACHMENT_BYTES = 50 * 1024 * 1024
const HEADING_LEVELS = [1, 2, 3] as const
const BLOCK_TYPES = ['taskList', 'bulletList', 'orderedList', 'blockquote', 'codeBlock'] as const

interface StatusPayload {
  label: string
  color: string
}

/** Всё, что приложение присылает для показа страницы (`PagePayload` в Swift). */
interface PagePayload {
  pageKey: string
  markdown: string
  title: string
  status: StatusPayload | null
  labels: string[]
  meta: string
  assetBase: string
  links: Record<string, boolean>
  children: ChildPageSummary[]
  pages: PageSummary[]
  spaceName: string
  banner: string | null
  resetScroll: boolean
  editable: boolean
}

/** Что сейчас под курсором — для меню и кнопок в панели окна. */
interface FormattingState {
  bold: boolean
  italic: boolean
  strike: boolean
  code: boolean
  link: boolean
  block: string
  inTable: boolean
  canUndo: boolean
  canRedo: boolean
}

interface PendingMarkdown {
  pageKey: string
  markdown: string
}

type NativeMessage =
  | { type: 'ready' }
  | { type: 'openPage'; space: string | null; title: string }
  | { type: 'openURL'; href: string }
  | { type: 'outline'; items: { level: number; text: string; index: number }[] }
  | { type: 'renderError'; message: string }
  | { type: 'change'; pageKey: string; markdown: string }
  | { type: 'state'; state: FormattingState }
  | { type: 'renameTitle'; pageKey: string; title: string }
  | { type: 'saveAttachment'; id: string; name: string; mime: string; data: string }
  | { type: 'pickAttachment' }
  | { type: 'notice'; message: string }

interface FolioBridge {
  render: (payload: PagePayload) => void
  updateHeader: (payload: PagePayload) => void
  setEditable: (editable: boolean) => void
  setPageKey: (pageKey: string) => void
  run: (name: string, args?: CommandArgs) => boolean
  takeMarkdown: (force: boolean) => PendingMarkdown | null
  attachmentSaved: (id: string, item: AttachmentItem | null) => void
  scrollToHeading: (index: number) => void
}

interface NativeHost {
  postMessage: (message: NativeMessage) => void
}

declare global {
  interface Window {
    webkit?: { messageHandlers?: { folio?: NativeHost } }
    /** Windows: редактор живёт во фрейме окна приложения, сообщения принимает родительское окно. */
    folioHost?: NativeHost
    folio: FolioBridge
  }
}

const isMac = /Mac/.test(navigator.platform)
document.documentElement.dataset.platform = isMac ? 'mac' : 'windows'

function nativeHost(): NativeHost | undefined {
  if (window.webkit?.messageHandlers?.folio) return window.webkit.messageHandlers.folio
  return window.parent !== window ? window.parent.folioHost : undefined
}

function post(message: NativeMessage): void {
  nativeHost()?.postMessage(message)
}

function requireElement<T extends HTMLElement>(id: string): T {
  const element = document.getElementById(id)
  if (!element) throw new Error(`В разметке нет элемента #${id}`)
  return element as T
}

const page = requireElement<HTMLElement>('page')
const banner = requireElement<HTMLElement>('page-banner')
const titleElement = requireElement<HTMLHeadingElement>('page-title')
const metaElement = requireElement<HTMLElement>('page-meta')
const editorElement = requireElement<HTMLElement>('editor')

// Вложения: файл уходит в приложение, ответ с путём приходит в attachmentSaved.
let attachmentCounter = 0
const pendingAttachments = new Map<string, (item: AttachmentItem | null) => void>()

const uploader: AttachmentUploader = {
  save: (file) =>
    new Promise((resolve) => {
      if (file.size > MAX_ATTACHMENT_BYTES) {
        post({ type: 'notice', message: `Файл «${file.name}» больше 50 МБ — положите его в папку страницы вручную` })
        resolve(null)
        return
      }
      const reader = new FileReader()
      reader.addEventListener('load', () => {
        const dataURL = typeof reader.result === 'string' ? reader.result : ''
        attachmentCounter += 1
        const id = `attachment-${attachmentCounter}`
        pendingAttachments.set(id, resolve)
        post({ type: 'saveAttachment', id, name: file.name, mime: file.type, data: dataURL.slice(dataURL.indexOf(',') + 1) })
      })
      reader.addEventListener('error', () => resolve(null))
      reader.readAsDataURL(file)
    }),
}

const formattingToolbar = createFormattingToolbar()
const editor = new Editor({
  element: editorElement,
  extensions: createExtensions({ bubbleMenu: formattingToolbar.element, uploader }),
  editable: false,
  content: '',
})
formattingToolbar.bind(editor)

let pageKey = ''
let savedMarkdown = ''
let committedTitle = ''
let isRendering = false
let renderFailed = false
let saveTimer: number | undefined
let lastStateJSON = ''
let stateFrame = 0

// MARK: - Сохранение

function currentMarkdown(): string {
  return editor.getMarkdown().trim()
}

/** Текст страницы, если он отличается от последнего сохранённого; force — отдать в любом случае. */
function takeMarkdown(force: boolean): PendingMarkdown | null {
  window.clearTimeout(saveTimer)
  saveTimer = undefined
  if (!pageKey || renderFailed) return null
  const markdown = currentMarkdown()
  if (!force && markdown === savedMarkdown) return null
  savedMarkdown = markdown
  return { pageKey, markdown }
}

function postPendingChange(): void {
  const pending = takeMarkdown(false)
  if (pending) post({ type: 'change', ...pending })
  postOutline()
}

editor.on('update', () => {
  if (!editor.isEditable || isRendering) return
  window.clearTimeout(saveTimer)
  saveTimer = window.setTimeout(postPendingChange, SAVE_DELAY_MS)
})

// MARK: - Состояние форматирования

function currentBlock(): string {
  for (const level of HEADING_LEVELS) {
    if (editor.isActive('heading', { level })) return `heading${level}`
  }
  return BLOCK_TYPES.find((name) => editor.isActive(name)) ?? 'paragraph'
}

function postState(): void {
  const editable = editor.isEditable
  const state: FormattingState = {
    bold: editable && editor.isActive('bold'),
    italic: editable && editor.isActive('italic'),
    strike: editable && editor.isActive('strike'),
    code: editable && editor.isActive('code'),
    link: editable && editor.isActive('link'),
    block: editable ? currentBlock() : 'paragraph',
    inTable: editable && editor.isActive('table'),
    canUndo: editable && editor.can().undo(),
    canRedo: editable && editor.can().redo(),
  }
  const json = JSON.stringify(state)
  if (json === lastStateJSON) return
  lastStateJSON = json
  post({ type: 'state', state })
}

editor.on('transaction', () => {
  if (stateFrame) return
  stateFrame = requestAnimationFrame(() => {
    stateFrame = 0
    postState()
  })
})

// MARK: - Отрисовка

function renderHeader(payload: PagePayload): void {
  committedTitle = payload.title
  if (document.activeElement !== titleElement) titleElement.textContent = payload.title
  banner.textContent = payload.banner ?? ''
  banner.hidden = !payload.banner

  const parts: HTMLElement[] = []
  if (payload.status?.label) {
    const status = document.createElement('span')
    status.className = `status status-${normalizeStatusColor(payload.status.color)}`
    status.textContent = payload.status.label
    parts.push(status)
  }
  for (const label of payload.labels) {
    const chip = document.createElement('span')
    chip.className = 'label'
    chip.textContent = label
    parts.push(chip)
  }
  if (payload.meta) {
    const meta = document.createElement('span')
    meta.className = 'page-meta-text'
    meta.textContent = payload.meta
    parts.push(meta)
  }
  metaElement.replaceChildren(...parts)
  metaElement.hidden = parts.length === 0
}

function applyContext(payload: PagePayload): void {
  Object.assign(editor.storage.folioContext, {
    links: payload.links,
    children: payload.children,
    assetBase: payload.assetBase,
    pages: payload.pages,
    spaceName: payload.spaceName,
  })
}

function postOutline(): void {
  const items = collectHeadings(editor.state.doc).map((heading, index) => ({
    level: heading.level,
    text: heading.text,
    index,
  }))
  post({ type: 'outline', items })
}

function render(payload: PagePayload): void {
  if (pageKey && payload.pageKey !== pageKey) postPendingChange()
  pageKey = payload.pageKey
  renderHeader(payload)
  applyContext(payload)

  isRendering = true
  renderFailed = false
  try {
    editor.commands.setContent(payload.markdown, { contentType: 'markdown', emitUpdate: false })
  } catch (error) {
    renderFailed = true
    post({ type: 'renderError', message: error instanceof Error ? error.message : String(error) })
    editor.commands.setContent(
      { type: 'doc', content: [{ type: 'codeBlock', content: payload.markdown ? [{ type: 'text', text: payload.markdown }] : [] }] },
      { emitUpdate: false },
    )
  } finally {
    isRendering = false
  }

  savedMarkdown = currentMarkdown()
  editorElement.dataset.empty = String(payload.markdown.trim() === '')
  applyEditable(payload.editable)
  page.hidden = false
  if (payload.resetScroll) window.scrollTo({ top: 0 })
  queueMicrotask(postOutline)
}

// MARK: - Режим правки

function applyEditable(requested: boolean): void {
  const editable = requested && !renderFailed
  if (editor.isEditable !== editable) {
    if (!editable) postPendingChange()
    editor.setEditable(editable, false)
  }
  page.classList.toggle('is-editing', editable)
  titleElement.contentEditable = editable ? 'plaintext-only' : 'false'
  titleElement.spellcheck = editable
  postState()
}

function setEditable(requested: boolean): void {
  const wasEditable = editor.isEditable
  applyEditable(requested)
  if (editor.isEditable && !wasEditable) {
    editor.commands.focus(null, { scrollIntoView: false })
  }
}

titleElement.addEventListener('keydown', (event) => {
  if (!editor.isEditable) return
  if (event.key === 'Enter') {
    event.preventDefault()
    titleElement.blur()
  } else if (event.key === 'Escape') {
    event.preventDefault()
    titleElement.textContent = committedTitle
    titleElement.blur()
  }
})

titleElement.addEventListener('blur', () => {
  if (!editor.isEditable) return
  const title = (titleElement.textContent ?? '').replace(/\s+/g, ' ').trim()
  if (!title || title === committedTitle) {
    titleElement.textContent = committedTitle
    return
  }
  post({ type: 'renameTitle', pageKey, title })
})

// MARK: - Ссылки и просьбы к приложению

page.addEventListener('click', (event) => {
  const target = event.target instanceof Element ? event.target : null
  if (!target) return
  const wikiLink = target.closest<HTMLElement>('a.wiki-link')
  const anchor = wikiLink ? null : target.closest<HTMLAnchorElement>('a[href]')
  if (!wikiLink && (!anchor || anchor.closest('.page-block'))) return

  event.preventDefault()
  // В режиме правки ссылка — это текст; открыть её можно с ⌘ (на Windows — с Ctrl).
  if (editor.isEditable && !(isMac ? event.metaKey : event.ctrlKey)) return

  if (wikiLink) {
    post({ type: 'openPage', space: wikiLink.dataset.space || null, title: wikiLink.dataset.title ?? '' })
  } else if (anchor) {
    post({ type: 'openURL', href: anchor.getAttribute('href') ?? '' })
  }
})

window.addEventListener(NATIVE_REQUEST_EVENT, (event) => {
  if (event instanceof CustomEvent && event.detail?.type === 'pickAttachment') {
    post({ type: 'pickAttachment' })
  }
})

function scrollToHeading(index: number): void {
  const heading = collectHeadings(editor.state.doc)[index]
  if (!heading) return
  const target = editor.view.nodeDOM(heading.pos)
  if (target instanceof HTMLElement) target.scrollIntoView({ block: 'start' })
}

window.folio = {
  render,
  updateHeader: (payload) => {
    pageKey = payload.pageKey
    renderHeader(payload)
    applyContext(payload)
  },
  setEditable,
  setPageKey: (key) => {
    pageKey = key
  },
  run: (name, args) => runCommand(editor, name, args ?? {}),
  takeMarkdown,
  attachmentSaved: (id, item) => {
    pendingAttachments.get(id)?.(item)
    pendingAttachments.delete(id)
  },
  scrollToHeading,
}

post({ type: 'ready' })
