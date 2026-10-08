import type { AttachmentItem, FormattingState, OutlineItem, PagePayload } from '../types'

/** Сообщения веб-части редактора (`NativeMessage` в web/src/main.ts). */
export type FrameMessage =
  | { type: 'ready' }
  | { type: 'openPage'; space: string | null; title: string }
  | { type: 'openURL'; href: string }
  | { type: 'outline'; items: OutlineItem[] }
  | { type: 'renderError'; message: string }
  | { type: 'change'; pageKey: string; markdown: string }
  | { type: 'state'; state: FormattingState }
  | { type: 'renameTitle'; pageKey: string; title: string }
  | { type: 'saveAttachment'; id: string; name: string; mime: string; data: string }
  | { type: 'pickAttachment' }
  | { type: 'notice'; message: string }

interface FolioFrame {
  render: (payload: PagePayload) => void
  updateHeader: (payload: PagePayload) => void
  setEditable: (editable: boolean) => void
  setPageKey: (pageKey: string) => void
  run: (name: string, args?: Record<string, unknown>) => boolean
  takeMarkdown: (force: boolean) => { pageKey: string; markdown: string } | null
  attachmentSaved: (id: string, item: AttachmentItem | null) => void
  scrollToHeading: (index: number) => void
}

declare global {
  interface Window {
    folioHost?: { postMessage: (message: FrameMessage) => void }
  }
}

/**
 * Один фрейм с редактором на окно: веб-часть грузится один раз, дальше ей передаются страницы и команды.
 * Фрейм того же происхождения, поэтому вызовы прямые и синхронные — без сериализации, как через WKWebView.
 */
class PageBridge {
  private frame: HTMLIFrameElement | null = null
  private isReady = false
  private lastPayload: PagePayload | null = null
  onMessage: (message: FrameMessage) => void = () => {}
  onFrameKeyDown: (event: KeyboardEvent) => void = () => {}
  onFrameMouseUp: (event: MouseEvent) => void = () => {}

  constructor() {
    window.folioHost = { postMessage: (message) => this.receive(message) }
  }

  attach(frame: HTMLIFrameElement): void {
    this.frame = frame
    frame.addEventListener('load', () => {
      const frameWindow = frame.contentWindow
      if (!frameWindow) return
      // Сочетания клавиш окна работают и когда фокус в тексте страницы.
      frameWindow.addEventListener('keydown', (event) => this.onFrameKeyDown(event), true)
      frameWindow.addEventListener('mouseup', (event) => this.onFrameMouseUp(event), true)
      // Меню браузера («Обновить», «Печать») в тексте страницы не нужно; в режиме правки оставляем подсказки правописания.
      frameWindow.document.addEventListener('contextmenu', (event) => {
        const target = event.target as Element | null
        if (!target?.closest?.('[contenteditable="true"], input, textarea')) event.preventDefault()
      })
    })
  }

  private get folio(): FolioFrame | undefined {
    if (!this.isReady) return undefined
    return (this.frame?.contentWindow as (Window & { folio?: FolioFrame }) | null)?.folio
  }

  render(payload: PagePayload): void {
    this.lastPayload = payload
    this.folio?.render(payload)
  }

  /** Шапка и контекст без замены текста — во время правки, когда текст на диске совпадает с редактором. */
  updateHeader(payload: PagePayload): void {
    this.lastPayload = payload
    this.folio?.updateHeader(payload)
  }

  restoreHeader(): void {
    if (this.lastPayload) this.folio?.updateHeader(this.lastPayload)
  }

  setEditable(editable: boolean): void {
    this.folio?.setEditable(editable)
  }

  setPageKey(key: string): void {
    this.folio?.setPageKey(key)
  }

  run(name: string, args: Record<string, unknown> = {}): void {
    this.folio?.run(name, args)
  }

  scrollToHeading(index: number): void {
    this.folio?.scrollToHeading(index)
  }

  attachmentSaved(id: string, item: AttachmentItem | null): void {
    this.folio?.attachmentSaved(id, item)
  }

  /** Текст редактора, если он ещё не отправлен; force — отдать в любом случае. */
  takeMarkdown(force: boolean): { pageKey: string; markdown: string } | null {
    const pending = this.folio?.takeMarkdown(force)
    return pending ? { pageKey: pending.pageKey, markdown: pending.markdown } : null
  }

  focus(): void {
    this.frame?.contentWindow?.focus()
  }

  private receive(message: FrameMessage): void {
    if (message.type === 'ready') {
      this.isReady = true
      if (this.lastPayload) this.folio?.render(this.lastPayload)
      return
    }
    if (message.type === 'renderError') {
      console.error('Страница не отрисовалась:', message.message)
      return
    }
    this.onMessage(message)
  }
}

export const bridge = new PageBridge()
