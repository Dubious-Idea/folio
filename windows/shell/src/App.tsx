import { getCurrentWindow } from '@tauri-apps/api/window'
import { openUrl } from '@tauri-apps/plugin-opener'
import { Show, createEffect, createSignal, on, onCleanup, onMount } from 'solid-js'
import * as actions from './actions'
import { api, on as listenEvent } from './api'
import { DialogHost, chooseAndAddSpace } from './components/Dialogs'
import { Inspector } from './components/Inspector'
import { MenuHost, closeMenu, isMenuOpen } from './components/Menu'
import { Sidebar, focusSearch } from './components/Sidebar'
import { Toolbar } from './components/Toolbar'
import { LibraryIcon } from './icons'
import { PageView } from './page/PageView'
import { bridge } from './page/bridge'
import * as editor from './store/editor'
import { library, nodeInfo, report, startLibrary } from './store/library'
import * as navigation from './store/navigation'
import { preference, savePreference } from './store/preferences'
import { startUpdater } from './store/updater'
import { refFromKey, type PageRef } from './types'

const SIDEBAR_LIMITS = [200, 460] as const
const INSPECTOR_LIMITS = [220, 420] as const

export function App() {
  const [sidebarWidth, setSidebarWidth] = createSignal(preference('sidebarWidth', 270))
  const [inspectorWidth, setInspectorWidth] = createSignal(preference('inspectorWidth', 280))
  /** Страница из ссылки `folio://open`, которая пришла раньше, чем её увидело дерево. */
  let pendingOpen: { ref: PageRef; edit: boolean } | null = null
  let restored = false

  onMount(async () => {
    document.addEventListener('keydown', handleShortcut, true)
    document.addEventListener('mouseup', handleMouseButtons, true)
    document.addEventListener('click', openExternalLinks, true)
    bridge.onFrameKeyDown = handleShortcut
    bridge.onFrameMouseUp = handleMouseButtons
    onCleanup(() => {
      document.removeEventListener('keydown', handleShortcut, true)
      document.removeEventListener('mouseup', handleMouseButtons, true)
      document.removeEventListener('click', openExternalLinks, true)
    })

    // Последние буквы набора ещё в таймере автосохранения: дописываем их до закрытия окна.
    void getCurrentWindow().onCloseRequested(() => editor.flushBeforeClose())

    await startLibrary()
    void getCurrentWindow().show()
    await listenEvent<string>('open-link', handleOpenUrl)
    for (const url of await api.takePendingLinks()) handleOpenUrl(url)
    void startUpdater().catch(() => undefined)
  })

  createEffect(
    on(library, () => {
      if (!library().loaded) return
      if (!restored) {
        restored = true
        restoreSelection()
      }
      openPendingPage()
    }),
  )

  createEffect(on(navigation.selection, (selection) => editor.pageChanged(selection), { defer: true }))

  function restoreSelection(): void {
    if (navigation.selection()) return
    const stored = refFromKey(preference('selectedPage', ''))
    if (stored && nodeInfo(stored)) {
      navigation.open(stored)
      return
    }
    const first = library().spaces[0]
    if (first) navigation.open({ spaceId: first.id, path: '' })
  }

  function handleOpenUrl(url: string): void {
    const parsed = parsePageLink(url)
    if (!parsed) return
    navigation.setSearchText('')
    pendingOpen = parsed
    openPendingPage()
  }

  /** Открывает страницу из `folio://open`, как только она появилась в дереве. */
  function openPendingPage(): void {
    if (!pendingOpen || !library().loaded || !nodeInfo(pendingOpen.ref)) return
    const { ref, edit } = pendingOpen
    pendingOpen = null
    navigation.open(ref)
    if (edit && !editor.isEditing()) void editor.begin(ref)
  }

  const startResize = (which: 'sidebar' | 'inspector') => (event: PointerEvent) => {
    event.preventDefault()
    const target = event.currentTarget as HTMLElement
    target.setPointerCapture(event.pointerId)
    const startX = event.clientX
    const startWidth = which === 'sidebar' ? sidebarWidth() : inspectorWidth()
    const [minimum, maximum] = which === 'sidebar' ? SIDEBAR_LIMITS : INSPECTOR_LIMITS
    document.body.classList.add('resizing')
    const move = (moveEvent: PointerEvent) => {
      const delta = moveEvent.clientX - startX
      const width = Math.round(Math.min(maximum, Math.max(minimum, startWidth + (which === 'sidebar' ? delta : -delta))))
      if (which === 'sidebar') setSidebarWidth(width)
      else setInspectorWidth(width)
    }
    const up = () => {
      document.body.classList.remove('resizing')
      target.removeEventListener('pointermove', move)
      target.removeEventListener('pointerup', up)
      savePreference(which === 'sidebar' ? 'sidebarWidth' : 'inspectorWidth', which === 'sidebar' ? sidebarWidth() : inspectorWidth())
    }
    target.addEventListener('pointermove', move)
    target.addEventListener('pointerup', up)
  }

  return (
    <div class="app">
      <div class="layout" style={{ '--sidebar-width': `${sidebarWidth()}px`, '--inspector-width': `${navigation.inspectorVisible() ? inspectorWidth() : 0}px` }}>
        <Sidebar />
        <div class="splitter" onPointerDown={startResize('sidebar')} />
        <main class="content">
          <Toolbar />
          <Show when={library().loaded && library().spaces.length === 0} fallback={<PageArea />}>
            <Welcome />
          </Show>
        </main>
        <Show when={navigation.inspectorVisible()}>
          <div class="splitter" onPointerDown={startResize('inspector')} />
          <Inspector />
        </Show>
      </div>
      <DialogHost />
      <MenuHost />
    </div>
  )
}

function PageArea() {
  return (
    <div class="page-area">
      <PageView />
      <Show when={navigation.selection() && !nodeInfo(navigation.selection()) && library().loaded}>
        <div class="empty-state overlay">
          <h2>Страница не найдена</h2>
          <p class="secondary">Её перенесли или удалили.</p>
        </div>
      </Show>
      <Show when={!navigation.selection() && library().loaded}>
        <div class="empty-state overlay">
          <p class="secondary">Выберите страницу</p>
        </div>
      </Show>
    </div>
  )
}

function Welcome() {
  return (
    <div class="empty-state welcome">
      <LibraryIcon size={44} class="accent" />
      <h2>Пространств пока нет</h2>
      <p class="secondary">Создайте «Работу» и «Личное» в папке Folio в вашей домашней папке или подключите готовую папку пространства.</p>
      <div class="row-buttons">
        <button class="button accent" onClick={() => void api.createDefaultSpaces().catch(report)}>
          Создать «Работу» и «Личное»
        </button>
        <button class="button" onClick={() => void chooseAndAddSpace()}>
          Подключить папку…
        </button>
      </div>
    </div>
  )
}

/** `folio://open?space=<id>&path=<путь>[&edit=1]`. */
function parsePageLink(url: string): { ref: PageRef; edit: boolean } | null {
  try {
    const parsed = new URL(url)
    if (parsed.protocol !== 'folio:' || parsed.hostname.toLowerCase() !== 'open') return null
    const spaceId = parsed.searchParams.get('space')
    if (!spaceId) return null
    return { ref: { spaceId: spaceId.toUpperCase(), path: parsed.searchParams.get('path') ?? '' }, edit: parsed.searchParams.get('edit') === '1' }
  } catch {
    return null
  }
}

/** Внешние ссылки в интерфейсе (выпуски на GitHub) открываются в браузере, а не в окне. */
function openExternalLinks(event: MouseEvent): void {
  const anchor = (event.target as Element | null)?.closest?.('a[href]') as HTMLAnchorElement | null
  if (!anchor || !/^https?:/i.test(anchor.href)) return
  event.preventDefault()
  void openUrl(anchor.href)
}

function handleMouseButtons(event: MouseEvent): void {
  if (event.button === 3) {
    event.preventDefault()
    navigation.goBack()
  } else if (event.button === 4) {
    event.preventDefault()
    navigation.goForward()
  }
}

/** Сочетания клавиш окна. Ловятся и во фрейме редактора — фокус там большую часть времени. */
function handleShortcut(event: KeyboardEvent): void {
  if (event.defaultPrevented) return
  const ctrl = event.ctrlKey && !event.metaKey
  const key = event.key.length === 1 ? event.key.toLowerCase() : event.key
  const code = event.code
  const run = (action: () => void) => {
    event.preventDefault()
    event.stopPropagation()
    if (isMenuOpen()) closeMenu()
    action()
  }
  const dialogOpen = navigation.sheet() !== null || navigation.pendingTrash() !== null

  // Перезагрузка, печать и поиск по странице из WebView2 в приложении не нужны и сбросили бы правку.
  if (key === 'F5' || (ctrl && (code === 'KeyR' || code === 'KeyP' || code === 'KeyG') && !event.altKey)) return run(() => undefined)
  if (dialogOpen) return

  if (ctrl && !event.altKey && !event.shiftKey && code === 'KeyE') return run(actions.toggleEditing)
  if (ctrl && !event.altKey && !event.shiftKey && code === 'KeyN') return run(() => actions.newPage())
  if (ctrl && event.shiftKey && !event.altKey && code === 'KeyN') return run(actions.newWindow)
  if (ctrl && !event.altKey && code === 'KeyF') return run(focusSearch)
  if (ctrl && !event.altKey && key === ',') return run(actions.openSettings)
  if (ctrl && event.altKey && !event.shiftKey && code === 'KeyR') return run(() => actions.revealPage())
  if (ctrl && event.altKey && event.shiftKey && code === 'KeyC') return run(() => actions.copyPagePath())
  if (ctrl && event.altKey && !event.shiftKey && code === 'KeyS') return run(() => actions.saveSnapshot())
  if (ctrl && event.altKey && code === 'KeyI') return run(actions.toggleInspector)
  if (event.altKey && !ctrl && key === 'ArrowLeft') return run(navigation.goBack)
  if (event.altKey && !ctrl && key === 'ArrowRight') return run(navigation.goForward)
  if (key === 'F2' && !editor.isEditing()) return run(() => actions.renamePage())
  if (key === 'Escape' && isMenuOpen()) return run(closeMenu)
}
