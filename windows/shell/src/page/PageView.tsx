import { open as openDialog } from '@tauri-apps/plugin-dialog'
import { Show, createEffect, createSignal, on, onMount } from 'solid-js'
import { api, errorText } from '../api'
import { noteShownLabels } from '../components/Inspector'
import { resolveWikiLink } from '../components/Sidebar'
import { TriangleAlertIcon } from '../icons'
import * as editor from '../store/editor'
import { changedPages, library, nodeInfo, pagesIndex, relocate, report, setErrorMessage } from '../store/library'
import * as navigation from '../store/navigation'
import { refFromKey, refKey, sameRef, type PagePayload } from '../types'
import { bridge, type FrameMessage } from './bridge'

/** Страница в окне: фрейм с веб-частью и загрузка текста при переходах и изменениях на диске. */
export function PageView() {
  let frame!: HTMLIFrameElement
  let loadToken = 0
  let renderedKey: string | null = null
  const [pageRevision, setPageRevision] = createSignal(0)

  onMount(() => {
    bridge.attach(frame)
    bridge.onMessage = route
  })

  // Перечитываем страницу, только если её файл изменили снаружи или поменялось дерево (заголовки, ссылки).
  createEffect(
    on(
      changedPages,
      (keys) => {
        const ref = navigation.selection()
        if (ref && keys.has(refKey(ref))) setPageRevision((value) => value + 1)
      },
      { defer: true },
    ),
  )
  createEffect(
    on(
      () => library().revision,
      () => {
        relocateIfNeeded()
        setPageRevision((value) => value + 1)
      },
      { defer: true },
    ),
  )

  createEffect(on([navigation.selection, navigation.version, editor.reloadToken, pageRevision], () => void load()))

  createEffect(
    on(navigation.headingRequest, (request) => {
      if (request) bridge.scrollToHeading(request.index)
    }),
  )

  // В старую версию правкой не войти: просмотр версии заканчивает правку.
  createEffect(
    on(navigation.version, (version) => {
      if (version) void editor.finish()
    }),
  )

  async function load(): Promise<void> {
    const ref = navigation.selection()
    const info = nodeInfo(ref)
    if (!ref || !info) return
    const token = ++loadToken
    const version = navigation.version()
    try {
      const [page, pages] = await Promise.all([api.page(ref, version), pagesIndex()])
      if (token !== loadToken) return
      const key = version ? `${refKey(ref)}@${version.hash}` : refKey(ref)
      const payload: PagePayload = {
        ...page,
        pages,
        resetScroll: renderedKey !== key,
        editable: !version && editor.isEditing() && sameRef(editor.editingRef(), ref),
      }
      if (page.pageId) navigation.setSelectedPageId(page.pageId)
      if (version) {
        editor.noteVersionRendered()
      } else {
        noteShownLabels(ref, page.labels)
        if (!editor.shouldRender(page.textHash, ref)) {
          bridge.updateHeader(payload)
          return
        }
        editor.noteRendered(ref, page.textHash)
      }
      renderedKey = key
      bridge.render(payload)
    } catch (error) {
      if (token === loadToken) report(`Страница «${info.node.title}» не открылась: ${errorText(error)}`)
    }
  }

  return (
    <div class="page-view">
      <Show when={editor.conflictRef() && sameRef(editor.conflictRef(), navigation.selection())}>
        <div class="conflict-banner" role="alert">
          <TriangleAlertIcon size={16} class="warning" />
          <span>Страницу изменили снаружи, пока вы её правили.</span>
          <span class="spacer" />
          <button class="button" onClick={editor.takeDiskVersion}>
            Взять версию с диска
          </button>
          <button class="button accent" onClick={editor.keepMyVersion}>
            Оставить мою
          </button>
        </div>
      </Show>
      <iframe ref={frame} class="page-frame" src="/page/index.html" title="Страница" />
    </div>
  )
}

/** Страницу переименовали или перенесли снаружи: находим её по id из шапки. */
function relocateIfNeeded(): void {
  const ref = navigation.selection()
  const pageId = navigation.selectedPageId()
  if (!ref || nodeInfo(ref) || !pageId) return
  const moved = relocate(ref.spaceId, pageId)
  if (moved) navigation.relocateTo(moved)
}

function route(message: FrameMessage): void {
  const current = navigation.selection()
  switch (message.type) {
    case 'outline':
      navigation.setOutline(message.items)
      break
    case 'openPage': {
      if (!current) return
      const target = resolveWikiLink(message.space, message.title, current.spaceId)
      if (target) navigation.open(target)
      else setErrorMessage(`Страницы «${message.title}» пока нет`)
      break
    }
    case 'openURL':
      if (current) api.openLink(message.href, current).catch(report)
      break
    case 'change':
      void editor.applyChange(message.markdown, message.pageKey)
      break
    case 'state':
      editor.setFormatting(message.state)
      break
    case 'renameTitle':
      void renameTitle(message.pageKey, message.title)
      break
    case 'saveAttachment':
      void saveAttachment(message.id, message.name, message.data)
      break
    case 'pickAttachment':
      void pickAttachments()
      break
    case 'notice':
      setErrorMessage(message.message)
      break
  }
}

async function renameTitle(pageKey: string, title: string): Promise<void> {
  const ref = refFromKey(pageKey)
  if (!ref) return
  // Сначала дописываем текст, пока путь страницы ещё прежний.
  const pending = bridge.takeMarkdown(false)
  if (pending) await editor.applyChange(pending.markdown, pending.pageKey)
  try {
    const renamed = await editor.rename(ref, title)
    if (sameRef(navigation.selection(), ref)) navigation.relocateTo(renamed)
    bridge.setPageKey(refKey(renamed))
  } catch (error) {
    report(error)
    bridge.restoreHeader()
  }
}

async function saveAttachment(id: string, name: string, data: string): Promise<void> {
  const ref = editor.editingRef()
  if (!ref) {
    bridge.attachmentSaved(id, null)
    return
  }
  try {
    bridge.attachmentSaved(id, await api.saveAttachment(ref, name, data))
  } catch (error) {
    report(error)
    bridge.attachmentSaved(id, null)
  }
}

async function pickAttachments(): Promise<void> {
  const ref = editor.editingRef()
  if (!ref) return
  const chosen = await openDialog({ multiple: true, directory: false, title: 'Файлы скопируются в папку страницы' })
  const paths = Array.isArray(chosen) ? chosen : chosen ? [chosen] : []
  if (paths.length === 0) return
  try {
    const items = await api.copyAttachments(ref, paths)
    bridge.run('insertAttachments', { items: JSON.stringify(items) })
  } catch (error) {
    report(error)
  }
}
