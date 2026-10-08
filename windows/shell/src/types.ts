export type StatusColor = 'grey' | 'blue' | 'green' | 'yellow' | 'red' | 'purple'

export interface PageRef {
  spaceId: string
  /** Путь папки страницы через `/`; пустая строка — главная. */
  path: string
}

export interface Status {
  label: string
  color: StatusColor
}

/** Узел дерева в прямом порядке обхода: глубина и число детей вместо вложенных списков. */
export interface TreeNode {
  path: string
  title: string
  pageId?: string
  status?: Status
  depth: number
  childCount: number
}

export interface SpaceInfo {
  id: string
  name: string
  icon: string
  rootPath: string
  nodes: TreeNode[]
}

export interface LibraryInfo {
  loaded: boolean
  revision: number
  spaces: SpaceInfo[]
  missingPaths: string[]
}

export interface PageEntry {
  space: string
  title: string
}

export interface PageDto {
  pageKey: string
  markdown: string
  title: string
  status: Status | null
  labels: string[]
  meta: string
  assetBase: string
  links: Record<string, boolean>
  children: { title: string }[]
  spaceName: string
  banner: string | null
  textHash: string
  pageId: string | null
}

/** Всё, что получает `window.folio.render` веб-части. */
export interface PagePayload extends Omit<PageDto, 'textHash' | 'pageId'> {
  pages: PageEntry[]
  resetScroll: boolean
  editable: boolean
}

export interface SearchHit {
  ref: PageRef
  title: string
  snippet: string
}

export interface Attachment {
  name: string
  path: string
  size: number
}

export interface Revision {
  hash: string
  path: string
  /** Секунды от 1970 года. */
  date: number
  message: string
  author: string
}

export interface PageHistory {
  revisions: Revision[]
  hasUncommittedChanges: boolean
}

export interface HistoryVersion {
  hash: string
  path: string
  date: number
  message: string
}

export interface AttachmentItem {
  path: string
  name: string
  isImage: boolean
}

export type SaveResult = { status: 'saved' | 'unchanged'; hash: string } | { status: 'conflict' }

export interface OutlineItem {
  level: number
  text: string
  index: number
}

export interface FormattingState {
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

export const emptyFormatting: FormattingState = {
  bold: false,
  italic: false,
  strike: false,
  code: false,
  link: false,
  block: 'paragraph',
  inTable: false,
  canUndo: false,
  canRedo: false,
}

export function refKey(ref: PageRef): string {
  return `${ref.spaceId}\n${ref.path}`
}

export function refFromKey(key: string): PageRef | null {
  const separator = key.indexOf('\n')
  if (separator < 0) return null
  return { spaceId: key.slice(0, separator), path: key.slice(separator + 1) }
}

export function sameRef(lhs: PageRef | null | undefined, rhs: PageRef | null | undefined): boolean {
  return !!lhs && !!rhs && lhs.spaceId === rhs.spaceId && lhs.path === rhs.path
}
