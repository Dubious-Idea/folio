import { createSignal } from 'solid-js'
import { api, errorText } from '../api'
import { bridge } from '../page/bridge'
import { emptyFormatting, refFromKey, refKey, sameRef, type FormattingState, type PageRef } from '../types'
import { nodeInfo, report } from './library'

export type SaveState = { kind: 'idle' } | { kind: 'saved'; at: Date } | { kind: 'failed'; message: string }

/** Режим правки окна: автосохранение, конфликты с правками снаружи и снимки в истории. */
const [isEditing, setIsEditing] = createSignal(false)
const [editingRef, setEditingRef] = createSignal<PageRef | null>(null)
const [saveState, setSaveState] = createSignal<SaveState>({ kind: 'idle' })
const [conflictRef, setConflictRef] = createSignal<PageRef | null>(null)
const [formatting, setFormatting] = createSignal<FormattingState>(emptyFormatting)
/** Растёт, когда страницу нужно перечитать с диска даже во время правки. */
const [reloadToken, setReloadToken] = createSignal(0)

export { conflictRef, editingRef, formatting, isEditing, reloadToken, saveState, setFormatting }

/** Отпечаток текста файла, который мы последний раз показали или записали. */
const baselines = new Map<string, string>()
/** Отпечаток текста, который сейчас в редакторе, по страницам. */
const shown = new Map<string, string>()
/** Страница, чей текст сейчас во фрейме (текущая версия, не старая из истории). */
let displayed: string | null = null
const editedRefs = new Set<string>()
let forceOverwrite = false
/** Сохранения идут по очереди: следующее знает отпечаток после предыдущего. */
let queue: Promise<void> = Promise.resolve()

const LATE_CHANGE_DELAY_MS = 1000

/** Фрейм показал текст страницы с диска. */
export function noteRendered(ref: PageRef, hash: string): void {
  displayed = refKey(ref)
  shown.set(displayed, hash)
}

/** Фрейм показал старую версию из истории: текущий текст страницы в нём больше не виден. */
export function noteVersionRendered(): void {
  displayed = null
}

/**
 * Начинает правку. Отпечаток для проверки конфликтов берётся, только если во фрейме уже эта страница;
 * иначе (новая страница, ссылка folio://…&edit=1) его выставит первая загрузка — и покажет текст сразу на правку.
 * Если взять отпечаток с диска раньше, загрузка решит, что текст уже в редакторе, и оставит там прежнюю страницу.
 */
export function begin(ref: PageRef): void {
  const key = refKey(ref)
  const isDisplayed = displayed === key && shown.has(key)
  if (isDisplayed) baselines.set(key, shown.get(key)!)
  else baselines.delete(key)
  setEditingRef(ref)
  setConflictRef(null)
  setSaveState({ kind: 'idle' })
  setFormatting(emptyFormatting)
  setIsEditing(true)
  if (isDisplayed) bridge.setEditable(true)
}

/** «Готово»: забрать последний текст, выйти из правки и сохранить снимок. */
export async function finish(): Promise<void> {
  if (!isEditing()) return
  const pending = bridge.takeMarkdown(false)
  if (pending) await applyChange(pending.markdown, pending.pageKey)
  await queue
  const ref = editingRef()
  setIsEditing(false)
  setEditingRef(null)
  setConflictRef(null)
  bridge.setEditable(false)
  if (ref) snapshot(ref, 0)
}

/** Окно закрывается посреди правки: дописать текст и сохранить снимок, не дожидаясь таймеров. */
export async function flushBeforeClose(): Promise<void> {
  if (!isEditing()) return
  const pending = bridge.takeMarkdown(false)
  if (pending) await applyChange(pending.markdown, pending.pageKey)
  await queue
  const ref = editingRef()
  if (ref && editedRefs.delete(refKey(ref))) {
    const title = nodeInfo(ref)?.node.title ?? ref.path
    await api.commitSpace(ref.spaceId, `Правка страницы «${title}»`).catch(() => undefined)
  }
}

/** Окно ушло на другую страницу посреди правки; последний текст веб-часть пришлёт вдогонку. */
export function pageChanged(to: PageRef | null): void {
  const ref = editingRef()
  if (!isEditing() || !ref || sameRef(ref, to)) return
  setIsEditing(false)
  setEditingRef(null)
  setConflictRef(null)
  snapshot(ref, LATE_CHANGE_DELAY_MS)
}

export function send(name: string, args: Record<string, unknown> = {}): void {
  if (isEditing()) bridge.run(name, args)
}

/** Показывать ли свежий текст с диска. Во время правки своё эхо пропускаем, чужую правку считаем конфликтом. */
export function shouldRender(hash: string, ref: PageRef): boolean {
  const key = refKey(ref)
  const baseline = baselines.get(key)
  if (!isEditing() || !sameRef(editingRef(), ref) || baseline === undefined) {
    baselines.set(key, hash)
    return true
  }
  if (baseline === hash) return false
  setConflictRef(ref)
  return false
}

export function applyChange(markdown: string, pageKey: string): Promise<void> {
  queue = queue.then(() => save(markdown, pageKey))
  return queue
}

async function save(markdown: string, pageKey: string): Promise<void> {
  const ref = refFromKey(pageKey)
  if (!ref) return
  const key = refKey(ref)
  const force = forceOverwrite
  forceOverwrite = false
  try {
    const result = await api.editorSave(pageKey, markdown, baselines.get(key) ?? null, force)
    if (result.status === 'conflict') {
      setConflictRef(ref)
      setSaveState({ kind: 'failed', message: 'страницу изменили снаружи' })
      return
    }
    baselines.set(key, result.hash)
    shown.set(key, result.hash)
    if (result.status === 'saved') editedRefs.add(key)
    if (sameRef(conflictRef(), ref)) setConflictRef(null)
    setSaveState({ kind: 'saved', at: new Date() })
  } catch (error) {
    setSaveState({ kind: 'failed', message: errorText(error) })
  }
}

/** «Оставить мою»: записать текст редактора поверх изменений на диске. */
export function keepMyVersion(): void {
  if (!conflictRef()) return
  forceOverwrite = true
  const pending = bridge.takeMarkdown(true)
  if (pending) void applyChange(pending.markdown, pending.pageKey)
  else forceOverwrite = false
}

/** «Взять версию с диска»: перечитать страницу, правки редактора отбросить. */
export function takeDiskVersion(): void {
  const ref = conflictRef()
  if (!ref) return
  baselines.delete(refKey(ref))
  setConflictRef(null)
  setSaveState({ kind: 'idle' })
  setReloadToken((token) => token + 1)
}

export async function setProperties(ref: PageRef, status: string | null, labels: string[]): Promise<void> {
  try {
    const { oldHash, newHash } = await api.setProperties(ref, status, labels)
    const key = refKey(ref)
    if (oldHash === newHash) return
    if (!baselines.has(key) || baselines.get(key) === oldHash) baselines.set(key, newHash)
    shown.set(key, newHash)
    editedRefs.add(key)
    if (!(isEditing() && sameRef(editingRef(), ref))) snapshot(ref, 0)
  } catch (error) {
    report(error)
  }
}

export async function rename(ref: PageRef, title: string): Promise<PageRef> {
  const renamed = await api.renamePage(ref, title)
  const renamedKey = refKey(renamed)
  // Текст в редакторе тот же, поменялись путь и заголовок в шапке: запоминаем новый отпечаток файла.
  try {
    const hash = (await api.page(renamed, null)).textHash
    baselines.set(renamedKey, hash)
    shown.set(renamedKey, hash)
    if (displayed === refKey(ref)) displayed = renamedKey
  } catch {
    baselines.delete(renamedKey)
  }
  if (refKey(ref) !== renamedKey) baselines.delete(refKey(ref))
  editedRefs.add(renamedKey)
  if (sameRef(editingRef(), ref)) setEditingRef(renamed)
  if (!isEditing()) snapshot(renamed, 0)
  return renamed
}

/** Один снимок в истории на заход в правку. */
function snapshot(ref: PageRef, delay: number): void {
  const key = refKey(ref)
  if (!editedRefs.delete(key)) return
  const title = nodeInfo(ref)?.node.title ?? ref.path
  window.setTimeout(() => {
    void queue.then(() => api.commitSpace(ref.spaceId, `Правка страницы «${title}»`)).catch(report)
  }, delay)
}
