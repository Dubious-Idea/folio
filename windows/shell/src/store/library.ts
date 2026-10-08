import { createMemo, createRoot, createSignal } from 'solid-js'
import { api, errorText, on } from '../api'
import { refFromKey, refKey, type LibraryInfo, type PageEntry, type PageRef, type SpaceInfo, type TreeNode } from '../types'

export interface NodeInfo {
  space: SpaceInfo
  node: TreeNode
  index: number
}

const [library, setLibrary] = createSignal<LibraryInfo>({ loaded: false, revision: 0, spaces: [], missingPaths: [] })
/** Растёт после обновления индекса: пересчитываются поиск и обратные ссылки. */
const [indexRevision, setIndexRevision] = createSignal(0)
/** Страницы, которые изменили снаружи (не само приложение): открытая страница перечитывается. */
const [changedPages, setChangedPages] = createSignal<Set<string>>(new Set(), { equals: false })
/** Папки страниц, где появились или пропали вложения. */
const [changedFolders, setChangedFolders] = createSignal<Set<string>>(new Set(), { equals: false })
/** Пространство, в истории которого появился снимок. */
const [historyChange, setHistoryChange] = createSignal<{ spaceId: string } | null>(null, { equals: false })
const [errorMessage, setErrorMessage] = createSignal<string | null>(null)

export { changedFolders, changedPages, errorMessage, historyChange, indexRevision, library, setErrorMessage }

const index = createRoot(() =>
  createMemo(() => {
    const map = new Map<string, NodeInfo>()
    for (const space of library().spaces) {
      space.nodes.forEach((node, position) => map.set(`${space.id}\n${node.path}`, { space, node, index: position }))
    }
    return map
  }),
)

export function nodeInfo(ref: PageRef | null | undefined): NodeInfo | undefined {
  return ref ? index().get(refKey(ref)) : undefined
}

export function spaceById(id: string): SpaceInfo | undefined {
  return library().spaces.find((space) => space.id === id)
}

/** Заголовки от главной до родителя страницы. */
export function ancestorTitles(ref: PageRef): string[] {
  if (!ref.path) return []
  const parts = ref.path.split('/')
  const titles: string[] = []
  for (let length = 0; length < parts.length; length += 1) {
    const path = parts.slice(0, length).join('/')
    const info = index().get(`${ref.spaceId}\n${path}`)
    if (info) titles.push(info.node.title)
  }
  return titles
}

/** Страница нашлась по id из шапки: её переименовали или перенесли снаружи. */
export function relocate(spaceId: string, pageId: string): PageRef | null {
  const space = spaceById(spaceId)
  const node = space?.nodes.find((candidate) => candidate.pageId === pageId)
  return node ? { spaceId, path: node.path } : null
}

export function report(error: unknown): void {
  setErrorMessage(errorText(error))
}

let pagesCache: { revision: number; pages: PageEntry[] } | null = null

/** Список страниц для подсказки по `[[`: запрашивается один раз на версию дерева и передаётся во фрейм без копирования. */
export async function pagesIndex(): Promise<PageEntry[]> {
  const revision = library().revision
  if (pagesCache?.revision === revision) return pagesCache.pages
  const pages = await api.pagesIndex()
  pagesCache = { revision, pages }
  return pages
}

let reloading: Promise<void> | null = null

export function reloadLibrary(): Promise<void> {
  reloading ??= api
    .library()
    .then((info) => {
      if (info.revision >= library().revision || !library().loaded) setLibrary(info)
    })
    .catch(report)
    .finally(() => {
      reloading = null
    })
  return reloading
}

export async function startLibrary(): Promise<void> {
  await Promise.all([
    on<number>('library-changed', (revision) => {
      if (revision !== library().revision) void reloadLibrary()
    }),
    on('index-changed', () => setIndexRevision((value) => value + 1)),
    on<PageRef[]>('pages-changed', (refs) => setChangedPages(new Set(refs.map(refKey)))),
    on<PageRef[]>('folders-changed', (refs) => setChangedFolders(new Set(refs.map(refKey)))),
    on<string>('history-changed', (spaceId) => setHistoryChange({ spaceId })),
  ])
  await reloadLibrary()
}

export { refFromKey }
