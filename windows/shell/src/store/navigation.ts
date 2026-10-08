import { createSignal } from 'solid-js'
import { refKey, sameRef, type HistoryVersion, type OutlineItem, type PageRef } from '../types'
import { preference, savePreference } from './preferences'

export type Sheet = { kind: 'newPage'; parent: PageRef } | { kind: 'rename'; page: PageRef } | { kind: 'settings' } | { kind: 'update' }

/** Состояние одного окна: открытая страница, история переходов, раскрытые ветки, инспектор. */
const [selection, setSelection] = createSignal<PageRef | null>(null)
const [selectedPageId, setSelectedPageId] = createSignal<string | null>(null)
const [backStack, setBackStack] = createSignal<PageRef[]>([])
const [forwardStack, setForwardStack] = createSignal<PageRef[]>([])
const [headingRequest, setHeadingRequest] = createSignal<{ index: number } | null>(null, { equals: false })
const [expanded, setExpanded] = createSignal<Set<string>>(new Set(preference<string[]>('expanded', [])))
const [collapsedSpaces, setCollapsedSpaces] = createSignal<Set<string>>(new Set(preference<string[]>('collapsedSpaces', [])))
const [searchText, setSearchText] = createSignal('')
const [inspectorVisible, setInspectorVisibleSignal] = createSignal(preference('inspectorVisible', true))
const [version, setVersion] = createSignal<HistoryVersion | null>(null)
const [outline, setOutline] = createSignal<OutlineItem[]>([])
const [sheet, setSheet] = createSignal<Sheet | null>(null)
const [pendingTrash, setPendingTrash] = createSignal<PageRef | null>(null)

export {
  backStack,
  collapsedSpaces,
  expanded,
  forwardStack,
  headingRequest,
  inspectorVisible,
  outline,
  pendingTrash,
  searchText,
  selectedPageId,
  selection,
  setOutline,
  setPendingTrash,
  setSearchText,
  setSelectedPageId,
  setSheet,
  setVersion,
  sheet,
  version,
}

export const canGoBack = () => backStack().length > 0
export const canGoForward = () => forwardStack().length > 0

export function open(ref: PageRef | null | undefined): void {
  if (!ref || sameRef(ref, selection())) return
  const current = selection()
  if (current) setBackStack((stack) => [...stack, current])
  setForwardStack([])
  show(ref)
}

export function goBack(): void {
  const stack = backStack()
  const previous = stack.at(-1)
  if (!previous) return
  setBackStack(stack.slice(0, -1))
  const current = selection()
  if (current) setForwardStack((forward) => [...forward, current])
  show(previous)
}

export function goForward(): void {
  const stack = forwardStack()
  const next = stack.at(-1)
  if (!next) return
  setForwardStack(stack.slice(0, -1))
  const current = selection()
  if (current) setBackStack((back) => [...back, current])
  show(next)
}

/** Адрес страницы поменялся снаружи: подменяем его без записи в историю переходов. */
export function relocateTo(ref: PageRef): void {
  setSelection(ref)
  expandAncestors(ref)
}

export function requestHeading(index: number): void {
  setHeadingRequest({ index })
}

export function isExpanded(ref: PageRef): boolean {
  return expanded().has(refKey(ref))
}

export function setPageExpanded(ref: PageRef, isOpen: boolean): void {
  const key = refKey(ref)
  if (expanded().has(key) === isOpen) return
  const next = new Set(expanded())
  if (isOpen) next.add(key)
  else next.delete(key)
  setExpanded(next)
  savePreference('expanded', [...next])
}

export function isSpaceExpanded(id: string): boolean {
  return !collapsedSpaces().has(id)
}

export function setSpaceExpanded(id: string, isOpen: boolean): void {
  const next = new Set(collapsedSpaces())
  if (isOpen) next.delete(id)
  else next.add(id)
  setCollapsedSpaces(next)
  savePreference('collapsedSpaces', [...next])
}

export function setInspectorVisible(visible: boolean): void {
  setInspectorVisibleSignal(visible)
  savePreference('inspectorVisible', visible)
}

function show(ref: PageRef): void {
  setSelection(ref)
  setSelectedPageId(null)
  setVersion(null)
  setOutline([])
  expandAncestors(ref)
  savePreference('selectedPage', refKey(ref))
}

function expandAncestors(ref: PageRef): void {
  const parts = ref.path ? ref.path.split('/') : []
  const next = new Set(expanded())
  let changed = false
  for (let length = 1; length < parts.length; length += 1) {
    const key = refKey({ spaceId: ref.spaceId, path: parts.slice(0, length).join('/') })
    if (!next.has(key)) {
      next.add(key)
      changed = true
    }
  }
  if (changed) {
    setExpanded(next)
    savePreference('expanded', [...next])
  }
  if (collapsedSpaces().has(ref.spaceId)) setSpaceExpanded(ref.spaceId, true)
}
