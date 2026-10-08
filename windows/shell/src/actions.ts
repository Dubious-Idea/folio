import { api } from './api'
import * as editor from './store/editor'
import { nodeInfo, report } from './store/library'
import * as navigation from './store/navigation'
import { russianDateTime } from './format'
import type { PageRef } from './types'

/** Команды окна: их зовут панель, контекстные меню и сочетания клавиш. */

export function canEdit(): boolean {
  const ref = navigation.selection()
  return !!ref && navigation.version() === null && !!nodeInfo(ref)
}

export function toggleEditing(): void {
  if (editor.isEditing()) {
    void editor.finish()
    return
  }
  const ref = navigation.selection()
  if (ref && canEdit()) void editor.begin(ref)
}

export function newPage(parent: PageRef | null = navigation.selection()): void {
  if (parent) navigation.setSheet({ kind: 'newPage', parent })
}

export function renamePage(page: PageRef | null = navigation.selection()): void {
  if (page && page.path) navigation.setSheet({ kind: 'rename', page })
}

export function trashPage(page: PageRef | null = navigation.selection()): void {
  if (page && page.path) navigation.setPendingTrash(page)
}

export function revealPage(page: PageRef | null = navigation.selection()): void {
  if (page) api.revealPage(page).catch(report)
}

export function copyPagePath(page: PageRef | null = navigation.selection()): void {
  if (page) api.copyPagePath(page).catch(report)
}

export function saveSnapshot(spaceId: string | undefined = navigation.selection()?.spaceId): void {
  if (!spaceId) return
  api.commitSpace(spaceId, `Снимок от ${russianDateTime(new Date())}`).catch(report)
}

export function openSettings(): void {
  navigation.setSheet({ kind: 'settings' })
}

export function newWindow(): void {
  api.newWindow().catch(report)
}

export function toggleInspector(): void {
  navigation.setInspectorVisible(!navigation.inspectorVisible())
}

export async function confirmTrash(): Promise<void> {
  const page = navigation.pendingTrash()
  navigation.setPendingTrash(null)
  if (!page) return
  const parentPath = page.path.split('/').slice(0, -1).join('/')
  try {
    if (editor.editingRef() && editor.editingRef()?.path === page.path && editor.editingRef()?.spaceId === page.spaceId) {
      editor.pageChanged(null)
    }
    await api.trashPage(page)
    const selection = navigation.selection()
    if (selection && selection.spaceId === page.spaceId && (selection.path === page.path || selection.path.startsWith(`${page.path}/`))) {
      navigation.open({ spaceId: page.spaceId, path: parentPath })
    }
  } catch (error) {
    report(error)
  }
}
