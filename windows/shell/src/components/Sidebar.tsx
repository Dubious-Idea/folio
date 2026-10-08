import { For, Index, Show, createEffect, createMemo, createSignal, on, onCleanup, onMount } from 'solid-js'
import * as actions from '../actions'
import { api } from '../api'
import { titleKey } from '../format'
import { ChevronRightIcon, FileTextIcon, FolderIcon, HouseIcon, SearchIcon, SettingsIcon, TriangleAlertIcon, XIcon } from '../icons'
import { ancestorTitles, indexRevision, library, report, spaceById } from '../store/library'
import * as navigation from '../store/navigation'
import { refKey, sameRef, type PageRef, type SearchHit, type SpaceInfo, type TreeNode } from '../types'
import { showContextMenu, type MenuItem } from './Menu'
import { SpaceIcon } from './SpaceIcon'
import { UpdateBanner } from './Updates'

const ROW_HEIGHT = 28
const OVERSCAN = 10

type Row =
  | { kind: 'space'; key: string; space: SpaceInfo; expanded: boolean }
  | { kind: 'page'; key: string; space: SpaceInfo; node: TreeNode; ref: PageRef; level: number; expandable: boolean; expanded: boolean; home: boolean }
  | { kind: 'missing-header'; key: string }
  | { kind: 'missing'; key: string; path: string }

let searchInput: HTMLInputElement | undefined

export function focusSearch(): void {
  searchInput?.focus()
  searchInput?.select()
}

export function Sidebar() {
  return (
    <aside class="sidebar">
      <div class="search-box">
        <SearchIcon class="search-icon" size={16} />
        <input
          ref={searchInput}
          type="search"
          placeholder="Поиск по страницам"
          value={navigation.searchText()}
          onInput={(event) => navigation.setSearchText(event.currentTarget.value)}
          onKeyDown={(event) => {
            if (event.key === 'Escape') navigation.setSearchText('')
          }}
          spellcheck={false}
        />
        <Show when={navigation.searchText()}>
          <button class="search-clear" title="Очистить" onClick={() => navigation.setSearchText('')}>
            <XIcon size={14} />
          </button>
        </Show>
      </div>
      <Show when={navigation.searchText().trim()} fallback={<PageTree />}>
        {(query) => <SearchResults query={query()} />}
      </Show>
      <UpdateBanner />
      <div class="sidebar-footer">
        <button class="footer-button" onClick={actions.openSettings} title="Настройки (Ctrl+,)">
          <SettingsIcon size={16} />
          <span>Настройки</span>
        </button>
      </div>
    </aside>
  )
}

function PageTree() {
  let scroller!: HTMLDivElement
  const [scrollTop, setScrollTop] = createSignal(0)
  const [viewport, setViewport] = createSignal(800)

  const rows = createMemo<Row[]>(() => {
    const expanded = navigation.expanded()
    const collapsedSpaces = navigation.collapsedSpaces()
    const result: Row[] = []
    for (const space of library().spaces) {
      const spaceOpen = !collapsedSpaces.has(space.id)
      result.push({ kind: 'space', key: `space:${space.id}`, space, expanded: spaceOpen })
      if (!spaceOpen) continue
      // Узлы идут в прямом порядке обхода: свёрнутая ветка — это следующие узлы с большей глубиной.
      let skipDeeperThan = Number.POSITIVE_INFINITY
      for (const node of space.nodes) {
        if (node.depth > skipDeeperThan) continue
        skipDeeperThan = Number.POSITIVE_INFINITY
        const ref = { spaceId: space.id, path: node.path }
        const key = refKey(ref)
        const home = node.depth === 0
        const open = home || expanded.has(key)
        result.push({
          kind: 'page',
          key,
          space,
          node,
          ref,
          level: home ? 0 : node.depth - 1,
          expandable: !home && node.childCount > 0,
          expanded: open,
          home,
        })
        if (!open) skipDeeperThan = node.depth
      }
    }
    if (library().missingPaths.length > 0) {
      result.push({ kind: 'missing-header', key: 'missing' })
      for (const path of library().missingPaths) result.push({ kind: 'missing', key: `missing:${path}`, path })
    }
    return result
  })

  const first = () => Math.max(0, Math.floor(scrollTop() / ROW_HEIGHT) - OVERSCAN)
  const last = () => Math.min(rows().length, Math.ceil((scrollTop() + viewport()) / ROW_HEIGHT) + OVERSCAN)
  const visible = createMemo(() => rows().slice(first(), last()))
  const selectedKey = () => {
    const selection = navigation.selection()
    return selection ? refKey(selection) : ''
  }

  onMount(() => {
    const observer = new ResizeObserver(() => setViewport(scroller.clientHeight))
    observer.observe(scroller)
    onCleanup(() => observer.disconnect())
  })

  // Выбранная страница всегда видна: после перехода по ссылке дерево прокручивается к ней.
  createEffect(
    on([selectedKey, rows], ([key]) => {
      const index = rows().findIndex((row) => row.key === key)
      if (index < 0) return
      const top = index * ROW_HEIGHT
      if (top < scroller.scrollTop) scroller.scrollTop = top
      else if (top + ROW_HEIGHT > scroller.scrollTop + scroller.clientHeight) scroller.scrollTop = top + ROW_HEIGHT - scroller.clientHeight
    }),
  )

  const pageRows = () => rows().filter((row): row is Extract<Row, { kind: 'page' }> => row.kind === 'page')

  const onKeyDown = (event: KeyboardEvent) => {
    const pages = pageRows()
    const index = pages.findIndex((row) => row.key === selectedKey())
    const current = pages[index]
    const move = (target: number) => {
      const row = pages[Math.max(0, Math.min(pages.length - 1, target))]
      if (row) navigation.open(row.ref)
    }
    switch (event.key) {
      case 'ArrowDown':
        move(index + 1)
        break
      case 'ArrowUp':
        move(index - 1)
        break
      case 'Home':
        move(0)
        break
      case 'End':
        move(pages.length - 1)
        break
      case 'ArrowRight':
        if (current?.expandable && !current.expanded) navigation.setPageExpanded(current.ref, true)
        else move(index + 1)
        break
      case 'ArrowLeft':
        if (current?.expandable && current.expanded) navigation.setPageExpanded(current.ref, false)
        else if (current && current.ref.path) {
          const parent = current.ref.path.split('/').slice(0, -1).join('/')
          navigation.open({ spaceId: current.ref.spaceId, path: parent })
        }
        break
      case 'F2':
        actions.renamePage(current?.ref ?? null)
        break
      case 'Delete':
        actions.trashPage(current?.ref ?? null)
        break
      default:
        return
    }
    event.preventDefault()
  }

  return (
    <div
      ref={scroller}
      class="tree"
      role="tree"
      tabIndex={0}
      onScroll={(event) => setScrollTop(event.currentTarget.scrollTop)}
      onKeyDown={onKeyDown}
    >
      <div class="tree-content" style={{ height: `${rows().length * ROW_HEIGHT}px` }}>
        <div class="tree-window" style={{ transform: `translateY(${first() * ROW_HEIGHT}px)` }}>
          <Index each={visible()}>{(row) => <TreeRow row={row()} selected={row().key === selectedKey()} />}</Index>
        </div>
      </div>
    </div>
  )
}

function TreeRow(props: { row: Row; selected: boolean }) {
  return (
    <Show
      when={props.row.kind === 'page' ? props.row : null}
      fallback={<HeaderRow row={props.row} />}
    >
      {(row) => (
        <div
          class="tree-row"
          classList={{ selected: props.selected }}
          role="treeitem"
          aria-selected={props.selected}
          aria-expanded={row().expandable ? row().expanded : undefined}
          style={{ 'padding-left': `${8 + row().level * 16}px` }}
          onMouseDown={(event) => {
            if (event.button === 0) navigation.open(row().ref)
          }}
          onDblClick={() => row().expandable && navigation.setPageExpanded(row().ref, !row().expanded)}
          onContextMenu={(event) => showContextMenu(event, pageMenu(row().ref))}
        >
          <span
            class="tree-chevron"
            classList={{ open: row().expanded, hidden: !row().expandable }}
            onMouseDown={(event) => {
              event.stopPropagation()
              if (row().expandable) navigation.setPageExpanded(row().ref, !row().expanded)
            }}
          >
            <ChevronRightIcon size={14} />
          </span>
          <span class="tree-icon">{row().home ? <HouseIcon size={16} /> : <FileTextIcon size={16} />}</span>
          <span class="tree-title">{row().home && row().node.title === row().space.name ? 'Главная' : row().node.title}</span>
          <Show when={row().node.status && row().node.status?.color !== 'grey'}>
            <span class={`status-dot status-${row().node.status?.color}`} title={row().node.status?.label} />
          </Show>
        </div>
      )}
    </Show>
  )
}

function HeaderRow(props: { row: Row }) {
  return (
    <Show
      when={props.row.kind === 'space' ? props.row : null}
      fallback={
        <Show when={props.row.kind === 'missing' ? props.row : null} fallback={<div class="tree-section">Недоступные папки</div>}>
          {(row) => (
            <div
              class="tree-row missing"
              title={row().path}
              onContextMenu={(event) =>
                showContextMenu(event, [{ label: 'Убрать из списка', action: () => void api.forgetMissing(row().path).catch(report) }])
              }
            >
              <span class="tree-icon">
                <TriangleAlertIcon size={16} />
              </span>
              <span class="tree-title">{row().path}</span>
            </div>
          )}
        </Show>
      }
    >
      {(row) => (
        <div
          class="tree-section"
          onClick={() => navigation.setSpaceExpanded(row().space.id, !row().expanded)}
          onContextMenu={(event) => showContextMenu(event, spaceMenu(row().space))}
        >
          <span class="tree-chevron" classList={{ open: row().expanded }}>
            <ChevronRightIcon size={12} />
          </span>
          <SpaceIcon icon={row().space.icon} size={14} />
          <span class="tree-section-title">{row().space.name}</span>
        </div>
      )}
    </Show>
  )
}

export function pageMenu(ref: PageRef): MenuItem[] {
  const items: MenuItem[] = [{ label: 'Новая подстраница…', action: () => actions.newPage(ref), shortcut: 'Ctrl+N' }]
  if (ref.path) items.push({ label: 'Переименовать…', action: () => actions.renamePage(ref), shortcut: 'F2' })
  items.push(
    { separator: true },
    { label: 'Показать в Проводнике', action: () => actions.revealPage(ref), shortcut: 'Ctrl+Alt+R' },
    { label: 'Скопировать путь к файлу', action: () => actions.copyPagePath(ref) },
  )
  if (ref.path) items.push({ separator: true }, { label: 'Переместить в Корзину…', action: () => actions.trashPage(ref), danger: true, shortcut: 'Del' })
  return items
}

function spaceMenu(space: SpaceInfo): MenuItem[] {
  return [
    { label: 'Новая страница…', action: () => actions.newPage({ spaceId: space.id, path: '' }) },
    { label: 'Сохранить снимок в истории', action: () => actions.saveSnapshot(space.id) },
    { label: 'Показать в Проводнике', action: () => void api.revealPath(space.rootPath).catch(report) },
  ]
}

const SEARCH_DEBOUNCE_MS = 120

function SearchResults(props: { query: string }) {
  const [hits, setHits] = createSignal<SearchHit[]>([])
  const [searched, setSearched] = createSignal(false)
  let token = 0

  createEffect(
    on([() => props.query, indexRevision], ([query]) => {
      const current = ++token
      const timer = window.setTimeout(async () => {
        try {
          const found = await api.search(query)
          if (current !== token) return
          setHits(found)
          setSearched(true)
        } catch (error) {
          report(error)
        }
      }, SEARCH_DEBOUNCE_MS)
      onCleanup(() => window.clearTimeout(timer))
    }),
  )

  const location = (ref: PageRef) => {
    const titles = ancestorTitles(ref)
    return titles.length ? titles.join(' › ') : (spaceById(ref.spaceId)?.name ?? '')
  }

  return (
    <div class="search-results" role="listbox">
      <For each={hits()}>
        {(hit) => (
          <div
            class="search-hit"
            classList={{ selected: sameRef(hit.ref, navigation.selection()) }}
            role="option"
            onClick={() => navigation.open(hit.ref)}
            onContextMenu={(event) => showContextMenu(event, pageMenu(hit.ref))}
          >
            <div class="search-hit-title">{hit.title}</div>
            <div class="search-hit-location">{location(hit.ref)}</div>
            <Show when={hit.snippet}>
              <div class="search-hit-snippet">
                <Snippet text={hit.snippet} />
              </div>
            </Show>
          </div>
        )}
      </For>
      <Show when={searched() && hits().length === 0}>
        <div class="empty-state small">
          <FolderIcon size={28} />
          <div>Ничего не найдено по запросу «{props.query}»</div>
        </div>
      </Show>
    </div>
  )
}

/** Превращает маркеры совпадений из индекса (\u0001…\u0002) в выделение. */
function Snippet(props: { text: string }) {
  const parts = () => {
    const result: { text: string; match: boolean }[] = []
    let match = false
    let fragment = ''
    for (const character of props.text.replace(/\s+/g, ' ')) {
      if (character === '\u0001' || character === '\u0002') {
        if (fragment) result.push({ text: fragment, match })
        fragment = ''
        match = character === '\u0001'
      } else {
        fragment += character
      }
    }
    if (fragment) result.push({ text: fragment, match })
    return result
  }
  return <For each={parts()}>{(part) => (part.match ? <mark>{part.text}</mark> : part.text)}</For>
}

/** Страница по ссылке `[[Пространство::Заголовок]]`: та же логика, что `LibrarySnapshot::resolve`. */
export function resolveWikiLink(space: string | null, title: string, from: string): PageRef | null {
  const target = space ? library().spaces.find((candidate) => titleKey(candidate.name) === titleKey(space)) : spaceById(from)
  if (!target) return null
  const key = titleKey(title)
  const matches = target.nodes.filter((node) => titleKey(node.title) === key).map((node) => node.path)
  matches.sort()
  return matches.length ? { spaceId: target.id, path: matches[0] } : null
}
